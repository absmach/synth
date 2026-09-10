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
    ComponentDeclAst, DiffPairAttr, DiffPairStmt, EndpointAst, KeepoutAttr, KeepoutStmt,
    ProgramAst, StatementAst,
};
use synth_diagnostics::{
    Diagnostic, DiagnosticBuilder, Location, Patch, PatchKind, Severity, SuggestedAction,
};
use synth_registry::{Part, Registry};

use crate::board::{
    Board, Component, ComponentId, DiffPair, Keepout, Net, NetEndpoint, NetId, PinId,
    PlacementEdge, PlacementRegion, PlacementSide,
};
use crate::units::{ConversionError, Impedance, Length};

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
    let mut components: Vec<Component> = Vec::new();
    let mut refdes_index: HashMap<String, ComponentId> = HashMap::new();
    let mut connections: Vec<(EndpointAst, EndpointAst, synth_diagnostics::Span)> = Vec::new();
    let mut diff_pairs: Vec<DiffPair> = Vec::new();
    let mut keepouts: Vec<Keepout> = Vec::new();
    let mut layers: u32 = 0;
    let mut manufacturer: Option<String> = None;
    let mut revision: Option<String> = None;

    // Groups are flattened here, not represented in the IR as a tree:
    // a group names its components and nothing more (see `GroupStmt`),
    // so lowering walks into one carrying the name down and leaves the
    // board a flat component list exactly as before. `stack` is the
    // enclosing group chain; nested groups take the innermost name.
    let mut stack: Vec<(&[StatementAst], usize, Option<&str>)> =
        vec![(&ast.board.statements, 0, None)];
    while let Some((statements, index, group)) = stack.pop() {
        let Some(stmt) = statements.get(index) else {
            continue;
        };
        stack.push((statements, index + 1, group));
        match stmt {
            StatementAst::Layers(l) => layers = l.count,
            StatementAst::Manufacturer(m) => manufacturer = Some(m.name.clone()),
            StatementAst::Revision(r) => revision = Some(r.rev.clone()),
            StatementAst::Component(c) => {
                let comp = ctx.lower_component(c, registry, components.len(), group);
                if refdes_index.contains_key(&comp.refdes) {
                    ctx.emit_duplicate_refdes(&comp.refdes, comp.source_span);
                } else {
                    refdes_index.insert(comp.refdes.clone(), comp.id);
                }
                components.push(comp);
            }
            StatementAst::Connection(c) => {
                connections.push((c.from.clone(), c.to.clone(), c.span));
            }
            StatementAst::DiffPair(d) => diff_pairs.push(ctx.lower_diff_pair(d)),
            StatementAst::Keepout(k) => keepouts.push(ctx.lower_keepout(k)),
            StatementAst::Group(g) => {
                stack.push((&g.statements, 0, Some(g.name.as_str())));
            }
            // StatementAst is #[non_exhaustive]; future statement
            // kinds reach here until lowering is taught about them.
            _ => {}
        }
    }

    // Build nets by union-find over resolved endpoints. Each successful
    // connect contributes two endpoints that join the same set.
    let nets = ctx.build_nets(&connections, &components, &refdes_index);

    let board = Board {
        name: ast.board.name.clone(),
        layers,
        manufacturer,
        revision,
        components,
        nets,
        diff_pairs,
        keepouts,
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
    ) -> Component {
        let part_id = decl.part.as_deref().unwrap_or("");
        let part = registry.lookup(part_id).cloned();

        if part.is_none() {
            let mut b = DiagnosticBuilder::new("E-SYNTH-COMP-001", Severity::Error, "unknown part")
                .location(Location::from_span(self.file.to_string(), decl.span))
                .expected("a part id present in the registry")
                .found(format!("`{part_id}` not found in registry"))
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
            placement_hint,
            group: group.map(str::to_string),
            source_span: decl.span,
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

    fn build_nets(
        &mut self,
        connections: &[(EndpointAst, EndpointAst, synth_diagnostics::Span)],
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
    ) -> Vec<Net> {
        // Each endpoint is keyed by (ComponentId, PinId). Endpoints
        // that fail resolution are dropped (with a diagnostic) and
        // do not participate in net construction.
        let mut endpoints: Vec<(ComponentId, PinId, synth_diagnostics::Span)> = Vec::new();
        let mut endpoint_index: HashMap<(ComponentId, PinId), usize> = HashMap::new();
        let mut joins: Vec<(usize, usize)> = Vec::new();

        for (from_ast, to_ast, _span) in connections {
            let f = self.resolve_endpoint(from_ast, components, refdes_index);
            let t = self.resolve_endpoint(to_ast, components, refdes_index);
            let (Some(f), Some(t)) = (f, t) else { continue };

            let fi = *endpoint_index
                .entry((f.component, f.pin))
                .or_insert_with(|| {
                    let i = endpoints.len();
                    endpoints.push((f.component, f.pin, f.source_span));
                    i
                });
            let ti = *endpoint_index
                .entry((t.component, t.pin))
                .or_insert_with(|| {
                    let i = endpoints.len();
                    endpoints.push((t.component, t.pin, t.source_span));
                    i
                });
            joins.push((fi, ti));
        }

        // Union-find with path compression.
        let mut parent: Vec<usize> = (0..endpoints.len()).collect();
        for (a, b) in joins {
            let ra = find(&mut parent, a);
            let rb = find(&mut parent, b);
            if ra != rb {
                parent[ra] = rb;
            }
        }

        // Group endpoints by root.
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..endpoints.len() {
            let r = find(&mut parent, i);
            groups.entry(r).or_default().push(i);
        }

        // Materialize nets in deterministic order: sort groups by the
        // minimum endpoint index they contain so output is stable.
        let mut ordered: Vec<Vec<usize>> = groups.into_values().collect();
        for g in &mut ordered {
            g.sort_unstable();
        }
        ordered.sort_by_key(|g| g[0]);

        ordered
            .into_iter()
            .enumerate()
            .map(|(idx, members)| {
                let id = NetId(idx as u32);
                let endpoints = members
                    .into_iter()
                    .map(|i| {
                        let (c, p, span) = endpoints[i];
                        NetEndpoint {
                            component: c,
                            pin: p,
                            source_span: span,
                        }
                    })
                    .collect();
                Net {
                    id,
                    name: format!("net_{idx}"),
                    endpoints,
                }
            })
            .collect()
    }

    fn resolve_endpoint(
        &mut self,
        ep: &EndpointAst,
        components: &[Component],
        refdes_index: &HashMap<String, ComponentId>,
    ) -> Option<NetEndpoint> {
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
                        "`{}.{}` — part has no pin named `{}`",
                        ep.component, ep.pin, ep.pin,
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
}
