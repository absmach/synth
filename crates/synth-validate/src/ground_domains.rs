//! Declared ground domains (absmach/synth#464, docs/ground-domains.md).
//!
//! A design that splits its ground names each domain with a `ground_domain`
//! block: the domain's ground net, the one part that joins it to another
//! ground, and the signals that cross the boundary with how their return
//! current crosses. Two codes:
//!
//! - `E-SYNTH-POWER-016` (error): the declaration does not hold together. A
//!   domain names a net that is missing or is not a ground; its `join` names
//!   a part that is missing or does not bridge it to another ground; two or
//!   more domains are declared and they are not all joined into one ground;
//!   a `crossing` names a missing net or a stitching part that does not sit
//!   between the two grounds.
//! - `E-SYNTH-POWER-017` (warning): a signal runs between parts on two
//!   different domains and no `crossing` declares its stitching path, so
//!   nothing says where its return current crosses the split.
//!
//! Nothing here changes copper: the block states the intent the checks
//! evaluate, and the join and the stitching parts are ordinary components.

use std::collections::{BTreeMap, BTreeSet};

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::{Board, ComponentId, GroundDomain, GroundStitch, NetId};

use crate::{ErcCategory, ErcRule};

const DECLARATION_CODE: &str = "E-SYNTH-POWER-016";
const CROSSING_CODE: &str = "E-SYNTH-POWER-017";

/// `E-SYNTH-POWER-016` and `E-SYNTH-POWER-017` in the ERC registry.
#[derive(Debug)]
pub struct GroundDomainRule;

impl ErcRule for GroundDomainRule {
    fn code(&self) -> &'static str {
        DECLARATION_CODE
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        if board.ground_domains.is_empty() {
            return Vec::new();
        }
        Checker::new(board, file).run()
    }
}

/// Whether a net or pin name reads as a ground: `gnd` or `vss` anywhere, or
/// exactly `0v`, in any case. The same rule the exporter's plane classifier
/// and the return-path check use.
fn is_ground_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("gnd") || lower.contains("vss") || lower == "0v"
}

struct Checker<'a> {
    board: &'a Board,
    file: &'a str,
    ground: BTreeSet<NetId>,
    out: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn new(board: &'a Board, file: &'a str) -> Self {
        let domains = synth_ir::infer_power_domains(board);
        let ground = board
            .nets
            .iter()
            .filter(|net| {
                domains
                    .get(net.id)
                    .is_some_and(synth_ir::PowerDomainKind::is_ground)
                    || is_ground_name(&net.name)
                    || net.endpoints.iter().any(|endpoint| {
                        board
                            .pin(endpoint.component, endpoint.pin)
                            .is_some_and(|pin| is_ground_name(&pin.name))
                    })
            })
            .map(|net| net.id)
            .collect();
        Self {
            board,
            file,
            ground,
            out: Vec::new(),
        }
    }

    fn net_id(&self, name: &str) -> Option<NetId> {
        self.board
            .nets
            .iter()
            .find(|net| net.name == name)
            .map(|net| net.id)
    }

    fn component_id(&self, refdes: &str) -> Option<ComponentId> {
        self.board
            .components
            .iter()
            .find(|component| component.refdes == refdes)
            .map(|component| component.id)
    }

    /// The nets a component's pins touch.
    fn nets_of(&self, component: ComponentId) -> BTreeSet<NetId> {
        self.board
            .nets
            .iter()
            .filter(|net| net.endpoints.iter().any(|e| e.component == component))
            .map(|net| net.id)
            .collect()
    }

    fn net_name(&self, id: NetId) -> String {
        self.board
            .net(id)
            .map_or_else(|| format!("net {}", id.0), |net| net.name.clone())
    }

    fn report(
        &mut self,
        code: &str,
        severity: Severity,
        title: &str,
        found: String,
        span: synth_diagnostics::Span,
        expected: &str,
    ) {
        self.out.push(
            DiagnosticBuilder::new(code, severity, title)
                .location(Location::from_span(self.file.to_string(), span))
                .expected(expected.to_string())
                .found(found)
                .explanation_url(format!("synth.docs/diagnostics/{code}"))
                .build(),
        );
    }

    // One pass in declaration order: resolve the domains, then their joins,
    // then the crossings, each step needing the last one's sets.
    #[allow(clippy::too_many_lines)]
    fn run(mut self) -> Vec<Diagnostic> {
        let board = self.board;
        // Each declared domain, resolved to its ground net.
        let mut domain_nets: Vec<(&GroundDomain, NetId)> = Vec::new();
        for domain in &board.ground_domains {
            match self.net_id(&domain.net) {
                Some(id) if self.ground.contains(&id) => domain_nets.push((domain, id)),
                Some(_) => self.report(
                    DECLARATION_CODE,
                    Severity::Error,
                    "ground_domain names a net that is not a ground",
                    format!("`{}` carries no ground pin and no ground name", domain.net),
                    domain.source_span,
                    "a ground net (a `gnd`/`vss` pin or name)",
                ),
                None => self.report(
                    DECLARATION_CODE,
                    Severity::Error,
                    "ground_domain names a net the design does not declare",
                    format!("no net named `{}`", domain.net),
                    domain.source_span,
                    "an existing ground net",
                ),
            }
        }

        // The grounds each join bridges, as edges for one union-find.
        let mut parent: BTreeMap<NetId, NetId> = BTreeMap::new();
        let mut join_parts: BTreeSet<ComponentId> = BTreeSet::new();
        for (domain, net) in &domain_nets {
            parent.entry(*net).or_insert(*net);
            let Some(refdes) = &domain.join else {
                continue;
            };
            let Some(part) = self.component_id(refdes) else {
                self.report(
                    DECLARATION_CODE,
                    Severity::Error,
                    "ground_domain join names a part the design does not declare",
                    format!("no component `{refdes}` to join `{}`", domain.net),
                    domain.source_span,
                    "a net tie or 0 ohm link between this ground and another",
                );
                continue;
            };
            let touched = self.nets_of(part);
            let others: Vec<NetId> = touched
                .iter()
                .copied()
                .filter(|other| *other != *net && self.ground.contains(other))
                .collect();
            if !touched.contains(net) || others.is_empty() {
                self.report(
                    DECLARATION_CODE,
                    Severity::Error,
                    "ground_domain join does not bridge the domain to another ground",
                    format!(
                        "`{refdes}` touches {}",
                        touched
                            .iter()
                            .map(|id| format!("`{}`", self.net_name(*id)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    domain.source_span,
                    "one pin on the domain's net and one on another ground",
                );
                continue;
            }
            join_parts.insert(part);
            for other in others {
                parent.entry(other).or_insert(other);
                union(&mut parent, *net, other);
            }
        }
        // Two or more declared domains must end up one ground.
        if domain_nets.len() >= 2 {
            let roots: BTreeSet<NetId> = domain_nets
                .iter()
                .map(|(_, net)| find(&mut parent, *net))
                .collect();
            if roots.len() > 1 {
                for (domain, net) in &domain_nets {
                    let root = find(&mut parent, *net);
                    let joined = domain_nets
                        .iter()
                        .filter(|(_, other)| find(&mut parent, *other) == root)
                        .count();
                    if joined < domain_nets.len() {
                        self.report(
                            DECLARATION_CODE,
                            Severity::Error,
                            "ground domain is not joined to the other declared domains",
                            format!(
                                "`{}` reaches {joined} of {} declared domains through its joins",
                                domain.net,
                                domain_nets.len()
                            ),
                            domain.source_span,
                            "every declared domain joined into one ground at a declared point",
                        );
                    }
                }
            }
        }

        // The ground set that defines a domain boundary: each declared
        // domain, plus any ground its join reaches.
        let boundary: BTreeSet<NetId> = parent.keys().copied().collect();
        // Stitching parts declared by a crossing; they bridge two grounds by
        // design, so they do not place a signal on either side.
        let mut declared: BTreeSet<NetId> = BTreeSet::new();
        let mut stitch_parts: BTreeSet<ComponentId> = BTreeSet::new();
        for (domain, _) in &domain_nets {
            for crossing in &domain.crossings {
                let Some(signal) = self.net_id(&crossing.net) else {
                    self.report(
                        DECLARATION_CODE,
                        Severity::Error,
                        "ground_domain crossing names a net the design does not declare",
                        format!("no net named `{}`", crossing.net),
                        crossing.source_span,
                        "an existing signal net",
                    );
                    continue;
                };
                if let GroundStitch::Part(refdes) = &crossing.stitch {
                    let bridges = self.component_id(refdes).is_some_and(|part| {
                        self.nets_of(part)
                            .iter()
                            .filter(|net| boundary.contains(net))
                            .count()
                            >= 2
                    });
                    if !bridges {
                        self.report(
                            DECLARATION_CODE,
                            Severity::Error,
                            "ground_domain crossing stitch is not a part between two grounds",
                            format!("`{refdes}` does not connect two of the domain grounds"),
                            crossing.source_span,
                            "a stitching capacitor with one pin on each ground, or `stitch join`",
                        );
                        continue;
                    }
                    if let Some(part) = self.component_id(refdes) {
                        stitch_parts.insert(part);
                    }
                }
                declared.insert(signal);
            }
        }

        // Which side of the boundary each part sits on: the one domain
        // ground its pins touch. A part on two grounds straddles the split
        // (a mixed-signal converter, a join, a stitch) and places nothing.
        let side_of = |component: ComponentId| -> Option<NetId> {
            if join_parts.contains(&component) || stitch_parts.contains(&component) {
                return None;
            }
            let grounds: Vec<NetId> = self
                .nets_of(component)
                .into_iter()
                .filter(|net| boundary.contains(net))
                .collect();
            match grounds.as_slice() {
                [one] => Some(*one),
                _ => None,
            }
        };
        let rails: BTreeSet<NetId> = {
            let domains = synth_ir::infer_power_domains(board);
            board
                .nets
                .iter()
                .filter(|net| {
                    domains
                        .get(net.id)
                        .is_some_and(synth_ir::PowerDomainKind::is_rail)
                })
                .map(|net| net.id)
                .collect()
        };
        let mut unstitched = Vec::new();
        for net in &board.nets {
            if self.ground.contains(&net.id) || rails.contains(&net.id) {
                continue;
            }
            let sides: BTreeSet<NetId> = net
                .endpoints
                .iter()
                .filter_map(|endpoint| side_of(endpoint.component))
                .collect();
            if sides.len() >= 2 && !declared.contains(&net.id) {
                unstitched.push((net.name.clone(), sides));
            }
        }
        let span = domain_nets.first().map_or_else(
            || synth_diagnostics::Span::new(0, 0),
            |(d, _)| d.source_span,
        );
        for (name, sides) in unstitched {
            let sides = sides
                .iter()
                .map(|id| format!("`{}`", self.net_name(*id)))
                .collect::<Vec<_>>()
                .join(" and ");
            self.report(
                CROSSING_CODE,
                Severity::Warning,
                "signal crosses a ground domain boundary with no declared stitching path",
                format!("`{name}` runs between parts on {sides}"),
                span,
                "`crossing <net> stitch join` or `crossing <net> stitch <capacitor>`",
            );
        }
        self.out
    }
}

fn find(parent: &mut BTreeMap<NetId, NetId>, net: NetId) -> NetId {
    let mut root = net;
    while let Some(next) = parent.get(&root).copied() {
        if next == root {
            break;
        }
        root = next;
    }
    parent.insert(net, root);
    root
}

fn union(parent: &mut BTreeMap<NetId, NetId>, a: NetId, b: NetId) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent.insert(ra, rb);
    }
}
