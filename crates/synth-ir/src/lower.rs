// SPDX-License-Identifier: Apache-2.0

//! AST → IR lowering.
//!
//! Inputs:
//!
//! - A parsed [`synth_ast::ProgramAst`].
//! - A loaded [`synth_registry::Registry`] used to resolve part ids
//!   and validate endpoint pin names.
//!
//! Outputs:
//!
//! - An [`Option<Board>`] (always present unless the input had no
//!   parseable program).
//! - A vector of [`Diagnostic`]s. The lowering is *best-effort*:
//!   diagnostics never abort the pipeline; downstream stages run on
//!   the partial IR. Components that fail part resolution appear in
//!   the IR with `part = None`; connections with an unresolved
//!   endpoint are dropped from the net graph but the diagnostic
//!   remains the agent-facing artifact.
//!
//! Diagnostic codes emitted here:
//!
//! - `E-SYNTH-COMP-001` unknown part
//! - `E-SYNTH-COMP-002` undefined pin
//! - `E-SYNTH-COMP-003` duplicate refdes
//! - `E-SYNTH-COMP-004` undefined component refdes
//! - `E-SYNTH-UNIT-001` value/unit conversion failure
//!
//! Net construction uses union-find on endpoints: every `connect`
//! statement joins its two endpoints into the same net, so if the
//! user writes `A.x -> B.y` and `B.y -> C.z`, the result is one net
//! `{A.x, B.y, C.z}`.

use std::collections::HashMap;

use synth_ast::{
    ComponentDeclAst, DiffPairAttr, DiffPairStmt, EndpointAst, EndpointRefKind, KeepoutAttr,
    KeepoutStmt, NetclassStmt, ProgramAst, StatementAst, ValueWithUnit,
};
use synth_diagnostics::{
    Diagnostic, DiagnosticBuilder, Location, Patch, PatchKind, Severity, Span, SuggestedAction,
};
use synth_registry::{Part, PinCapability, Registry};

use crate::board::{
    Board, Component, ComponentId, DiffPair, Keepout, Net, NetClass, NetEndpoint, NetId, Note,
    PinId, PlacementEdge, PlacementRegion, PlacementSide, Variant,
};
use crate::units::{ConversionError, Impedance, Length, Voltage};

/// One `connect` statement after flattening: a source, one or more
/// targets (one-to-many fanout), and the optional `as "NET"` /
/// `class "CLASS"` join clauses.
struct ConnectionRecord {
    from: EndpointAst,
    tos: Vec<EndpointAst>,
    span: Span,
    net_name: Option<String>,
    netclass: Option<String>,
}

/// One `net "NAME" { … }` block after flattening.
struct NetDeclRecord {
    name: String,
    netclass: Option<String>,
    endpoints: Vec<EndpointAst>,
    span: Span,
}

/// One `power "NAME" <voltage> { … }` block after flattening.
struct PowerDeclRecord {
    name: String,
    voltage: ValueWithUnit,
    netclass: Option<String>,
    endpoints: Vec<EndpointAst>,
    span: Span,
}

/// Name/class/voltage assertions one statement contributes to the
/// union-find in [`LowerCtx::build_named_nets`]: the member endpoint
/// indices it joins plus the explicit names it declares.
struct Assertion {
    members: Vec<usize>,
    names: Vec<String>,
    classes: Vec<String>,
    voltages: Vec<Voltage>,
    span: Span,
}

/// One union-find group's accumulated assertions, keyed by root.
struct GroupInfo {
    members: Vec<usize>,
    names: Vec<(String, Span)>,
    classes: Vec<(String, Span)>,
    voltages: Vec<(Voltage, Span)>,
}

/// A declared name with no populated net to join (bare
/// `power "X" 3.3v`, or a block whose endpoints all failed
/// resolution): materialized as an empty named net.
struct OrphanDecl {
    name: String,
    classes: Vec<(String, Span)>,
    voltages: Vec<(Voltage, Span)>,
}

/// Resolved endpoints, joins, and per-statement assertions feeding
/// the union-find in [`LowerCtx::build_named_nets`].
struct EndpointSet {
    endpoints: Vec<(ComponentId, PinId, Span)>,
    joins: Vec<(usize, usize)>,
    assertions: Vec<Assertion>,
}

/// One frame of the block-flattening walk in [`lower`]: the statement
/// slice, the next index into it, and the enclosing group/sheet
/// names. Aliased because the bare 4-tuple trips
/// `clippy::type_complexity`.
type BlockFrame<'a> = (&'a [StatementAst], usize, Option<&'a str>, Option<&'a str>);

#[derive(Debug)]
pub struct LowerResult {
    pub board: Option<Board>,
    pub diagnostics: Vec<Diagnostic>,
}

impl LowerResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity.is_blocking())
    }
}

/// Lower an AST to an IR `Board`. `file` is used as the diagnostic
/// location anchor; pass the same file string used at parse time so
/// agents can correlate diagnostics across stages.
pub fn lower(ast: &ProgramAst, registry: &Registry, file: &str) -> LowerResult {
    let mut ctx = LowerCtx::new(file);
    // Reusable blocks first: every `use` becomes concrete, refdes-prefixed
    // components and connections (see `modules.rs` and `docs/modules.md`),
    // and `bind` becomes plain connections. Declarations are lifted out as
    // board metadata; lowering then walks the expanded statements.
    let expansion = crate::modules::expand(&ast.board.statements, file);
    ctx.diagnostics.extend(expansion.diagnostics);
    let buses = expansion.buses;
    let modules = expansion.modules;
    let root_statements = expansion.statements;
    let mut components: Vec<Component> = Vec::new();
    let mut refdes_index: HashMap<String, ComponentId> = HashMap::new();
    let mut connections: Vec<ConnectionRecord> = Vec::new();
    let mut net_decls: Vec<NetDeclRecord> = Vec::new();
    let mut power_decls: Vec<PowerDeclRecord> = Vec::new();
    let mut diff_pair_stmts: Vec<DiffPairStmt> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();
    let mut keepouts: Vec<Keepout> = Vec::new();
    let mut netclasses: Vec<NetClass> = Vec::new();
    let mut layers: u32 = 0;
    let mut manufacturer: Option<String> = None;
    let mut revision: Option<String> = None;
    let mut company: Option<String> = None;
    // Schematic-quality plan Phase A3: connector pin legends are
    // opt-in (`legends on`), default off.
    let mut legends: bool = false;

    // Groups and sheets are flattened here, not represented in the
    // IR as a tree: a group names its components and a sheet names
    // its future hierarchical-sheet boundary (see `GroupStmt` /
    // `SheetStmt`), so lowering walks into each carrying the names
    // down and leaves the board a flat component list exactly as
    // before. `stack` is the enclosing block chain; nested blocks
    // take the innermost name, and sheets nest independently of
    // groups (a component may carry both).
    let mut stack: Vec<BlockFrame<'_>> = vec![(&root_statements, 0, None, None)];
    while let Some((statements, index, group, sheet)) = stack.pop() {
        let Some(stmt) = statements.get(index) else {
            continue;
        };
        stack.push((statements, index + 1, group, sheet));
        match stmt {
            StatementAst::Layers(l) => layers = l.count,
            StatementAst::Manufacturer(m) => manufacturer = Some(m.name.clone()),
            StatementAst::Revision(r) => revision = Some(r.rev.clone()),
            StatementAst::Company(c) => company = Some(c.name.clone()),
            StatementAst::Legends(l) => legends = l.enabled,
            StatementAst::Component(c) => {
                let comp = ctx.lower_component(c, registry, components.len(), group, sheet);
                if refdes_index.contains_key(&comp.refdes) {
                    ctx.emit_duplicate_refdes(&comp.refdes, comp.source_span);
                } else {
                    refdes_index.insert(comp.refdes.clone(), comp.id);
                }
                components.push(comp);
            }
            StatementAst::Connection(_)
            | StatementAst::Net(_)
            | StatementAst::Power(_)
            | StatementAst::Notes(_)
            | StatementAst::DiffPair(_) => {
                push_statement_records(
                    stmt,
                    group,
                    sheet,
                    &mut connections,
                    &mut net_decls,
                    &mut power_decls,
                    &mut diff_pair_stmts,
                    &mut notes,
                );
            }
            StatementAst::Keepout(k) => keepouts.push(ctx.lower_keepout(k)),
            StatementAst::Netclass(n) => netclasses.push(ctx.lower_netclass(n)),
            StatementAst::Group(g) => {
                stack.push((&g.statements, 0, Some(g.name.as_str()), sheet));
            }
            StatementAst::Sheet(s) => {
                stack.push((&s.statements, 0, group, Some(s.name.as_str())));
            }
            // StatementAst is #[non_exhaustive]; future statement
            // kinds reach here until lowering is taught about them.
            _ => {}
        }
    }

    // Build nets by union-find over resolved endpoints. Each
    // successful `connect` joins its source with every target, each
    // `net`/`power` block joins its listed endpoints, and every
    // statement naming the same explicit net merges into one net.
    let nets = ctx.build_named_nets(
        &connections,
        &net_decls,
        &power_decls,
        &components,
        &refdes_index,
        &netclasses,
    );

    let diff_pairs = ctx.lower_diff_pairs(&diff_pair_stmts, &nets);

    // Design variants (§Phase 7): gathered after the walk so a variant's
    // refdes can be checked against the final component set.
    let variants = ctx.lower_variants(&root_statements, &refdes_index);

    let board = Board {
        legends,
        name: ast.board.name.clone(),
        layers,
        manufacturer,
        revision,
        company,
        components,
        nets,
        diff_pairs,
        notes,
        keepouts,
        netclasses,
        buses,
        modules,
        variants,
        source_span: ast.board.span,
    };

    LowerResult {
        board: Some(board),
        diagnostics: ctx.diagnostics,
    }
}

struct LowerCtx<'a> {
    file: &'a str,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> LowerCtx<'a> {
    fn new(file: &'a str) -> Self {
        Self {
            file,
            diagnostics: Vec::new(),
        }
    }

    fn lower_component(
        &mut self,
        decl: &ComponentDeclAst,
        registry: &Registry,
        next_index: usize,
        group: Option<&str>,
        sheet: Option<&str>,
    ) -> Component {
        let part_id = decl.part.as_deref().unwrap_or("");
        let part = registry.lookup(part_id).cloned();

        if part.is_none() {
            let mut b = DiagnosticBuilder::new("E-SYNTH-COMP-001", Severity::Error, "unknown part")
                .location(Location::from_span(self.file.to_string(), decl.span))
                .expected("a part id present in the registry")
                .found(format!(
                    "`{part_id}` not found in registry (for {})",
                    describe_decl(&decl.refdes, group)
                ))
                .message(format!(
                    "`{part_id}` is not in the registry. Author it instead of guessing pins: \
                     `synth part stub {part_id} --pins <N>` writes a skeleton into the Tier-2 \
                     registry (all pins `required = false`), then fill in the real pinout from \
                     the datasheet — or use the `synth_author_part` MCP tool."
                ))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-COMP-001");
            for suggestion in suggest_similar_parts(part_id, registry) {
                b = b.suggested_fix(Patch {
                    confidence: 0.5,
                    rationale: Some(format!("did you mean `{suggestion}`?")),
                    patch_consequence_preview: None,
                    kind: PatchKind::ReplaceRange {
                        range: decl.span,
                        replacement: format!(
                            "component {}: {} \"{suggestion}\"",
                            decl.refdes, decl.kind,
                        ),
                    },
                });
            }
            // Unknown-part design loop (§18.8.4): the part id itself
            // isn't a text-editable fix, so offer the registry-growth
            // path as tool-call actions instead — search first, import
            // if a source match turns up, else fall back to a stub the
            // agent fills from a datasheet.
            b = b
                .suggested_action(SuggestedAction::SearchRegistryWeb {
                    query: part_id.to_string(),
                })
                .suggested_action(SuggestedAction::ImportPartStub {
                    part_id: part_id.to_string(),
                })
                .suggested_action(SuggestedAction::CreatePartStub {
                    part_id: part_id.to_string(),
                });
            self.diagnostics.push(b.build());
        }

        let placement_hint = decl
            .placement_hint
            .as_ref()
            .map(|h| self.lower_placement_hint(h));

        Component {
            id: ComponentId(next_index as u32),
            refdes: decl.refdes.clone(),
            kind: decl.kind.clone(),
            part,
            value: decl.value.clone(),
            dnp: decl.dnp,
            properties: decl.properties.clone(),
            placement_hint,
            group: group.map(str::to_string),
            sheet: sheet.map(str::to_string),
            source_span: decl.span,
        }
    }

    /// Collect the board's design variants, checking each listed refdes
    /// against the final component set. Duplicate variant names
    /// (`E-SYNTH-VARIANT-001`) and unknown refdes
    /// (`E-SYNTH-VARIANT-002`) are reported; a variant with no valid
    /// override is dropped so it never reaches the export.
    fn lower_variants(
        &mut self,
        statements: &[StatementAst],
        refdes_index: &HashMap<String, ComponentId>,
    ) -> Vec<Variant> {
        let mut decls = Vec::new();
        collect_variant_decls(statements, &mut decls);
        let mut out = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for decl in decls {
            if !seen.insert(decl.name.as_str()) {
                self.diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-VARIANT-001",
                        Severity::Error,
                        "duplicate variant name",
                    )
                    .location(Location::from_span(self.file.to_string(), decl.span))
                    .expected("each variant to have a unique name")
                    .found(format!("variant `{}` declared more than once", decl.name))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-VARIANT-001")
                    .build(),
                );
                continue;
            }
            let mut dnp: Vec<String> = Vec::new();
            for refdes in &decl.dnp {
                if !refdes_index.contains_key(refdes.as_str()) {
                    self.diagnostics.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-VARIANT-002",
                            Severity::Error,
                            "variant names an unknown component",
                        )
                        .location(Location::from_span(self.file.to_string(), decl.span))
                        .expected(format!(
                            "`{refdes}` to be declared with a `component` statement"
                        ))
                        .found(format!(
                            "variant `{}` marks undeclared refdes `{refdes}` do-not-populate",
                            decl.name
                        ))
                        .explanation_url("synth.docs/diagnostics/E-SYNTH-VARIANT-002")
                        .build(),
                    );
                    continue;
                }
                if !dnp.contains(refdes) {
                    dnp.push(refdes.clone());
                }
            }
            out.push(Variant {
                name: decl.name.clone(),
                description: decl.description.clone(),
                dnp,
            });
        }
        out
    }

    fn lower_netclass(&mut self, n: &NetclassStmt) -> NetClass {
        use synth_ast::NetclassAttr;
        let mut trace_width: Option<Length> = None;
        let mut clearance: Option<Length> = None;
        let mut color: Option<[u8; 3]> = None;
        for attr in &n.attrs {
            match attr {
                NetclassAttr::TraceWidth(v) => match Length::try_from(v) {
                    Ok(l) => trace_width = Some(l),
                    Err(e) => self.emit_unit_error(&e, "netclass trace_width"),
                },
                NetclassAttr::Clearance(v) => match Length::try_from(v) {
                    Ok(l) => clearance = Some(l),
                    Err(e) => self.emit_unit_error(&e, "netclass clearance"),
                },
                NetclassAttr::Color(hex) => match parse_hex_color(hex) {
                    Some(rgb) => color = Some(rgb),
                    None => self.diagnostics.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-NAME-011",
                            Severity::Warning,
                            "invalid netclass color",
                        )
                        .location(Location::from_span(self.file.to_string(), n.span))
                        .expected("a six-digit hex colour, e.g. \"#c2410c\"")
                        .found(format!("\"{hex}\""))
                        .message(format!(
                            "netclass `{}` colour `{hex}` is not `#rrggbb`; the class \
                             keeps its default palette hue",
                            n.name
                        ))
                        .explanation_url("synth.docs/diagnostics/E-SYNTH-NAME-011")
                        .build(),
                    ),
                },
                // Non-exhaustive enum: future attrs reach here as a
                // no-op until lowering learns about them.
                _ => {}
            }
        }
        NetClass {
            name: n.name.clone(),
            trace_width,
            clearance,
            color,
            source_span: n.span,
        }
    }

    fn lower_placement_hint(
        &mut self,
        ast: &synth_ast::PlacementHintAst,
    ) -> crate::board::PlacementConstraint {
        let mut c = crate::board::PlacementConstraint::default();
        for attr in &ast.attrs {
            match attr {
                synth_ast::PlacementHintAttr::Region(s, span) => {
                    if let Some(r) = parse_region(s) {
                        c.region = Some(r);
                    } else {
                        self.emit_hint_warning(
                            "W-SYNTH-HINT-001",
                            "unknown placement region",
                            "one of: top_left, top_right, bottom_left, bottom_right, centre, top_edge, bottom_edge, left_edge, right_edge",
                            format!("`{s}`"),
                            *span,
                        );
                    }
                }
                synth_ast::PlacementHintAttr::Edge(s, span) => {
                    if let Some(e) = parse_edge(s) {
                        c.edge = Some(e);
                    } else {
                        self.emit_hint_warning(
                            "W-SYNTH-HINT-001",
                            "unknown placement edge",
                            "one of: top, bottom, left, right",
                            format!("`{s}`"),
                            *span,
                        );
                    }
                }
                synth_ast::PlacementHintAttr::Near(s, _) => {
                    c.near = Some(s.clone());
                }
                synth_ast::PlacementHintAttr::Side(s, span) => {
                    if let Some(side) = parse_side(s) {
                        c.side = Some(side);
                    } else {
                        self.emit_hint_warning(
                            "W-SYNTH-HINT-001",
                            "unknown placement side",
                            "one of: above, below, left, right",
                            format!("`{s}`"),
                            *span,
                        );
                    }
                }
                synth_ast::PlacementHintAttr::Priority(s, span) => {
                    match s.to_lowercase().as_str() {
                        "hard" => c.priority = crate::board::PlacementPriority::Hard,
                        "soft" => c.priority = crate::board::PlacementPriority::Soft,
                        _ => {
                            self.emit_hint_warning(
                                "W-SYNTH-HINT-001",
                                "unknown placement priority",
                                "one of: hard, soft",
                                format!("`{s}`"),
                                *span,
                            );
                            c.priority = crate::board::PlacementPriority::Soft;
                        }
                    }
                }
            }
        }
        c
    }

    fn emit_hint_warning(
        &mut self,
        code: &str,
        title: &str,
        expected: &str,
        found: String,
        span: synth_diagnostics::Span,
    ) {
        self.diagnostics.push(
            DiagnosticBuilder::new(code, Severity::Warning, title)
                .location(Location::from_span(self.file.to_string(), span))
                .expected(expected)
                .found(found)
                .explanation_url(format!("synth.docs/diagnostics/{code}"))
                .build(),
        );
    }

    /// Lower `diff_pair` statements and resolve each leg to its
    /// real net when the leg names a declared net (`net "USB_DP"` /
    /// `connect … as "USB_DP"`). Unresolved legs keep `None` and
    /// validation falls back to endpoint-name matching for legacy
    /// designs without named nets.
    fn lower_diff_pairs(&mut self, stmts: &[DiffPairStmt], nets: &[Net]) -> Vec<DiffPair> {
        let name_to_id: HashMap<&str, NetId> =
            nets.iter().map(|n| (n.name.as_str(), n.id)).collect();
        let mut out = Vec::with_capacity(stmts.len());
        for d in stmts {
            let mut dp = self.lower_diff_pair(d);
            dp.positive_net = name_to_id.get(d.pos.as_str()).copied();
            dp.negative_net = name_to_id.get(d.neg.as_str()).copied();
            out.push(dp);
        }
        out
    }

    fn lower_diff_pair(&mut self, d: &DiffPairStmt) -> DiffPair {
        let mut impedance: Option<Impedance> = None;
        for attr in &d.attrs {
            if let DiffPairAttr::Impedance(v) = attr {
                match Impedance::try_from(v) {
                    Ok(z) => impedance = Some(z),
                    Err(e) => self.emit_unit_error(&e, "impedance"),
                }
            }
            // Non-exhaustive enum: future attrs reach here as a no-op
            // until lowering learns about them.
        }
        DiffPair {
            positive: d.pos.clone(),
            negative: d.neg.clone(),
            // Resolved to real nets by the caller (`lower`) once the
            // net table exists; `None` until then.
            positive_net: None,
            negative_net: None,
            impedance,
            source_span: d.span,
        }
    }

    fn lower_keepout(&mut self, k: &KeepoutStmt) -> Keepout {
        let mut radius: Option<Length> = None;
        for attr in &k.attrs {
            if let KeepoutAttr::Radius(v) = attr {
                match Length::try_from(v) {
                    Ok(l) => radius = Some(l),
                    Err(e) => self.emit_unit_error(&e, "keepout radius"),
                }
            }
        }
        Keepout {
            name: k.name.clone(),
            radius,
            source_span: k.span,
        }
    }

    /// Build nets by union-find over resolved endpoints, honouring
    /// explicit net names, netclass joins, and declared rail voltages.
    ///
    /// - Every `connect` joins its source with each target; every
    ///   `net`/`power` block joins its listed endpoints.
    /// - Statements naming the same net (`net "X"`, `power "X"`,
    ///   `connect … as "X"`) merge into one net even when they share
    ///   no endpoint.
    /// - Two *different* explicit names shorted together is an error
    ///   (`E-SYNTH-NAME-005`); the first-seen name wins.
    /// - `class "C"` must name a declared netclass
    ///   (`E-SYNTH-NAME-006`); conflicting joins on one net are also
    ///   `E-SYNTH-NAME-006`. The first known class wins.
    /// - Conflicting declared voltages on one net are
    ///   `E-SYNTH-POWER-007`; the first wins.
    /// - Declared names with no endpoints (bare `power "X" 3.3v`)
    ///   still materialize an (empty) named net so later joins and
    ///   power-domain inference see the rail.
    #[allow(clippy::too_many_arguments)]
    fn build_named_nets(
        &mut self,
        connections: &[ConnectionRecord],
        net_decls: &[NetDeclRecord],
        power_decls: &[PowerDeclRecord],
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
        netclasses: &[NetClass],
    ) -> Vec<Net> {
        let EndpointSet {
            endpoints,
            joins,
            assertions,
        } = self.collect_assertions(
            connections,
            net_decls,
            power_decls,
            components,
            refdes_index,
        );

        // Union-find with path compression, then merge members that
        // share an explicit net name even when they share no endpoint
        // (two `net "X"` blocks, or `as "X"` on disjoint connects).
        // Returns member lists in deterministic order (by minimum
        // endpoint index) so output is stable.
        let ordered_groups = union_endpoint_sets(endpoints.len(), &joins, &assertions);

        // Per-group assertions in statement order. Groups are keyed
        // by position in `ordered_groups`; `member_to_root` maps each
        // endpoint index back to its group.
        let mut group_info: HashMap<usize, GroupInfo> = HashMap::new();
        let mut member_to_root: HashMap<usize, usize> = HashMap::new();
        for (gi, members) in ordered_groups.iter().enumerate() {
            for m in members {
                member_to_root.insert(*m, gi);
            }
            group_info.insert(
                gi,
                GroupInfo {
                    members: members.clone(),
                    names: Vec::new(),
                    classes: Vec::new(),
                    voltages: Vec::new(),
                },
            );
        }
        // Name → root for bare declarations (no resolved endpoints)
        // to join, and orphan names (no group at all) to materialize
        // as empty nets, both in statement order.
        let mut name_to_root: HashMap<String, usize> = HashMap::new();
        let mut orphans: Vec<OrphanDecl> = Vec::new();
        let mut orphan_index: HashMap<String, usize> = HashMap::new();
        distribute_assertions(
            &assertions,
            &member_to_root,
            &mut group_info,
            &mut name_to_root,
            &mut orphans,
            &mut orphan_index,
        );

        // Materialize nets in `ordered_groups` order (already
        // deterministic: by minimum endpoint index).
        let known_classes: HashMap<&str, &NetClass> =
            netclasses.iter().map(|nc| (nc.name.as_str(), nc)).collect();

        let mut nets: Vec<Net> = Vec::new();

        for (idx, members) in ordered_groups.iter().enumerate() {
            let info = group_info
                .get(&member_to_root[&members[0]])
                .expect("group exists");
            nets.push(self.materialize_group(info, idx, &endpoints, &known_classes));
        }

        // Orphan names — declared (`power "X" 3.3v`, or blocks whose
        // endpoints all failed resolution) but with no populated net
        // to join — materialize as empty named nets in first-seen
        // order, with the same join diagnostics as populated nets.
        for orphan in &orphans {
            let (netclass, voltage) = self.resolve_joins(
                &orphan.name,
                &orphan.classes,
                &orphan.voltages,
                &known_classes,
            );
            let id = NetId(nets.len() as u32);
            nets.push(Net {
                id,
                name: orphan.name.clone(),
                endpoints: Vec::new(),
                netclass,
                voltage,
            });
        }

        nets
    }

    /// Materialize one union-find group as a [`Net`]: emit the
    /// conflicting-names error (`E-SYNTH-NAME-005`) when distinct
    /// explicit names were shorted together (first-seen wins), then
    /// resolve the class/voltage joins. Unnamed groups keep the
    /// legacy `net_<idx>` auto-name.
    fn materialize_group(
        &mut self,
        info: &GroupInfo,
        idx: usize,
        endpoints: &[(ComponentId, PinId, Span)],
        known_classes: &HashMap<&str, &NetClass>,
    ) -> Net {
        let mut distinct_names: Vec<(String, Span)> = Vec::new();
        for (n, s) in &info.names {
            if !distinct_names.iter().any(|(m, _)| m == n) {
                distinct_names.push((n.clone(), *s));
            }
        }
        if distinct_names.len() > 1 {
            // A pin can only serve one peripheral function at a time.
            // When the shorted names are *function* names from different
            // protocols (`I2C1_SCL` and `UART1_TX`), that is the pin-mux
            // mistake — report it at the shared pin, and leave
            // `E-SYNTH-NAME-005` for the non-function case so exactly
            // one of the two fires.
            let mut functions: Vec<PinCapability> = Vec::new();
            for (n, _) in &distinct_names {
                if let Some(f) = PinCapability::from_net_name(n) {
                    if !functions.contains(&f) {
                        functions.push(f);
                    }
                }
            }
            if functions.len() > 1 {
                let fn_list = functions
                    .iter()
                    .map(|f| format!("`{}`", f.canonical_name()))
                    .collect::<Vec<_>>()
                    .join(" and ");
                let pin_span = info
                    .members
                    .first()
                    .map_or(distinct_names[1].1, |m| endpoints[*m].2);
                self.diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-PINMUX-001",
                        Severity::Error,
                        "one pin asked to carry two functions",
                    )
                    .location(Location::from_span(self.file.to_string(), pin_span))
                    .expected("the pin to carry a single peripheral function")
                    .found(format!("{fn_list} shorted onto the same pin"))
                    .message(format!(
                        "this pin is shared by two function nets ({fn_list}); a pin can only \
                         serve one peripheral function at a time — route one of them to a \
                         different pin, or split the nets"
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-PINMUX-001")
                    .build(),
                );
            } else {
                let first = &distinct_names[0].0;
                for (other, span) in distinct_names.iter().skip(1) {
                    self.diagnostics.push(
                        DiagnosticBuilder::new(
                            "E-SYNTH-NAME-005",
                            Severity::Error,
                            "conflicting net names shorted together",
                        )
                        .location(Location::from_span(self.file.to_string(), *span))
                        .expected(format!("endpoints of net `{first}` only"))
                        .found(format!("net `{other}` shorted to net `{first}`"))
                        .message(format!(
                            "net `{other}` is shorted to net `{first}` by shared endpoints; give \
                             the connection one name (rename one side) or split the nets"
                        ))
                        .explanation_url("synth.docs/diagnostics/E-SYNTH-NAME-005")
                        .build(),
                    );
                }
            }
        }
        let name = distinct_names
            .first()
            .map_or_else(|| format!("net_{idx}"), |(n, _)| n.clone());

        let (netclass, voltage) =
            self.resolve_joins(&name, &info.classes, &info.voltages, known_classes);

        let id = NetId(idx as u32);
        let ir_endpoints = info
            .members
            .iter()
            .map(|i| {
                let (c, p, span) = endpoints[*i];
                NetEndpoint {
                    component: c,
                    pin: p,
                    source_span: span,
                }
            })
            .collect();
        Net {
            id,
            name,
            endpoints: ir_endpoints,
            netclass,
            voltage,
        }
    }

    /// Resolve every endpoint of every `connect` / `net` / `power`
    /// statement to `(ComponentId, PinId)`, intern them, and record
    /// the joins plus the per-statement name/class/voltage
    /// assertions. Endpoints that fail resolution are dropped (with
    /// a diagnostic) and do not participate in net construction.
    fn collect_assertions(
        &mut self,
        connections: &[ConnectionRecord],
        net_decls: &[NetDeclRecord],
        power_decls: &[PowerDeclRecord],
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
    ) -> EndpointSet {
        // Each endpoint is keyed by (ComponentId, PinId).
        let mut endpoints: Vec<(ComponentId, PinId, Span)> = Vec::new();
        let mut endpoint_index: HashMap<(ComponentId, PinId), usize> = HashMap::new();
        let mut joins: Vec<(usize, usize)> = Vec::new();

        // Per-statement name/class/voltage assertions, in statement
        // order (see [`Assertion`]).
        let mut assertions: Vec<Assertion> = Vec::new();

        for c in connections {
            // A `"NAME"` endpoint (module-port/bus bindings lowered by
            // `modules.rs`) is a reference to a *named* net, not a pin:
            // it contributes a name that merges this statement's group
            // with every other group of the same name (see
            // `union_endpoint_sets`), exactly like `as "NAME"`.
            let mut names: Vec<String> = c.net_name.clone().into_iter().collect();
            let mut members: Vec<usize> = Vec::new();
            let mut from_failed = false;
            if let Some(name) = net_endpoint_name(&c.from) {
                names.push(name);
            } else {
                match self.resolve_endpoint(&c.from, components, refdes_index) {
                    Some(f) => {
                        members.push(intern_endpoint(&f, &mut endpoints, &mut endpoint_index));
                    }
                    None => from_failed = true,
                }
            }
            // Targets join the source (or, when the source is a bare net
            // name, the first resolved target) and any `"NAME"` targets
            // contribute their name.
            let chain_root = members.first().copied();
            let targets = self.resolve_endpoint_list(
                &c.tos,
                chain_root,
                &mut names,
                components,
                refdes_index,
                &mut endpoints,
                &mut endpoint_index,
                &mut joins,
            );
            members.extend(targets);
            // Nothing resolved and no name to anchor: the source already
            // reported its own error, and the targets above were still
            // visited so their typos are not masked.
            if from_failed && members.is_empty() && names.is_empty() {
                continue;
            }
            assertions.push(Assertion {
                members,
                names,
                classes: c.netclass.clone().into_iter().collect(),
                voltages: Vec::new(),
                span: c.span,
            });
        }

        for n in net_decls {
            let mut names = vec![n.name.clone()];
            let members = self.resolve_endpoint_list(
                &n.endpoints,
                None,
                &mut names,
                components,
                refdes_index,
                &mut endpoints,
                &mut endpoint_index,
                &mut joins,
            );
            assertions.push(Assertion {
                members,
                names,
                classes: n.netclass.clone().into_iter().collect(),
                voltages: Vec::new(),
                span: n.span,
            });
        }

        for p in power_decls {
            let voltage = match Voltage::try_from(&p.voltage) {
                Ok(v) => Some(v),
                Err(e) => {
                    self.emit_unit_error(&e, "power voltage");
                    None
                }
            };
            let mut names = vec![p.name.clone()];
            let members = self.resolve_endpoint_list(
                &p.endpoints,
                None,
                &mut names,
                components,
                refdes_index,
                &mut endpoints,
                &mut endpoint_index,
                &mut joins,
            );
            assertions.push(Assertion {
                members,
                names,
                classes: p.netclass.clone().into_iter().collect(),
                voltages: voltage.into_iter().collect(),
                span: p.span,
            });
        }

        EndpointSet {
            endpoints,
            joins,
            assertions,
        }
    }

    /// Shared netclass/voltage join resolution for one net: emit
    /// unknown-class (`E-SYNTH-NAME-006`), conflicting-class
    /// (`E-SYNTH-NAME-006`), and conflicting-voltage
    /// (`E-SYNTH-POWER-007`) diagnostics, and return the winning
    /// (netclass, voltage). Used for both populated nets and empty
    /// orphan rails so the two paths cannot drift apart.
    fn resolve_joins(
        &mut self,
        net_name: &str,
        classes: &[(String, Span)],
        voltages: &[(Voltage, Span)],
        known_classes: &HashMap<&str, &NetClass>,
    ) -> (Option<String>, Option<Voltage>) {
        let mut distinct_classes: Vec<(String, Span)> = Vec::new();
        for (c, s) in classes {
            if !distinct_classes.iter().any(|(m, _)| m == c) {
                distinct_classes.push((c.clone(), *s));
            }
        }
        for (class, span) in &distinct_classes {
            if !known_classes.contains_key(class.as_str()) {
                self.diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-NAME-006",
                        Severity::Error,
                        "net joined to unknown netclass",
                    )
                    .location(Location::from_span(self.file.to_string(), *span))
                    .expected("a netclass declared with `netclass \"NAME\" { … }`")
                    .found(format!("netclass `{class}` not declared"))
                    .message(format!(
                        "net `{net_name}` joins netclass `{class}`, which is not declared; declare \
                         it with `netclass \"{class}\" {{ … }}` or fix the spelling"
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-NAME-006")
                    .build(),
                );
            }
        }
        if distinct_classes
            .iter()
            .filter(|(c, _)| known_classes.contains_key(c.as_str()))
            .count()
            > 1
        {
            let first = distinct_classes
                .iter()
                .find(|(c, _)| known_classes.contains_key(c.as_str()))
                .expect("a known class");
            for (other, span) in &distinct_classes {
                if other == &first.0 || !known_classes.contains_key(other.as_str()) {
                    continue;
                }
                self.diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-NAME-006",
                        Severity::Error,
                        "net joined to conflicting netclasses",
                    )
                    .location(Location::from_span(self.file.to_string(), *span))
                    .expected(format!("net `{net_name}` in a single netclass"))
                    .found(format!(
                        "netclasses `{}` and `{other}` both joined",
                        first.0
                    ))
                    .message(format!(
                        "net `{net_name}` joins both netclass `{}` and netclass `{other}`; keep \
                         one `class` clause",
                        first.0
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-NAME-006")
                    .build(),
                );
            }
        }
        let netclass = distinct_classes
            .iter()
            .find(|(c, _)| known_classes.contains_key(c.as_str()))
            .map(|(c, _)| c.clone());

        if voltages.len() > 1 {
            let first = voltages[0].0;
            for (other, span) in voltages.iter().skip(1) {
                self.diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-POWER-007",
                        Severity::Error,
                        "conflicting rail voltages shorted together",
                    )
                    .location(Location::from_span(self.file.to_string(), *span))
                    .expected(format!(
                        "a single nominal voltage on net `{net_name}` ({} V)",
                        first.to_v()
                    ))
                    .found(format!("{} V shorted to {} V", other.to_v(), first.to_v()))
                    .message(format!(
                        "net `{net_name}` declares both {} V and {} V; rails at different \
                         voltages must not be shorted",
                        first.to_v(),
                        other.to_v()
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-POWER-007")
                    .build(),
                );
            }
        }
        let voltage = voltages.first().map(|(v, _)| *v);
        (netclass, voltage)
    }

    /// Resolve a run of endpoints into member indices, appending any
    /// `"NAME"` references to `names`. Each resolved member joins
    /// `chain_root` when given (so a statement's targets all attach to
    /// its source), otherwise the first resolved member becomes the
    /// root and the rest join it. Unresolvable endpoints are skipped
    /// after `resolve_endpoint` has reported them.
    #[allow(clippy::too_many_arguments)]
    fn resolve_endpoint_list(
        &mut self,
        endpoints_ast: &[EndpointAst],
        chain_root: Option<usize>,
        names: &mut Vec<String>,
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
        endpoints: &mut Vec<(ComponentId, PinId, Span)>,
        endpoint_index: &mut HashMap<(ComponentId, PinId), usize>,
        joins: &mut Vec<(usize, usize)>,
    ) -> Vec<usize> {
        let mut members = Vec::new();
        let mut root = chain_root;
        for ep_ast in endpoints_ast {
            if let Some(name) = net_endpoint_name(ep_ast) {
                names.push(name);
                continue;
            }
            let Some(ep) = self.resolve_endpoint(ep_ast, components, refdes_index) else {
                continue;
            };
            let idx = intern_endpoint(&ep, endpoints, endpoint_index);
            if let Some(r) = root {
                joins.push((r, idx));
            }
            root = Some(idx);
            members.push(idx);
        }
        members
    }

    fn resolve_endpoint(
        &mut self,
        ep: &EndpointAst,
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
    ) -> Option<NetEndpoint> {
        // A `"NAME"` endpoint is a reference to a named net, not a pin.
        // It is handled by the caller (it contributes a net *name*), so
        // there is nothing to resolve here.
        if ep.ref_kind == EndpointRefKind::Net {
            return None;
        }
        let Some(&cid) = refdes_index.get(&ep.component) else {
            self.diagnostics.push(
                DiagnosticBuilder::new(
                    "E-SYNTH-COMP-004",
                    Severity::Error,
                    "undefined component refdes",
                )
                .location(Location::from_span(self.file.to_string(), ep.span))
                .expected("a refdes declared earlier in the board")
                .found(format!("`{}` not declared", ep.component))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-COMP-004")
                .build(),
            );
            return None;
        };
        let comp = &components[cid.0 as usize];
        let Some((pid, _pin)) = comp.find_pin(&ep.pin) else {
            // Either the part failed to resolve (so no pins are known)
            // or the pin name simply doesn't exist on the part. Emit a
            // pin diagnostic in both cases; the unresolved-part case is
            // also already covered by E-SYNTH-COMP-001 emitted at
            // component lowering.
            let mut b =
                DiagnosticBuilder::new("E-SYNTH-COMP-002", Severity::Error, "undefined pin")
                    .location(Location::from_span(self.file.to_string(), ep.span))
                    .expected(comp.part.as_ref().map_or_else(
                        || "part to be resolvable before pin lookup".to_string(),
                        |p| format!("a pin defined on part `{}`", p.id),
                    ))
                    .found(format!(
                        "{} — part has no pin named `{}`",
                        comp.describe_pin(&ep.pin),
                        ep.pin,
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-COMP-002");
            if let Some(part) = comp.part.as_ref() {
                for suggestion in suggest_similar_pins(&ep.pin, part) {
                    b = b.suggested_fix(Patch {
                        confidence: 0.5,
                        rationale: Some(format!("did you mean `{suggestion}`?")),
                        patch_consequence_preview: None,
                        kind: PatchKind::ReplaceRange {
                            range: ep.span,
                            replacement: format!("{}.{suggestion}", ep.component),
                        },
                    });
                }
            }
            self.diagnostics.push(b.build());
            return None;
        };
        Some(NetEndpoint {
            component: cid,
            pin: pid,
            source_span: ep.span,
        })
    }

    fn emit_duplicate_refdes(&mut self, refdes: &str, span: synth_diagnostics::Span) {
        self.diagnostics.push(
            DiagnosticBuilder::new(
                "E-SYNTH-NAME-001",
                Severity::Error,
                "duplicate component refdes",
            )
            .location(Location::from_span(self.file.to_string(), span))
            .expected("each refdes used at most once in a board")
            .found(format!("`{refdes}` declared more than once"))
            .explanation_url("synth.docs/diagnostics/E-SYNTH-NAME-001")
            .build(),
        );
    }

    fn emit_unit_error(&mut self, e: &ConversionError, context: &str) {
        let span = e.span();
        let (title, expected, found) = match e {
            ConversionError::InvalidLiteral { literal, .. } => (
                "invalid numeric literal",
                "an integer or decimal literal".to_string(),
                format!("`{literal}` ({context})"),
            ),
            ConversionError::WrongUnit {
                actual,
                expected_quantity,
                ..
            } => (
                "unit is not compatible with expected quantity",
                format!("a {expected_quantity} unit"),
                format!("unit `{}` ({context})", actual.as_str()),
            ),
            ConversionError::Overflow { literal, unit, .. } => (
                "value overflows integer base unit",
                "a value within the representable range".to_string(),
                format!("`{literal}{}` ({context})", unit.as_str()),
            ),
        };
        self.diagnostics.push(
            DiagnosticBuilder::new("E-SYNTH-UNIT-001", Severity::Error, title)
                .location(Location::from_span(self.file.to_string(), span))
                .expected(expected)
                .found(found)
                .explanation_url("synth.docs/diagnostics/E-SYNTH-UNIT-001")
                .build(),
        );
    }
}

/// Distribute per-statement assertions into their union-find
/// groups. Two passes so a bare `power "X" 3.3v` joins the named net
/// no matter whether it is written before or after the statements
/// that populate it: first every assertion with members (building
/// name → root), then the bare ones (joining by name, else becoming
/// orphans for empty-net materialization).
#[allow(clippy::too_many_arguments)]
fn distribute_assertions(
    assertions: &[Assertion],
    member_to_root: &HashMap<usize, usize>,
    group_info: &mut HashMap<usize, GroupInfo>,
    name_to_root: &mut HashMap<String, usize>,
    orphans: &mut Vec<OrphanDecl>,
    orphan_index: &mut HashMap<String, usize>,
) {
    for a in assertions.iter().filter(|a| !a.members.is_empty()) {
        let r = member_to_root[&a.members[0]];
        let info = group_info.get_mut(&r).expect("group exists");
        for name in &a.names {
            if !info.names.iter().any(|(n, _)| n == name) {
                info.names.push((name.clone(), a.span));
            }
            name_to_root.entry(name.clone()).or_insert(r);
        }
        for class in &a.classes {
            if !info.classes.iter().any(|(c, _)| c == class) {
                info.classes.push((class.clone(), a.span));
            }
        }
        for v in &a.voltages {
            if !info.voltages.iter().any(|(w, _)| w == v) {
                info.voltages.push((*v, a.span));
            }
        }
    }
    for a in assertions.iter().filter(|a| a.members.is_empty()) {
        for name in &a.names {
            if let Some(&r) = name_to_root.get(name) {
                let info = group_info.get_mut(&r).expect("group exists");
                for class in &a.classes {
                    if !info.classes.iter().any(|(c, _)| c == class) {
                        info.classes.push((class.clone(), a.span));
                    }
                }
                for v in &a.voltages {
                    if !info.voltages.iter().any(|(w, _)| w == v) {
                        info.voltages.push((*v, a.span));
                    }
                }
            } else if let Some(&oi) = orphan_index.get(name) {
                let orphan = &mut orphans[oi];
                for class in &a.classes {
                    if !orphan.classes.iter().any(|(c, _)| c == class) {
                        orphan.classes.push((class.clone(), a.span));
                    }
                }
                for v in &a.voltages {
                    if !orphan.voltages.iter().any(|(w, _)| w == v) {
                        orphan.voltages.push((*v, a.span));
                    }
                }
            } else {
                orphan_index.insert(name.clone(), orphans.len());
                orphans.push(OrphanDecl {
                    name: name.clone(),
                    classes: a.classes.iter().map(|c| (c.clone(), a.span)).collect(),
                    voltages: a.voltages.iter().map(|v| (*v, a.span)).collect(),
                });
            }
        }
    }
}

/// Union-find over endpoint indices plus merging of members that
/// share an explicit net name even when they share no endpoint (two
/// `net "X"` blocks, or `as "X"` on disjoint connects). Returns the
/// member lists in deterministic order (by minimum endpoint index)
/// so net output is stable.
fn union_endpoint_sets(
    count: usize,
    joins: &[(usize, usize)],
    assertions: &[Assertion],
) -> Vec<Vec<usize>> {
    // Union-find with path compression.
    let mut parent: Vec<usize> = (0..count).collect();
    for (a, b) in joins {
        let ra = find(&mut parent, *a);
        let rb = find(&mut parent, *b);
        if ra != rb {
            parent[ra] = rb;
        }
    }
    // Merge members sharing an explicit name. Stable iteration:
    // assertions are already in statement order and member lists in
    // endpoint order.
    let mut name_root: HashMap<&str, usize> = HashMap::new();
    for a in assertions {
        for name in &a.names {
            for m in &a.members {
                if let Some(&first) = name_root.get(name.as_str()) {
                    let rm = find(&mut parent, *m);
                    let rf = find(&mut parent, first);
                    if rm != rf {
                        parent[rm] = rf;
                    }
                } else {
                    name_root.insert(name.as_str(), *m);
                }
            }
        }
    }

    // Group endpoints by root.
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..count {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    // Deterministic order: sort groups by the minimum endpoint index
    // they contain.
    let mut ordered: Vec<Vec<usize>> = groups.into_values().collect();
    for g in &mut ordered {
        g.sort_unstable();
    }
    ordered.sort_by_key(|g| g[0]);
    ordered
}

/// Intern one resolved endpoint to its stable index, reusing the
/// index of an identical `(component, pin)` seen before.
fn intern_endpoint(
    ep: &NetEndpoint,
    endpoints: &mut Vec<(ComponentId, PinId, Span)>,
    endpoint_index: &mut HashMap<(ComponentId, PinId), usize>,
) -> usize {
    *endpoint_index
        .entry((ep.component, ep.pin))
        .or_insert_with(|| {
            let i = endpoints.len();
            endpoints.push((ep.component, ep.pin, ep.source_span));
            i
        })
}

/// Gather every `variant` declaration in the statement tree, in source
/// order (a variant may sit inside a group or sheet).
fn collect_variant_decls<'a>(
    statements: &'a [StatementAst],
    out: &mut Vec<&'a synth_ast::VariantDeclStmt>,
) {
    for stmt in statements {
        match stmt {
            StatementAst::Variant(v) => out.push(v),
            StatementAst::Group(g) => collect_variant_decls(&g.statements, out),
            StatementAst::Sheet(s) => collect_variant_decls(&s.statements, out),
            _ => {}
        }
    }
}

/// The net name of a `"NAME"` endpoint reference, or `None` for a
/// component-pin endpoint. Module/bus expansion emits these instead of
/// synthesizing per-instance copies of a shared net.
fn net_endpoint_name(ep: &EndpointAst) -> Option<String> {
    (ep.ref_kind == EndpointRefKind::Net).then(|| ep.component.clone())
}

/// A component refdes for lowering-time diagnostics, before the
/// [`Component`] exists: `` `U1` ``, or `` `U1` (group "Power") ``
/// when the declaration sits inside a group.
fn describe_decl(refdes: &str, group: Option<&str>) -> String {
    match group {
        Some(group) => format!("`{refdes}` (group \"{group}\")"),
        None => format!("`{refdes}`"),
    }
}

/// Record the net/notes statements of one flattened board position
/// into their lowering buffers. Pure cloning — resolution happens in
/// [`LowerCtx::collect_assertions`]. `group` is the innermost
/// enclosing group, recorded on `notes` for placement.
#[allow(clippy::too_many_arguments)]
fn push_statement_records(
    stmt: &StatementAst,
    group: Option<&str>,
    sheet: Option<&str>,
    connections: &mut Vec<ConnectionRecord>,
    net_decls: &mut Vec<NetDeclRecord>,
    power_decls: &mut Vec<PowerDeclRecord>,
    diff_pair_stmts: &mut Vec<DiffPairStmt>,
    notes: &mut Vec<Note>,
) {
    match stmt {
        StatementAst::Connection(c) => {
            let mut tos = vec![c.to.clone()];
            tos.extend(c.additional.iter().cloned());
            connections.push(ConnectionRecord {
                from: c.from.clone(),
                tos,
                span: c.span,
                net_name: c.net_name.clone(),
                netclass: c.netclass.clone(),
            });
        }
        StatementAst::Net(n) => {
            net_decls.push(NetDeclRecord {
                name: n.name.clone(),
                netclass: n.netclass.clone(),
                endpoints: n.endpoints.clone(),
                span: n.span,
            });
        }
        StatementAst::Power(p) => {
            power_decls.push(PowerDeclRecord {
                name: p.name.clone(),
                voltage: p.voltage.clone(),
                netclass: p.netclass.clone(),
                endpoints: p.endpoints.clone(),
                span: p.span,
            });
        }
        StatementAst::Notes(n) => {
            notes.push(Note {
                title: n.title.clone(),
                lines: n.lines.clone(),
                group: group.map(str::to_string),
                sheet: sheet.map(str::to_string),
                source_span: n.span,
            });
        }
        StatementAst::DiffPair(d) => diff_pair_stmts.push(d.clone()),
        // Components, metadata, keepouts, netclasses, and blocks are
        // handled by the caller; future statement kinds are ignored
        // here until lowering learns about them.
        _ => {}
    }
}

fn suggest_similar_parts(needle: &str, registry: &Registry) -> Vec<String> {
    let mut out: Vec<(usize, String)> = registry
        .ids()
        .filter_map(|id| {
            let s = id.as_str();
            let d = levenshtein(needle, s);
            (d <= 3).then(|| (d, s.to_string()))
        })
        .collect();
    out.sort_by_key(|(d, _)| *d);
    out.into_iter().take(3).map(|(_, s)| s).collect()
}

fn suggest_similar_pins(needle: &str, part: &Part) -> Vec<String> {
    let mut out: Vec<(usize, String)> = part
        .pins
        .iter()
        .filter_map(|p| {
            let d = levenshtein(needle, &p.name);
            (d <= 3).then(|| (d, p.name.clone()))
        })
        .collect();
    out.sort_by_key(|(d, _)| *d);
    out.into_iter().take(3).map(|(_, s)| s).collect()
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn parse_region(s: &str) -> Option<PlacementRegion> {
    match s.to_lowercase().as_str() {
        "top_left" => Some(PlacementRegion::TopLeft),
        "top_right" => Some(PlacementRegion::TopRight),
        "bottom_left" => Some(PlacementRegion::BottomLeft),
        "bottom_right" => Some(PlacementRegion::BottomRight),
        "centre" | "center" => Some(PlacementRegion::Centre),
        "top_edge" => Some(PlacementRegion::TopEdge),
        "bottom_edge" => Some(PlacementRegion::BottomEdge),
        "left_edge" => Some(PlacementRegion::LeftEdge),
        "right_edge" => Some(PlacementRegion::RightEdge),
        _ => None,
    }
}

fn parse_edge(s: &str) -> Option<PlacementEdge> {
    match s.to_lowercase().as_str() {
        "top" => Some(PlacementEdge::Top),
        "bottom" => Some(PlacementEdge::Bottom),
        "left" => Some(PlacementEdge::Left),
        "right" => Some(PlacementEdge::Right),
        _ => None,
    }
}

fn parse_side(s: &str) -> Option<PlacementSide> {
    match s.to_lowercase().as_str() {
        "above" => Some(PlacementSide::Above),
        "below" => Some(PlacementSide::Below),
        "left" => Some(PlacementSide::Left),
        "right" => Some(PlacementSide::Right),
        _ => None,
    }
}

/// Parse a `#rrggbb` (or bare `rrggbb`) hex colour into RGB. Returns
/// `None` for anything else — the caller reports `E-SYNTH-NAME-011`
/// and keeps the default palette hue.
fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlacementPriority;
    use synth_parser::parse;

    #[test]
    fn test_lower_placement_hint() {
        let src = r#"board "b" {
            component U1: mcu "stm32h743" {
                placement_hint { region: top_left  priority: hard }
            }
        }"#;
        let parse_res = parse(src, "test.synth");
        let ast = parse_res.ast.unwrap();
        let registry = Registry::default();
        let lower_res = lower(&ast, &registry, "test.synth");
        let board = lower_res.board.unwrap();
        let comp = &board.components[0];
        assert!(comp.placement_hint.is_some());
        let hint = comp.placement_hint.as_ref().unwrap();
        assert_eq!(hint.region, Some(PlacementRegion::TopLeft));
        assert_eq!(hint.priority, PlacementPriority::Hard);
    }

    #[test]
    fn test_lower_revision_statement() {
        let src = r#"board "b" {
            revision "B"
            layers 2
        }"#;
        let parse_res = parse(src, "test.synth");
        let ast = parse_res.ast.unwrap();
        let registry = Registry::default();
        let lower_res = lower(&ast, &registry, "test.synth");
        let board = lower_res.board.unwrap();
        assert_eq!(board.revision.as_deref(), Some("B"));
    }

    #[test]
    fn test_lower_netclass_statement() {
        let src = r#"board "b" {
            netclass "PWR" {
                trace_width 0.5mm
                clearance 0.2mm
            }
        }"#;
        let parse_res = parse(src, "test.synth");
        assert!(
            parse_res.diagnostics.is_empty(),
            "{:?}",
            parse_res.diagnostics
        );
        let ast = parse_res.ast.unwrap();
        let registry = Registry::default();
        let lower_res = lower(&ast, &registry, "test.synth");
        assert!(
            lower_res.diagnostics.is_empty(),
            "{:?}",
            lower_res.diagnostics
        );
        let board = lower_res.board.unwrap();
        assert_eq!(board.netclasses.len(), 1);
        let nc = &board.netclasses[0];
        assert_eq!(nc.name, "PWR");
        assert_eq!(nc.trace_width, Some(Length::from_mm(0.5)));
        assert_eq!(nc.clearance, Some(Length::from_mm(0.2)));
        assert_eq!(nc.color, None, "no colour attribute → default palette");
    }

    #[test]
    fn test_lower_netclass_color() {
        let src = r##"board "b" {
            netclass "PWR" {
                color "#c2410c"
            }
        }"##;
        let ast = parse(src, "test.synth").ast.unwrap();
        let res = lower(&ast, &Registry::default(), "test.synth");
        assert!(res.diagnostics.is_empty(), "{:?}", res.diagnostics);
        assert_eq!(
            res.board.unwrap().netclasses[0].color,
            Some([0xc2, 0x41, 0x0c])
        );
    }

    #[test]
    fn test_lower_netclass_bad_color_warns_and_keeps_default() {
        let src = r#"board "b" {
            netclass "PWR" {
                color "not-a-colour"
            }
        }"#;
        let ast = parse(src, "test.synth").ast.unwrap();
        let res = lower(&ast, &Registry::default(), "test.synth");
        assert!(
            res.diagnostics.iter().any(|d| d.code == "E-SYNTH-NAME-011"),
            "{:?}",
            res.diagnostics
        );
        assert_eq!(
            res.board.unwrap().netclasses[0].color,
            None,
            "bad colour keeps the default palette hue"
        );
    }

    #[test]
    fn test_lower_sheet_annotation() {
        let src = r#"board "b" {
            sheet "Power" {
                component C1: capacitor "c_generic_0603"
            }
            component R1: resistor "r_generic_0603"
        }"#;
        let parse_res = parse(src, "test.synth");
        let ast = parse_res.ast.unwrap();
        let registry = Registry::default();
        let lower_res = lower(&ast, &registry, "test.synth");
        let board = lower_res.board.unwrap();
        assert_eq!(board.components.len(), 2);
        assert_eq!(board.components[0].sheet.as_deref(), Some("Power"));
        assert_eq!(board.components[1].sheet, None);
    }

    #[test]
    fn test_lower_sheet_and_group_nest() {
        let src = r#"board "b" {
            sheet "Power" {
                group "LDO input" {
                    component C1: capacitor "c_generic_0603"
                }
            }
        }"#;
        let parse_res = parse(src, "test.synth");
        let ast = parse_res.ast.unwrap();
        let registry = Registry::default();
        let lower_res = lower(&ast, &registry, "test.synth");
        let board = lower_res.board.unwrap();
        let comp = &board.components[0];
        assert_eq!(comp.sheet.as_deref(), Some("Power"));
        assert_eq!(comp.group.as_deref(), Some("LDO input"));
    }

    fn seed_registry() -> Registry {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/parts");
        synth_registry::load_dir(&dir).expect("seed registry must load")
    }

    fn lower_ok(src: &str) -> Board {
        let parse_res = parse(src, "test.synth");
        assert!(
            !parse_res.has_errors(),
            "parse diagnostics: {:?}",
            parse_res.diagnostics
        );
        let ast = parse_res.ast.expect("ast present");
        let registry = seed_registry();
        let lower_res = lower(&ast, &registry, "test.synth");
        assert!(
            !lower_res.has_errors(),
            "lower diagnostics: {:?}",
            lower_res
                .diagnostics
                .iter()
                .map(|d| (&d.code, &d.title))
                .collect::<Vec<_>>()
        );
        lower_res.board.expect("board present")
    }

    fn lower_with_diags(src: &str) -> LowerResult {
        let parse_res = parse(src, "test.synth");
        assert!(
            !parse_res.has_errors(),
            "parse diagnostics: {:?}",
            parse_res.diagnostics
        );
        let ast = parse_res.ast.expect("ast present");
        lower(&ast, &seed_registry(), "test.synth")
    }

    #[test]
    fn test_named_net_block_merges_endpoints() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                net "+3V3" {
                    U1.vout, C3.p1
                }
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        let net = &board.nets[0];
        assert_eq!(net.name, "+3V3");
        assert_eq!(net.endpoints.len(), 2);
        assert_eq!(net.netclass, None);
        assert_eq!(net.voltage, None);
    }

    #[test]
    fn test_connect_as_names_net_with_fanout() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                component C4: capacitor "c_generic_0603"
                connect U1.vout -> C3.p1, C4.p1 as "+3V3"
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        let net = &board.nets[0];
        assert_eq!(net.name, "+3V3");
        assert_eq!(net.endpoints.len(), 3);
    }

    #[test]
    fn test_same_name_disjoint_statements_merge() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                component C4: capacitor "c_generic_0603"
                component C5: capacitor "c_generic_0603"
                net "+3V3" { U1.vout }
                connect C3.p1 -> C4.p1 as "+3V3"
                connect C4.p1 -> C5.p1
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        let net = &board.nets[0];
        assert_eq!(net.name, "+3V3");
        assert_eq!(net.endpoints.len(), 4);
    }

    #[test]
    fn test_bare_power_declares_voltage_and_joins() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                power "+3V3" 3.3v
                connect U1.vout -> C3.p1 as "+3V3"
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        let net = &board.nets[0];
        assert_eq!(net.name, "+3V3");
        assert_eq!(net.endpoints.len(), 2);
        assert_eq!(net.voltage, Some(crate::units::Voltage::from_v(3.3)));
    }

    #[test]
    fn test_power_with_body_and_class() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                netclass "PWR" { trace_width 0.5mm }
                power "+3V3" 3.3v class "PWR" {
                    U1.vout, C3.p1
                }
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        let net = &board.nets[0];
        assert_eq!(net.name, "+3V3");
        assert_eq!(net.netclass.as_deref(), Some("PWR"));
        assert_eq!(net.voltage, Some(crate::units::Voltage::from_v(3.3)));
    }

    #[test]
    fn test_net_joins_netclass() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                netclass "PWR" { trace_width 0.5mm }
                net "+3V3" class "PWR" { U1.vout, C3.p1 }
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        assert_eq!(board.nets[0].netclass.as_deref(), Some("PWR"));
    }

    #[test]
    fn test_unknown_netclass_errors() {
        let res = lower_with_diags(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                connect U1.vout -> C3.p1 as "+3V3" class "NOPE"
            }"#,
        );
        assert!(res.has_errors());
        assert!(
            res.diagnostics.iter().any(|d| d.code == "E-SYNTH-NAME-006"),
            "expected E-SYNTH-NAME-006, got {:?}",
            res.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        let board = res.board.expect("board present");
        assert_eq!(board.nets[0].netclass, None);
    }

    #[test]
    fn test_conflicting_net_names_error() {
        let res = lower_with_diags(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                component C4: capacitor "c_generic_0603"
                connect U1.vout -> C3.p1 as "+3V3"
                connect C3.p1 -> C4.p1 as "+5V"
            }"#,
        );
        assert!(res.has_errors());
        assert!(
            res.diagnostics.iter().any(|d| d.code == "E-SYNTH-NAME-005"),
            "expected E-SYNTH-NAME-005, got {:?}",
            res.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        // First-seen name wins; all three endpoints still merge.
        let board = res.board.expect("board present");
        assert_eq!(board.nets.len(), 1);
        assert_eq!(board.nets[0].name, "+3V3");
        assert_eq!(board.nets[0].endpoints.len(), 3);
    }

    #[test]
    fn test_conflicting_rail_voltages_error() {
        let res = lower_with_diags(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                power "+3V3" 3.3v
                power "+3V3" 5v
                connect U1.vout -> C3.p1 as "+3V3"
            }"#,
        );
        assert!(res.has_errors());
        assert!(
            res.diagnostics
                .iter()
                .any(|d| d.code == "E-SYNTH-POWER-007"),
            "expected E-SYNTH-POWER-007, got {:?}",
            res.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        let board = res.board.expect("board present");
        assert_eq!(
            board.nets[0].voltage,
            Some(crate::units::Voltage::from_v(3.3))
        );
    }

    #[test]
    fn test_diff_pair_resolves_to_named_nets() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                component C4: capacitor "c_generic_0603"
                component C5: capacitor "c_generic_0603"
                net "DP" { U1.vout, C3.p1 }
                net "DN" { U1.vin, C4.p1 }
                connect C3.p2 -> C5.p1
                diff_pair DP DN {
                    impedance 90ohm
                }
            }"#,
        );
        assert_eq!(board.diff_pairs.len(), 1);
        let dp = &board.diff_pairs[0];
        let by_name = |n: &str| {
            board
                .nets
                .iter()
                .find(|x| x.name == n)
                .expect("net exists")
                .id
        };
        assert_eq!(dp.positive_net, Some(by_name("DP")));
        assert_eq!(dp.negative_net, Some(by_name("DN")));
    }

    #[test]
    fn test_legacy_auto_names_unchanged() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component C3: capacitor "c_generic_0603"
                connect U1.vout -> C3.p1
            }"#,
        );
        assert_eq!(board.nets.len(), 1);
        assert_eq!(board.nets[0].name, "net_0");
        assert_eq!(board.nets[0].netclass, None);
        assert_eq!(board.nets[0].voltage, None);
    }

    #[test]
    fn test_legends_flag_lowered_default_off() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
            }"#,
        );
        assert!(!board.legends, "legends default off");
        let board = lower_ok(
            r#"board "b" {
                legends on
                component U1: regulator "ams1117_3v3"
            }"#,
        );
        assert!(board.legends, "legends on lowers to true");
    }

    #[test]
    fn test_dnp_flag_lowered() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                component R7: resistor "r_generic_0603" dnp
                connect U1.vout -> R7.p1
                connect U1.gnd -> R7.p2
            }"#,
        );
        assert_eq!(board.components.len(), 2);
        assert!(!board.components[0].dnp);
        assert!(board.components[1].dnp);
        // DNP parts still join the net graph — ERC checks them.
        assert_eq!(board.nets.len(), 2);
    }

    #[test]
    fn test_dnp_still_checked_by_erc() {
        // A DNP part with an undefined pin still fails lowering like
        // any other part: the flag never silences checks.
        let res = lower_with_diags(
            r#"board "b" {
                component R7: resistor "r_generic_0603" dnp
                component R8: resistor "r_generic_0603"
                connect R7.nope -> R8.p1
            }"#,
        );
        assert!(res.has_errors());
        assert!(
            res.diagnostics.iter().any(|d| d.code == "E-SYNTH-COMP-002"),
            "expected E-SYNTH-COMP-002, got {:?}",
            res.diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_notes_lower_with_group() {
        let board = lower_ok(
            r#"board "b" {
                component U1: regulator "ams1117_3v3"
                group "Power" {
                    component C1: capacitor "c_generic_0603"
                    notes "Power notes" {
                        "Keep bulk caps close."
                    }
                }
                notes "General" {
                    "Assemble at JLCPCB."
                }
            }"#,
        );
        assert_eq!(board.notes.len(), 2);
        assert_eq!(board.notes[0].title, "Power notes");
        assert_eq!(board.notes[0].lines, vec!["Keep bulk caps close."]);
        assert_eq!(board.notes[0].group.as_deref(), Some("Power"));
        assert_eq!(board.notes[1].title, "General");
        assert_eq!(board.notes[1].group, None);
    }
}
