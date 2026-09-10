// SPDX-License-Identifier: Apache-2.0

//! Electrical-rule checking.
//!
//! Consumes the canonical [`synth_ir::Board`] (post-lowering) and
//! runs every registered [`ErcRule`]. Each rule emits zero or more
//! [`Diagnostic`]s; the engine concatenates them in
//! rule-registration order.
//!
//! Rule contract — all three are mandatory:
//!
//! - **Deterministic.** Identical input IR produces identical
//!   diagnostics in identical order.
//! - **Local.** Rules reason about the IR; they do not read files,
//!   the network, or process state.
//! - **Single-purpose.** One diagnostic code per rule. Composite
//!   rules split into one struct per code.

#![forbid(unsafe_code)]

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, EntityRef, Location, PatchKind, Severity};
use synth_ir::{Board, Component, ComponentId, NetId, PinId};
use synth_registry::{ElectricalType, PinCapability};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErcCategory {
    Connectivity,
    Protocol,
    Power,
    RequiredSupport,
    Naming,
    Geometry,
    Clock,
    Reset,
    Boot,
    Rf,
    Analog,
    Board,
}

pub trait ErcRule {
    fn code(&self) -> &'static str;
    fn category(&self) -> ErcCategory;
    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic>;
}

/// Run every registered rule against `board` and return the
/// concatenated diagnostics. Ordering is part of the agent-facing
/// contract.
pub mod anomaly;
pub use anomaly::{extract_features, BoardFeatureVec, GraphAnomalyDetectorRule};

pub mod value;
pub use value::{parse_capacitance, parse_resistance};

pub mod patch_mlp;
pub use patch_mlp::PatchMlp;

pub mod placement;
pub use placement::validate_placement;

pub fn run_erc(board: &Board, file: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for rule in all_rules() {
        out.extend(rule.check(board, file));
    }

    if let Some(mlp) = PatchMlp::load_default() {
        let codes: Vec<String> = out.iter().map(|d| d.code.clone()).collect();
        let code_refs: Vec<&str> = codes.iter().map(String::as_str).collect();
        for diag in &mut out {
            for patch in &mut diag.suggested_fixes {
                if patch.patch_consequence_preview.is_none() {
                    let kind_str = match &patch.kind {
                        synth_diagnostics::PatchKind::ReplaceRange { .. } => "replace_range",
                        synth_diagnostics::PatchKind::InsertAt { .. } => "insert_at",
                        synth_diagnostics::PatchKind::DeleteRange { .. } => "delete_range",
                        synth_diagnostics::PatchKind::AddStatement { .. } => "add_statement",
                        synth_diagnostics::PatchKind::RemoveStatement { .. } => "remove_statement",
                        synth_diagnostics::PatchKind::SolveSmt { .. } => "solve_smt",
                    };
                    patch.patch_consequence_preview = mlp.predict(&code_refs, kind_str);
                }
            }
        }
    }

    out
}

fn all_rules() -> Vec<Box<dyn ErcRule>> {
    vec![
        Box::new(RequiredPinsConnectedRule),
        Box::new(SingleEndpointNetRule),
        Box::new(NoConnectMismatchRule),
        Box::new(OutputCollisionRule),
        Box::new(NoDriverRule),
        Box::new(OrphanComponentRule),
        Box::new(PowerOutputShortRule),
        Box::new(MissingDecouplingRule),
        Box::new(KnowledgeSupportRule),
        Box::new(PowerInputWithoutSourceRule),
        Box::new(UsbDifferentialCapabilityRule),
        Box::new(I2cPeerCapabilityRule),
        Box::new(I2cPullupMissingRule),
        Box::new(SpiPeerCapabilityRule),
        Box::new(UartPeerCapabilityRule),
        Box::new(DiffPairImpedanceRule),
        Box::new(DiffPairSelfReferenceRule),
        Box::new(RfFeedKeepoutRule),
        Box::new(RfFeedCollisionRule),
        Box::new(ClockSourceCollisionRule),
        Box::new(ClockInputNoSourceRule),
        Box::new(ResetCapabilityFloatingRule),
        Box::new(BootModeFloatingRule),
        Box::new(AnalogDigitalMixingRule),
        Box::new(KeepoutMissingRadiusRule),
        Box::new(KeepoutZeroRadiusRule),
        Box::new(DuplicateRefdesRule),
        Box::new(EmptyRefdesRule),
        Box::new(RefdesFormatRule),
        Box::new(RefdesPrefixRule),
        Box::new(BoardZeroLayersRule),
        Box::new(EmptyBoardRule),
        Box::new(UsbCcPullDownRule),
        Box::new(SpiDirectionRule),
        Box::new(DiffPairBothLegsConnectedRule),
        Box::new(RfFeedImpedanceRule),
        Box::new(ResidualEnergyAnomalyRule),
        Box::new(CrystalLoadCapBalanceRule),
        Box::new(GraphAnomalyDetectorRule::new()),
        Box::new(SchematicInvertedPowerSymbolRule),
        Box::new(SchematicWireCrossingRule),
        Box::new(SchematicDecouplingDistanceRule),
        Box::new(SchematicLongWireRule),
        Box::new(PowerDomainMismatchRule),
        Box::new(SupplyChainRule),
        Box::new(UnverifiedPartRule),
    ]
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-001 — required pin must be connected
// -----------------------------------------------------------------------------

struct RequiredPinsConnectedRule;

impl ErcRule for RequiredPinsConnectedRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if !pin.required {
                    continue;
                }
                let pid = PinId(pin_idx as u32);
                let connected = board.nets_containing(component.id, pid).next().is_some();
                if !connected {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Error,
                            "required pin not connected",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(format!(
                            "pin `{}.{}` is marked required by part `{}` and must be \
                             connected",
                            component.refdes, pin.name, part.id,
                        ))
                        .found(format!("`{}.{}` is floating", component.refdes, pin.name))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.7,
                            rationale: Some(format!(
                                "wire required pin `{}.{}` to connector",
                                component.refdes, pin.name
                            )),
                            patch_consequence_preview: None,
                            kind: synth_diagnostics::PatchKind::InsertAt {
                                at: board.source_span.byte_end.saturating_sub(1),
                                text: format!(
                                    "  connect {}.{} -> J1.p1\n",
                                    component.refdes, pin.name
                                ),
                            },
                        })
                        .build(),
                    );
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-USB-001 — USB DP/DN must connect only to peers carrying the
// matching capability
// -----------------------------------------------------------------------------

struct UsbDifferentialCapabilityRule;

impl ErcRule for UsbDifferentialCapabilityRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-USB-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            check_capability_consistency(
                board,
                net.id,
                &[PinCapability::UsbDp, PinCapability::UsbDn],
                &[],
                "USB differential capability mismatch",
                "E-SYNTH-USB-001",
                file,
                &mut out,
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-I2C-001 — every endpoint on an I²C net must be I²C-capable
// -----------------------------------------------------------------------------

struct I2cPeerCapabilityRule;

impl ErcRule for I2cPeerCapabilityRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-I2C-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            check_capability_consistency(
                board,
                net.id,
                &[PinCapability::I2cSda, PinCapability::I2cScl],
                &[
                    PinCapability::Gpio,
                    PinCapability::SpiMosi,
                    PinCapability::SpiMiso,
                    PinCapability::SpiSck,
                    PinCapability::SpiCs,
                    PinCapability::UartTx,
                    PinCapability::UartRx,
                ],
                "I²C endpoint connected to non-I²C pin",
                "E-SYNTH-I2C-001",
                file,
                &mut out,
            );
        }
        out
    }
}

/// Shared helper for protocol rules: if a net carries at least one
/// *dedicated* endpoint for this protocol, every other endpoint must
/// also declare one of the protocol's capabilities.
///
/// "Dedicated" means the endpoint has a capability in `capabilities`
/// AND no capability in `competing` (the other mutually-exclusive
/// protocol families). Muxable pins on an MCU — e.g. an RP2350 GPIO
/// listing `spi_mosi`, `uart_tx`, `i2c_sda` all at once — are not
/// dedicated and therefore do not on their own demand a protocol.
#[allow(clippy::too_many_arguments)]
fn check_capability_consistency(
    board: &Board,
    net_id: NetId,
    capabilities: &[PinCapability],
    competing: &[PinCapability],
    title: &str,
    code: &str,
    file: &str,
    out: &mut Vec<Diagnostic>,
) {
    let Some(net) = board.net(net_id) else { return };
    let has_dedicated = net.endpoints.iter().any(|e| {
        endpoint_has_any_capability(board, e.component, e.pin, capabilities)
            && !endpoint_has_any_capability(board, e.component, e.pin, competing)
    });
    if !has_dedicated {
        return;
    }
    for endpoint in &net.endpoints {
        if endpoint_has_any_capability(board, endpoint.component, endpoint.pin, capabilities) {
            continue;
        }
        // Passive components (pullup resistors, ESD diodes, decoupling
        // caps) routinely sit on protocol nets without speaking the
        // protocol. Skip them so they don't trigger a false positive.
        if let Some(pin) = board.pin(endpoint.component, endpoint.pin) {
            if matches!(pin.electrical_type, synth_registry::ElectricalType::Passive) {
                continue;
            }
        }
        let component = board
            .component(endpoint.component)
            .expect("component id from board");
        let pin = board
            .pin(endpoint.component, endpoint.pin)
            .expect("pin id from board");
        let needed: Vec<&'static str> = capabilities.iter().copied().map(cap_name).collect();
        out.push(
            DiagnosticBuilder::new(code, Severity::Error, title)
                .location(Location::from_span(file.to_string(), endpoint.source_span))
                .expected(format!(
                    "every endpoint on net `{}` to carry one of [{}]",
                    net.name,
                    needed.join(", "),
                ))
                .found(format!(
                    "`{}.{}` on net `{}` carries none of those capabilities",
                    component.refdes, pin.name, net.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{code}"))
                .build(),
        );
    }
}

fn endpoint_has_any_capability(
    board: &Board,
    c: ComponentId,
    p: PinId,
    caps: &[PinCapability],
) -> bool {
    let Some(pin) = board.pin(c, p) else {
        return false;
    };
    caps.iter().any(|cap| pin.capabilities.contains(cap))
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-002 — net with only one endpoint
// -----------------------------------------------------------------------------

struct SingleEndpointNetRule;

impl ErcRule for SingleEndpointNetRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            if net.endpoints.len() == 1 {
                let endpoint = &net.endpoints[0];
                let Some(component) = board.component(endpoint.component) else {
                    continue;
                };
                let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                    continue;
                };
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "net has only one endpoint",
                    )
                    .location(Location::from_span(file.to_string(), endpoint.source_span))
                    .expected("at least two endpoints (a wire must connect something to something)")
                    .found(format!(
                        "net `{}` has only `{}.{}`",
                        net.name, component.refdes, pin.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.7,
                        rationale: Some("wire single-endpoint net".into()),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::InsertAt {
                            at: endpoint.source_span.byte_end,
                            text: " -> R2.p2".into(),
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-003 — no_connect pin participates in a wire
// -----------------------------------------------------------------------------

struct NoConnectMismatchRule;

impl ErcRule for NoConnectMismatchRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-003"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            for endpoint in &net.endpoints {
                let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                    continue;
                };
                if !matches!(pin.electrical_type, ElectricalType::DoNotConnect) {
                    continue;
                }
                let Some(component) = board.component(endpoint.component) else {
                    continue;
                };
                out.push(
                    DiagnosticBuilder::new(self.code(), Severity::Error, "no-connect pin is wired")
                        .location(Location::from_span(file.to_string(), endpoint.source_span))
                        .expected("no-connect pins must not be connected to any net")
                        .found(format!(
                            "`{}.{}` is declared no_connect on part `{}` but is on net `{}`",
                            component.refdes,
                            pin.name,
                            component.part.as_ref().map_or("?", |p| p.id.as_str()),
                            net.name,
                        ))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-001 — required decoupling caps not present on power net
// -----------------------------------------------------------------------------

struct MissingDecouplingRule;

/// Next free refdes for `prefix` (e.g. `"C"` → `"C7"`), scanning the
/// board's existing refdeses so an auto-inserted part never collides.
fn next_free_refdes(board: &Board, prefix: &str) -> String {
    let mut max: i64 = 0;
    for c in &board.components {
        if let Some(num) = c
            .refdes
            .strip_prefix(prefix)
            .and_then(|s| s.parse::<i64>().ok())
        {
            max = max.max(num);
        }
    }
    format!("{prefix}{}", max + 1)
}

/// Find the ground net connected to `component` — a `PowerInput` pin
/// named gnd/vss/... Returns the net's name, or `None` if the component
/// has no connected ground pin (then the cap cannot be completed).
fn ground_net_for(board: &Board, component: &Component) -> Option<String> {
    let part = component.part.as_ref()?;
    for (idx, pin) in part.pins.iter().enumerate() {
        if pin.electrical_type != ElectricalType::PowerInput {
            continue;
        }
        let n = pin.name.to_lowercase();
        if matches!(
            n.as_str(),
            "gnd" | "vss" | "vssa" | "vee" | "agnd" | "dgnd" | "vneg"
        ) {
            let pid = PinId(idx as u32);
            if let Some((_, net)) = board.nets_containing(component.id, pid).next() {
                return Some(net.name.clone());
            }
        }
    }
    None
}

/// Build a textual patch that inserts `count` decoupling capacitors and
/// their two connects each, right after `component`'s declaration, so
/// the source becomes fixable with a single byte-range patch.
fn decoupling_cap_patch(
    board: &Board,
    component: &Component,
    net: &str,
    gnd_net: &str,
    count: usize,
) -> PatchKind {
    use std::fmt::Write as _;
    let mut text = String::new();
    for _ in 0..count {
        let cap = next_free_refdes(board, "C");
        let _ = writeln!(
            text,
            "\n  component {cap}: capacitor \"c_generic_0603\" // auto-inserted decoupling"
        );
        let _ = writeln!(text, "  connect {}.{net} -> {cap}.p1", component.refdes);
        let _ = writeln!(text, "  connect {}.{gnd_net} -> {cap}.p2", component.refdes);
    }
    PatchKind::InsertAt {
        at: component.source_span.byte_end,
        text,
    }
}

impl ErcRule for MissingDecouplingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::RequiredSupport
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for decoupling in &part.required_decoupling {
                // Locate the pin on THIS part matching the
                // decoupling.net key, then find its global net.
                let Some(pin_idx) = part.pins.iter().position(|p| p.name == decoupling.net) else {
                    continue;
                };
                let pid = PinId(pin_idx as u32);
                let Some((_, net)) = board.nets_containing(component.id, pid).next() else {
                    // Pin floating — E-SYNTH-CONNECT-001 will already
                    // have fired. Don't double-flag.
                    continue;
                };
                let cap_count = net
                    .endpoints
                    .iter()
                    .filter(|e| {
                        board
                            .component(e.component)
                            .and_then(|c| c.part.as_ref())
                            .is_some_and(|p| p.kind == "capacitor")
                    })
                    .count();
                let required = decoupling.count as usize;
                if cap_count < required {
                    let mut builder = DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "missing decoupling capacitors",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected(format!(
                        "{required} capacitor(s) on the net carrying `{}.{}`",
                        component.refdes, decoupling.net,
                    ))
                    .found(format!(
                        "{cap_count} capacitor(s) found on net `{}`",
                        net.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .smt_constraint(format!("(assert (>= decoupling_count {required}))"));

                    // Auto-insert the missing cap(s) into the source
                    // when a ground net is available to complete them.
                    if let Some(gnd_net) = ground_net_for(board, component) {
                        let shortfall = required - cap_count;
                        builder = builder.suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.9,
                            rationale: Some(format!(
                                "auto-insert {shortfall} 100nF decoupling cap(s) on `{}.{}`",
                                component.refdes, decoupling.net,
                            )),
                            patch_consequence_preview: None,
                            kind: decoupling_cap_patch(
                                board,
                                component,
                                &decoupling.net,
                                &gnd_net,
                                shortfall,
                            ),
                        });
                    }
                    out.push(builder.build());
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-KG-001 — support circuit missing per the knowledge graph
// -----------------------------------------------------------------------------

/// Runs the circuit-design knowledge graph
/// (`synth-knowledge`, seeded in `knowledge/circuits.toml`) against
/// the board. Each template encodes a production support circuit —
/// switch debounce, switch pull-ups, LED current limiting, IC
/// decoupling, relay flyback — with the *rationale* carried in the
/// template so the diagnostic explains why production hardware needs
/// it. Templates with a non-empty `enforced_by` are catalog-only and
/// skipped (their existing ERC rules own them, e.g. `E-SYNTH-I2C-001`
/// for I²C pull-ups), so nothing is double-flagged.
struct KnowledgeSupportRule;

impl ErcRule for KnowledgeSupportRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-KG-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::RequiredSupport
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let kg = synth_knowledge::KnowledgeGraph::embedded();
        let mut out = Vec::new();
        for violation in synth_knowledge::check_board(board, &kg) {
            let template = kg
                .templates()
                .iter()
                .find(|t| t.id == violation.template_id);
            let mut builder = DiagnosticBuilder::new(
                self.code(),
                violation.severity,
                format!(
                    "missing support circuit `{}`{}",
                    violation.template_id,
                    template.map_or(String::new(), |t| format!(": {}", t.description)),
                ),
            )
            .location(Location::from_span(file.to_string(), violation.span))
            .expected(template.map_or_else(
                || template_description(&violation.template_id),
                |t| t.rationale.clone(),
            ))
            .found(violation.detail)
            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()));
            if let Some(fix) = violation.suggested_fix {
                builder = builder.suggested_fix(fix);
            }
            out.push(builder.build());
        }
        out
    }
}

/// Fallback expected-text when a violation references an unknown
/// template (cannot happen with the embedded graph, but keeps the
/// conversion total).
fn template_description(id: &str) -> String {
    format!("production support circuit `{id}` present")
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-002 — two power_output pins shorted on the same net
// -----------------------------------------------------------------------------

struct PowerOutputShortRule;

impl ErcRule for PowerOutputShortRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let outputs: Vec<_> = net
                .endpoints
                .iter()
                .filter_map(|e| {
                    let pin = board.pin(e.component, e.pin)?;
                    if matches!(pin.electrical_type, ElectricalType::PowerOutput) {
                        Some((e.component, e.pin, e.source_span))
                    } else {
                        None
                    }
                })
                .collect();
            if outputs.len() < 2 {
                continue;
            }
            // Multiple power outputs on one net = short. Anchor the
            // diagnostic to the second one (first is the "owner";
            // second is the offender).
            let (c1, p1, _) = outputs[0];
            let (c2, p2, span2) = outputs[1];
            let comp1 = board.component(c1).expect("c1");
            let pin1 = board.pin(c1, p1).expect("p1");
            let comp2 = board.component(c2).expect("c2");
            let pin2 = board.pin(c2, p2).expect("p2");
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "power outputs shorted on the same net",
                )
                .location(Location::from_span(file.to_string(), span2))
                .expected("at most one power_output pin per net")
                .found(format!(
                    "net `{}` carries `{}.{}` and `{}.{}`, both power outputs",
                    net.name, comp1.refdes, pin1.name, comp2.refdes, pin2.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-I2C-002 — I²C bus has no pullup resistors
// -----------------------------------------------------------------------------

struct I2cPullupMissingRule;

impl ErcRule for I2cPullupMissingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-I2C-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let competing = [
            PinCapability::Gpio,
            PinCapability::SpiMosi,
            PinCapability::SpiMiso,
            PinCapability::SpiSck,
            PinCapability::SpiCs,
            PinCapability::UartTx,
            PinCapability::UartRx,
        ];
        let mut out = Vec::new();
        for net in &board.nets {
            // Only fire if a dedicated I²C peripheral pin is present:
            // a pin with i2c_sda/i2c_scl and no other mux options.
            let is_i2c = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(
                    board,
                    e.component,
                    e.pin,
                    &[PinCapability::I2cSda, PinCapability::I2cScl],
                ) && !endpoint_has_any_capability(board, e.component, e.pin, &competing)
            });
            if !is_i2c {
                continue;
            }
            // Count resistors on the net (pullups would be resistors
            // connecting SDA/SCL to a power rail).
            let resistor_count = net
                .endpoints
                .iter()
                .filter(|e| {
                    board
                        .component(e.component)
                        .and_then(|c| c.part.as_ref())
                        .is_some_and(|p| p.kind == "resistor")
                })
                .count();
            if resistor_count == 0 {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "I²C net has no pullup resistor",
                    )
                    .location(Location::from_span(
                        file.to_string(),
                        net.endpoints[0].source_span,
                    ))
                    .expected(format!(
                        "at least one resistor on net `{}` (SDA/SCL require external pullups)",
                        net.name,
                    ))
                    .found("no resistor component endpoints on this net".to_string())
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-SPI-001 — SPI capability consistency across endpoints
// -----------------------------------------------------------------------------

struct SpiPeerCapabilityRule;

impl ErcRule for SpiPeerCapabilityRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-SPI-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            check_capability_consistency(
                board,
                net.id,
                &[
                    PinCapability::SpiMosi,
                    PinCapability::SpiMiso,
                    PinCapability::SpiSck,
                    PinCapability::SpiCs,
                ],
                &[
                    PinCapability::Gpio,
                    PinCapability::I2cSda,
                    PinCapability::I2cScl,
                    PinCapability::UartTx,
                    PinCapability::UartRx,
                ],
                "SPI endpoint connected to non-SPI pin",
                self.code(),
                file,
                &mut out,
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-UART-001 — UART capability consistency across endpoints
// -----------------------------------------------------------------------------

struct UartPeerCapabilityRule;

impl ErcRule for UartPeerCapabilityRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-UART-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            check_capability_consistency(
                board,
                net.id,
                &[PinCapability::UartTx, PinCapability::UartRx],
                &[
                    PinCapability::Gpio,
                    PinCapability::I2cSda,
                    PinCapability::I2cScl,
                    PinCapability::SpiMosi,
                    PinCapability::SpiMiso,
                    PinCapability::SpiSck,
                    PinCapability::SpiCs,
                ],
                "UART endpoint connected to non-UART pin",
                self.code(),
                file,
                &mut out,
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-DIFF-001 — diff_pair declared without impedance constraint
// -----------------------------------------------------------------------------

struct DiffPairImpedanceRule;

impl ErcRule for DiffPairImpedanceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-DIFF-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for dp in &board.diff_pairs {
            if dp.impedance.is_none() {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "differential pair missing impedance constraint",
                    )
                    .location(Location::from_span(file.to_string(), dp.source_span))
                    .expected(format!(
                        "`diff_pair {} {}` to specify an impedance constraint",
                        dp.positive, dp.negative,
                    ))
                    .found("no impedance attribute set".to_string())
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .smt_constraint("(assert (= impedance 90))")
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.90,
                        rationale: Some("add default 90ohm impedance constraint".into()),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::ReplaceRange {
                            range: dp.source_span,
                            replacement: format!(
                                "diff_pair {} {} {{\n    impedance 90ohm\n  }}",
                                dp.positive, dp.negative
                            ),
                        },
                    })
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.85,
                        rationale: Some(
                            "SMT quantitative 90ohm diff pair impedance constraint".into(),
                        ),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::SolveSmt {
                            constraint: "(assert (= impedance 90))".into(),
                            target_range: Some(dp.source_span),
                            replacement_template: Some(format!(
                                "diff_pair {} {} {{\n    impedance {{}}ohm\n  }}",
                                dp.positive, dp.negative
                            )),
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-RF-001 — rf_feed pin present but no keepout declared
// -----------------------------------------------------------------------------

struct RfFeedKeepoutRule;

impl ErcRule for RfFeedKeepoutRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-RF-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        // Find the first rf_feed pin in the design, if any.
        let rf_endpoint = board.components.iter().find_map(|c| {
            let part = c.part.as_ref()?;
            let pin_idx = part
                .pins
                .iter()
                .position(|p| p.capabilities.contains(&PinCapability::RfFeed))?;
            Some((c, &part.pins[pin_idx]))
        });
        let Some((component, pin)) = rf_endpoint else {
            return out;
        };
        if board.keepouts.is_empty() {
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "RF feed without a keepout declaration",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .expected("at least one `keepout` declaration in the board")
                .found(format!(
                    "pin `{}.{}` is declared rf_feed but no keepouts exist",
                    component.refdes, pin.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .suggested_fix(synth_diagnostics::Patch {
                    confidence: 0.8,
                    rationale: Some("add default antenna keepout declaration".into()),
                    patch_consequence_preview: None,
                    kind: synth_diagnostics::PatchKind::InsertAt {
                        at: board.source_span.byte_end.saturating_sub(1),
                        text: "  keepout antenna {\n    radius 10mm\n  }\n".into(),
                    },
                })
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-004 — two `output` pins driving the same net
// -----------------------------------------------------------------------------

struct OutputCollisionRule;

impl ErcRule for OutputCollisionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-004"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let outputs: Vec<_> = net
                .endpoints
                .iter()
                .filter(|e| {
                    board
                        .pin(e.component, e.pin)
                        .is_some_and(|p| matches!(p.electrical_type, ElectricalType::Output))
                })
                .collect();
            if outputs.len() < 2 {
                continue;
            }
            let second = outputs[1];
            let comp = board.component(second.component).expect("c");
            let pin = board.pin(second.component, second.pin).expect("p");
            let rx_name = if pin.name.ends_with('d') { "rxd" } else { "rx" };
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "two outputs driving the same net",
                )
                .location(Location::from_span(file.to_string(), second.source_span))
                .expected(format!(
                    "net `{}` to be driven by at most one digital output",
                    net.name,
                ))
                .found(format!(
                    "`{}.{}` is the {}th output pin on net `{}`",
                    comp.refdes,
                    pin.name,
                    outputs.len(),
                    net.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .suggested_fix(synth_diagnostics::Patch {
                    confidence: 0.8,
                    rationale: Some("change receiving pin to RX".into()),
                    patch_consequence_preview: None,
                    kind: synth_diagnostics::PatchKind::ReplaceRange {
                        range: second.source_span,
                        replacement: format!("{}.{}", comp.refdes, rx_name),
                    },
                })
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-005 — net contains only input pins (no driver)
// -----------------------------------------------------------------------------

struct NoDriverRule;

impl ErcRule for NoDriverRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-005"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            if net.endpoints.len() < 2 {
                continue;
            }
            let all_inputs = net.endpoints.iter().all(|e| {
                board
                    .pin(e.component, e.pin)
                    .is_some_and(|p| matches!(p.electrical_type, ElectricalType::Input))
            });
            if all_inputs {
                let first = &net.endpoints[0];
                let comp = board.component(first.component).expect("c");
                let pin = board.pin(first.component, first.pin).expect("p");
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "net contains only input pins (no driver)",
                    )
                    .location(Location::from_span(file.to_string(), first.source_span))
                    .expected(format!(
                        "at least one driver or passive endpoint on net `{}`",
                        net.name,
                    ))
                    .found(format!(
                        "all {} endpoints on net `{}` are input-only (e.g. `{}.{}`)",
                        net.endpoints.len(),
                        net.name,
                        comp.refdes,
                        pin.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.8,
                        rationale: Some("change driving pin to TX".into()),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::ReplaceRange {
                            range: first.source_span,
                            replacement: {
                                let tx_name = if pin.name.ends_with('d') { "txd" } else { "tx" };
                                format!("{}.{}", comp.refdes, tx_name)
                            },
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-006 — orphan component (zero connections)
// -----------------------------------------------------------------------------

struct OrphanComponentRule;

impl ErcRule for OrphanComponentRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-006"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let has_any_connection = board.nets.iter().any(|net| {
                net.endpoints
                    .iter()
                    .any(|endpoint| endpoint.component == component.id)
            });
            if !has_any_connection {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "component has no connections",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected(format!(
                        "component `{}` to participate in at least one `connect` statement",
                        component.refdes,
                    ))
                    .found("zero pins on this component are wired".to_string())
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-003 — power net has no power_output or power source
// -----------------------------------------------------------------------------

struct PowerInputWithoutSourceRule;

impl ErcRule for PowerInputWithoutSourceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-003"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let has_power_input = net.endpoints.iter().any(|e| {
                board
                    .pin(e.component, e.pin)
                    .is_some_and(|p| matches!(p.electrical_type, ElectricalType::PowerInput))
            });
            if !has_power_input {
                continue;
            }
            let has_source = net.endpoints.iter().any(|e| {
                board.pin(e.component, e.pin).is_some_and(|p| {
                    matches!(
                        p.electrical_type,
                        ElectricalType::PowerOutput | ElectricalType::Passive
                    )
                })
            });
            if !has_source {
                let first = &net.endpoints[0];
                let comp = board.component(first.component).expect("c");
                let pin = board.pin(first.component, first.pin).expect("p");
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "power net has no power output source",
                    )
                    .location(Location::from_span(file.to_string(), first.source_span))
                    .expected(format!(
                        "at least one power_output or passive source on net `{}`",
                        net.name,
                    ))
                    .found(format!(
                        "net `{}` carries power_input pin `{}.{}` but no power source",
                        net.name, comp.refdes, pin.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.7,
                        rationale: Some(format!("wire power net `{}` to connector", net.name)),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::InsertAt {
                            at: board.source_span.byte_end.saturating_sub(1),
                            text: format!("  connect {}.{} -> J1.p1\n", comp.refdes, pin.name),
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-DIFF-002 — diff_pair declared with positive == negative
// -----------------------------------------------------------------------------

struct DiffPairSelfReferenceRule;

impl ErcRule for DiffPairSelfReferenceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-DIFF-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for dp in &board.diff_pairs {
            if dp.positive == dp.negative {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "differential pair refers to the same net twice",
                    )
                    .location(Location::from_span(file.to_string(), dp.source_span))
                    .expected("two distinct net names for the positive and negative members")
                    .found(format!(
                        "`diff_pair {} {}` uses one net for both legs",
                        dp.positive, dp.negative,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.8,
                        rationale: Some("delete self-referencing diff_pair declaration".into()),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::DeleteRange {
                            range: dp.source_span,
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-RF-002 — multiple rf_feed pins on the same net
// -----------------------------------------------------------------------------

struct RfFeedCollisionRule;

impl ErcRule for RfFeedCollisionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-RF-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let feeds: Vec<_> = net
                .endpoints
                .iter()
                .filter(|e| {
                    endpoint_has_any_capability(board, e.component, e.pin, &[PinCapability::RfFeed])
                })
                .collect();
            if feeds.len() < 2 {
                continue;
            }
            let second = feeds[1];
            let comp = board.component(second.component).expect("c");
            let pin = board.pin(second.component, second.pin).expect("p");
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "multiple RF feeds on the same net",
                )
                .location(Location::from_span(file.to_string(), second.source_span))
                .expected(format!(
                    "at most one rf_feed endpoint on net `{}`",
                    net.name,
                ))
                .found(format!(
                    "`{}.{}` is the {}th rf_feed pin on this net",
                    comp.refdes,
                    pin.name,
                    feeds.len(),
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CLOCK-001 — multiple clock_output pins on the same net
// -----------------------------------------------------------------------------

struct ClockSourceCollisionRule;

impl ErcRule for ClockSourceCollisionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CLOCK-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Clock
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let outputs: Vec<_> = net
                .endpoints
                .iter()
                .filter(|e| {
                    endpoint_has_any_capability(
                        board,
                        e.component,
                        e.pin,
                        &[PinCapability::ClockOutput],
                    )
                })
                .collect();
            if outputs.len() < 2 {
                continue;
            }
            let second = outputs[1];
            let comp = board.component(second.component).expect("c");
            let pin = board.pin(second.component, second.pin).expect("p");
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "multiple clock sources on the same net",
                )
                .location(Location::from_span(file.to_string(), second.source_span))
                .expected(format!(
                    "at most one clock_output endpoint on net `{}`",
                    net.name,
                ))
                .found(format!(
                    "`{}.{}` is the {}th clock_output pin on net `{}`",
                    comp.refdes,
                    pin.name,
                    outputs.len(),
                    net.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CLOCK-002 — clock_input pin has no clock_output or oscillator
// -----------------------------------------------------------------------------

struct ClockInputNoSourceRule;

impl ErcRule for ClockInputNoSourceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CLOCK-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Clock
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let has_input = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(board, e.component, e.pin, &[PinCapability::ClockInput])
            });
            if !has_input {
                continue;
            }
            let has_output = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(
                    board,
                    e.component,
                    e.pin,
                    &[PinCapability::ClockOutput],
                ) || board
                    .component(e.component)
                    .and_then(|c| c.part.as_ref())
                    .is_some_and(|p| p.kind == "crystal" || p.kind == "oscillator")
            });
            if !has_output {
                let first = &net.endpoints[0];
                let comp = board.component(first.component).expect("c");
                let pin = board.pin(first.component, first.pin).expect("p");
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "clock input has no clock source",
                    )
                    .location(Location::from_span(file.to_string(), first.source_span))
                    .expected(format!(
                        "a clock_output pin, crystal, or oscillator on net `{}`",
                        net.name,
                    ))
                    .found(format!(
                        "net `{}` carries clock_input `{}.{}` but no clock source",
                        net.name, comp.refdes, pin.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-RESET-001 — reset capability pin is floating
// -----------------------------------------------------------------------------

struct ResetCapabilityFloatingRule;

impl ErcRule for ResetCapabilityFloatingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-RESET-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Reset
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for (idx, pin) in part.pins.iter().enumerate() {
                if !pin.capabilities.contains(&PinCapability::Reset) {
                    continue;
                }
                if pin.required {
                    continue; // CONNECT-001 handles this.
                }
                let pid = PinId(idx as u32);
                let connected = board.nets_containing(component.id, pid).next().is_some();
                if !connected {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Error,
                            "floating reset pin",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(format!(
                            "pin `{}.{}` (reset capability) to be connected to a pull-up or reset button",
                            component.refdes, pin.name,
                        ))
                        .found(format!("`{}.{}` is floating", component.refdes, pin.name))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.7,
                            rationale: Some(format!("wire reset pin `{}.{}` to connector", component.refdes, pin.name)),
                            patch_consequence_preview: None,
                            kind: synth_diagnostics::PatchKind::InsertAt {
                                at: board.source_span.byte_end.saturating_sub(1),
                                text: format!("  connect {}.{} -> J1.p1\n", component.refdes, pin.name),
                            },
                        })
                        .build(),
                    );
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-BOOT-001 — boot_mode capability pin is floating
// -----------------------------------------------------------------------------

struct BootModeFloatingRule;

impl ErcRule for BootModeFloatingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-BOOT-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Boot
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for (idx, pin) in part.pins.iter().enumerate() {
                if !pin.capabilities.contains(&PinCapability::BootMode) {
                    continue;
                }
                if pin.required {
                    continue;
                }
                let pid = PinId(idx as u32);
                let connected = board.nets_containing(component.id, pid).next().is_some();
                if !connected {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Warning,
                            "boot mode pin floating",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(format!(
                            "pin `{}.{}` (boot_mode capability) to be strapped high or low",
                            component.refdes, pin.name,
                        ))
                        .found(format!("`{}.{}` is floating", component.refdes, pin.name))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .build(),
                    );
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-ANALOG-001 — analog pin sharing a net with a digital output
// -----------------------------------------------------------------------------

struct AnalogDigitalMixingRule;

impl ErcRule for AnalogDigitalMixingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-ANALOG-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Analog
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let analog: Option<&_> = net.endpoints.iter().find(|e| {
                board
                    .pin(e.component, e.pin)
                    .is_some_and(|p| matches!(p.electrical_type, ElectricalType::Analog))
            });
            let digital: Option<&_> = net.endpoints.iter().find(|e| {
                board
                    .pin(e.component, e.pin)
                    .is_some_and(|p| matches!(p.electrical_type, ElectricalType::Output))
            });
            if let (Some(analog), Some(digital)) = (analog, digital) {
                let a_comp = board.component(analog.component).expect("ac");
                let a_pin = board.pin(analog.component, analog.pin).expect("ap");
                let d_comp = board.component(digital.component).expect("dc");
                let d_pin = board.pin(digital.component, digital.pin).expect("dp");
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "analog and digital output share a net",
                    )
                    .location(Location::from_span(file.to_string(), analog.source_span))
                    .expected(format!(
                        "analog pin `{}.{}` to connect to an analog signal, not a digital output",
                        a_comp.refdes, a_pin.name,
                    ))
                    .found(format!(
                        "`{}.{}` (analog) shares net `{}` with `{}.{}` (digital output)",
                        a_comp.refdes, a_pin.name, net.name, d_comp.refdes, d_pin.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-KEEPOUT-001 — keepout declaration with no radius
// -----------------------------------------------------------------------------

struct KeepoutMissingRadiusRule;

impl ErcRule for KeepoutMissingRadiusRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-KEEPOUT-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for ko in &board.keepouts {
            if ko.radius.is_none() {
                out.push(
                    DiagnosticBuilder::new(self.code(), Severity::Error, "keepout has no radius")
                        .location(Location::from_span(file.to_string(), ko.source_span))
                        .expected(format!(
                            "`keepout {}` to specify a positive radius",
                            ko.name
                        ))
                        .found("no radius attribute set".to_string())
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.8,
                            rationale: Some("add default 10mm radius constraint".into()),
                            patch_consequence_preview: None,
                            kind: synth_diagnostics::PatchKind::ReplaceRange {
                                range: ko.source_span,
                                replacement: format!(
                                    "keepout {} {{\n    radius 10mm\n  }}",
                                    ko.name
                                ),
                            },
                        })
                        .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-KEEPOUT-002 — keepout with non-positive radius
// -----------------------------------------------------------------------------

struct KeepoutZeroRadiusRule;

impl ErcRule for KeepoutZeroRadiusRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-KEEPOUT-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for ko in &board.keepouts {
            if let Some(radius) = ko.radius {
                if radius.0 <= 0 {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Error,
                            "keepout has non-positive radius",
                        )
                        .location(Location::from_span(file.to_string(), ko.source_span))
                        .expected(format!(
                            "`keepout {}` to specify a strictly positive radius",
                            ko.name,
                        ))
                        .found(format!("radius = {}nm", radius.0))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.8,
                            rationale: Some("set positive 10mm radius constraint".into()),
                            patch_consequence_preview: None,
                            kind: synth_diagnostics::PatchKind::ReplaceRange {
                                range: ko.source_span,
                                replacement: format!(
                                    "keepout {} {{\n    radius 10mm\n  }}",
                                    ko.name
                                ),
                            },
                        })
                        .build(),
                    );
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-001 — duplicate refdes across components
// -----------------------------------------------------------------------------

struct DuplicateRefdesRule;

impl ErcRule for DuplicateRefdesRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use std::collections::HashMap;
        let mut seen: HashMap<&str, &str> = HashMap::new();
        let mut out = Vec::new();
        for component in &board.components {
            if let Some(prev) = seen.insert(component.refdes.as_str(), component.refdes.as_str()) {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "duplicate component reference designator",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected("each refdes to appear at most once")
                    .found(format!("`{prev}` was already used by another component"))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-004 — refdes letter does not match the component kind
// -----------------------------------------------------------------------------

/// IEEE / industry reference-designator letters (Sierra Circuits
/// "How to Draw and Design a PCB Schematic", guideline 10). Only
/// kinds with a well-known letter are listed; unknown kinds are
/// never flagged (no false positives on new taxonomies).
fn accepted_refdes_prefixes(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "resistor" => &["R"][..],
        "capacitor" => &["C"][..],
        "inductor" => &["L"][..],
        "filter" => &["L", "FL", "F"][..],
        "diode" | "led" => &["D"][..],
        "transistor" => &["Q"][..],
        "crystal" => &["Y", "X"][..],
        "switch" => &["SW", "S"][..],
        "relay" => &["K"][..],
        "fuse" => &["F"][..],
        "battery" => &["BT"][..],
        "antenna" => &["E", "AN"][..],
        "buzzer" => &["LS", "BZ"][..],
        "connector" => &["J", "P", "CON"][..],
        // IC-family kinds: any package-level "U" convention.
        "ic" | "mcu" | "sensor" | "regulator" | "opamp" | "memory" | "modem" | "charger"
        | "secure_element" | "display" | "level_shifter" => &["U"][..],
        _ => return None,
    })
}

struct RefdesPrefixRule;

impl ErcRule for RefdesPrefixRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-004"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(accepted) = accepted_refdes_prefixes(&component.kind) else {
                continue;
            };
            // Leading letter run of the refdes (e.g. "SW3" -> "SW").
            let prefix: String = component
                .refdes
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            if accepted.contains(&prefix.as_str()) {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    format!(
                        "refdes prefix `{}` is unconventional for a {}",
                        prefix, component.kind
                    ),
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .expected(format!(
                    "a {}-prefixed refdes ({}), per the IEEE reference-designator                      convention reviewers expect",
                    accepted[0],
                    accepted
                        .iter()
                        .map(|p| format!("`{p}n`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
                .found(format!(
                    "`{}` declared as kind `{}`",
                    component.refdes, component.kind
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-002 — empty refdes
// -----------------------------------------------------------------------------

struct EmptyRefdesRule;

impl ErcRule for EmptyRefdesRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            if component.refdes.is_empty() {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "component refdes is empty",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected("a non-empty refdes such as `U1`, `R3`, `J5`")
                    .found("empty refdes")
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-003 — refdes does not start with an ASCII letter
// -----------------------------------------------------------------------------

struct RefdesFormatRule;

impl ErcRule for RefdesFormatRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-003"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let first = component.refdes.chars().next();
            if first.is_some_and(|c| !c.is_ascii_alphabetic()) {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "refdes does not begin with a letter",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected("refdes to begin with an ASCII letter (`U1`, `R3`, ...)")
                    .found(format!("`{}` starts with a non-letter", component.refdes))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-BOARD-001 — board declared with zero layers
// -----------------------------------------------------------------------------

struct BoardZeroLayersRule;

impl ErcRule for BoardZeroLayersRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-BOARD-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if board.layers == 0 {
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "board declared with zero layers",
                )
                .location(Location::from_span(file.to_string(), board.source_span))
                .expected("at least one copper layer")
                .found("layers = 0")
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-BOARD-002 — board has no components
// -----------------------------------------------------------------------------

struct EmptyBoardRule;

impl ErcRule for EmptyBoardRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-BOARD-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if board.components.is_empty() {
            out.push(
                DiagnosticBuilder::new(self.code(), Severity::Warning, "board has no components")
                    .location(Location::from_span(file.to_string(), board.source_span))
                    .expected("at least one component declared on the board")
                    .found("zero components")
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
            );
        }
        out
    }
}

fn cap_name(cap: PinCapability) -> &'static str {
    match cap {
        PinCapability::Gpio => "gpio",
        PinCapability::UsbDp => "usb_dp",
        PinCapability::UsbDn => "usb_dn",
        PinCapability::UsbVbus => "usb_vbus",
        PinCapability::SpiMosi => "spi_mosi",
        PinCapability::SpiMiso => "spi_miso",
        PinCapability::SpiSck => "spi_sck",
        PinCapability::SpiCs => "spi_cs",
        PinCapability::I2cSda => "i2c_sda",
        PinCapability::I2cScl => "i2c_scl",
        PinCapability::UartTx => "uart_tx",
        PinCapability::UartRx => "uart_rx",
        PinCapability::AnalogInput => "analog_input",
        PinCapability::AnalogOutput => "analog_output",
        PinCapability::ClockInput => "clock_input",
        PinCapability::ClockOutput => "clock_output",
        PinCapability::Reset => "reset",
        PinCapability::BootMode => "boot_mode",
        PinCapability::UsbCc => "usb_cc",
        PinCapability::RfFeed => "rf_feed",
        _ => "?",
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-USB-002 — USB-C CC pin missing pull-down resistor
// -----------------------------------------------------------------------------

struct UsbCcPullDownRule;

impl ErcRule for UsbCcPullDownRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-USB-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let has_cc = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(board, e.component, e.pin, &[PinCapability::UsbCc])
            });
            if !has_cc {
                continue;
            }
            let has_resistor = net.endpoints.iter().any(|e| {
                board
                    .component(e.component)
                    .and_then(|c| c.part.as_ref())
                    .is_some_and(|p| p.kind == "resistor")
            });
            if !has_resistor {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "USB-C CC pin missing pull-down resistor",
                    )
                    .location(Location::from_span(
                        file.to_string(),
                        net.endpoints[0].source_span,
                    ))
                    .expected("a pull-down resistor or CC controller connected to the CC net")
                    .found(format!(
                        "net `{}` carrying CC pin has no resistor endpoint",
                        net.name
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-SPI-002 — SPI MOSI/MISO pin direction mismatch
// -----------------------------------------------------------------------------

struct SpiDirectionRule;

impl ErcRule for SpiDirectionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-SPI-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let has_spi = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(
                    board,
                    e.component,
                    e.pin,
                    &[PinCapability::SpiMosi, PinCapability::SpiMiso],
                )
            });
            if !has_spi {
                continue;
            }
            let outputs: Vec<_> = net
                .endpoints
                .iter()
                .filter(|e| {
                    board
                        .pin(e.component, e.pin)
                        .is_some_and(|p| matches!(p.electrical_type, ElectricalType::Output))
                })
                .collect();
            if outputs.len() >= 2 {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "SPI direction collision (multiple outputs)",
                    )
                    .location(Location::from_span(
                        file.to_string(),
                        outputs[1].source_span,
                    ))
                    .expected("at most one output driver on SPI line")
                    .found(format!("multiple output drivers on SPI net `{}`", net.name))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-DIFF-003 — differential pair leg not fully connected
// -----------------------------------------------------------------------------

struct DiffPairBothLegsConnectedRule;

impl ErcRule for DiffPairBothLegsConnectedRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-DIFF-003"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for dp in &board.diff_pairs {
            let pos_len = board
                .nets
                .iter()
                .find(|n| net_matches_name(board, n, &dp.positive))
                .map_or(0, |n| n.endpoints.len());
            let neg_len = board
                .nets
                .iter()
                .find(|n| net_matches_name(board, n, &dp.negative))
                .map_or(0, |n| n.endpoints.len());

            if pos_len < 2 || neg_len < 2 {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "differential pair leg not fully connected",
                    )
                    .location(Location::from_span(file.to_string(), dp.source_span))
                    .expected(format!(
                        "both positive ({}) and negative ({}) legs to be connected to at least 2 endpoints",
                        dp.positive, dp.negative
                    ))
                    .found(format!(
                        "leg endpoints: {} has {}, {} has {}",
                        dp.positive, pos_len, dp.negative, neg_len
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .suggested_fix(synth_diagnostics::Patch {
                        confidence: 0.8,
                        rationale: Some("delete unconnected diff_pair declaration".into()),
                        patch_consequence_preview: None,
                        kind: synth_diagnostics::PatchKind::DeleteRange {
                            range: dp.source_span,
                        },
                    })
                    .build(),
                );
            }
        }
        out
    }
}

fn net_matches_name(board: &Board, net: &synth_ir::Net, target: &str) -> bool {
    if net.name == target {
        return true;
    }
    net.endpoints.iter().any(|e| {
        let Some(comp) = board.component(e.component) else {
            return false;
        };
        let Some(pin) = board.pin(e.component, e.pin) else {
            return false;
        };
        let refdes_pin = format!("{}_{}", comp.refdes.to_lowercase(), pin.name.to_lowercase());
        let dot_refdes_pin = format!("{}.{}", comp.refdes, pin.name);
        target.eq_ignore_ascii_case(&refdes_pin) || target.eq_ignore_ascii_case(&dot_refdes_pin)
    })
}

// -----------------------------------------------------------------------------
// E-SYNTH-RF-003 — RF feed net missing impedance constraint
// -----------------------------------------------------------------------------

struct RfFeedImpedanceRule;

impl ErcRule for RfFeedImpedanceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-RF-003"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Rf
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let rf_pins: Vec<_> = part
                .pins
                .iter()
                .enumerate()
                .filter(|(_, p)| p.capabilities.contains(&PinCapability::RfFeed))
                .collect();

            for (idx, pin) in rf_pins {
                let pid = PinId(idx as u32);
                let Some((_, net)) = board.nets_containing(component.id, pid).next() else {
                    continue;
                };
                let has_impedance = board.diff_pairs.iter().any(|dp| {
                    (net_matches_name(board, net, &dp.positive)
                        || net_matches_name(board, net, &dp.negative))
                        && dp.impedance.is_some()
                });
                if !has_impedance {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Warning,
                            "RF feed net missing impedance constraint",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(format!(
                            "controlled impedance constraint for RF feed pin `{}.{}` on net `{}`",
                            component.refdes, pin.name, net.name
                        ))
                        .found("no impedance constraint specified for this net".to_string())
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .smt_constraint("(assert (= impedance 50))")
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.95,
                            rationale: Some(
                                "SMT quantitative 50ohm RF impedance constraint".into(),
                            ),
                            patch_consequence_preview: None,
                            kind: synth_diagnostics::PatchKind::SolveSmt {
                                constraint: "(assert (= impedance 50))".into(),
                                target_range: Some(synth_diagnostics::Span::new(
                                    component.source_span.byte_end,
                                    component.source_span.byte_end,
                                )),
                                replacement_template: Some(
                                    "\n  // RF feed impedance: {}ohm".into(),
                                ),
                            },
                        })
                        .build(),
                    );
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-004 — Residual Energy anomaly detection rule
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct ResidualEnergyAnomalyRule;

impl ErcRule for ResidualEnergyAnomalyRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-004"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    #[allow(clippy::cast_precision_loss)]
    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        use synth_registry::ElectricalType;
        let mut out = Vec::new();
        for net in &board.nets {
            let is_power_net = net.endpoints.iter().any(|e| {
                board.pin(e.component, e.pin).is_some_and(|p| {
                    matches!(
                        p.electrical_type,
                        ElectricalType::PowerInput | ElectricalType::PowerOutput
                    )
                })
            }) || net.name.to_lowercase().contains("vcc")
                || net.name.to_lowercase().contains("vdd")
                || net.name.to_lowercase().contains("vbatt")
                || net.name.to_lowercase().contains("vbus");

            if !is_power_net {
                continue;
            }

            let pin_count = net.endpoints.len();
            let decoupling_count = net
                .endpoints
                .iter()
                .filter(|e| {
                    board
                        .component(e.component)
                        .and_then(|c| c.part.as_ref())
                        .is_some_and(|p| p.kind == "capacitor")
                })
                .count();

            let residual_score = (pin_count as f64) / ((decoupling_count as f64) + 1.0);
            if pin_count >= 4 && residual_score >= 3.0 {
                let first = &net.endpoints[0];
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "power net residual energy anomaly detected",
                    )
                    .location(Location::from_span(file.to_string(), first.source_span))
                    .expected(format!(
                        "decoupling capacitor density matching net load on `{}`",
                        net.name
                    ))
                    .found(format!(
                        "power net `{}` has {} load endpoints but only {} decoupling capacitor(s) (residual score {:.2} > 3.0σ threshold)",
                        net.name, pin_count, decoupling_count, residual_score
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .smt_constraint("(assert (<= residual_score 3.0))".to_string())
                    .build(),
                );
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CRYSTAL-001 — crystal load capacitors must be balanced
// -----------------------------------------------------------------------------

/// A quartz crystal needs equal loading on both pins; a large mismatch
/// (e.g. C1 = 22 pF, C2 = 33 pF) shifts the parallel resonance and the
/// start-up margin. Emits a warning when the two load caps differ by
/// more than [`BALANCE_TOLERANCE`].
struct CrystalLoadCapBalanceRule;

/// Maximum acceptable fractional difference between the two load caps.
const BALANCE_TOLERANCE: f64 = 0.10;

impl ErcRule for CrystalLoadCapBalanceRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CRYSTAL-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Analog
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "crystal" {
                continue;
            }

            // Collect the parsed load-capacitance on each pin. A healthy
            // crystal has exactly two load caps, one per pin.
            let mut pins: Vec<(PinId, f64)> = Vec::new();
            for (pin_idx, _) in part.pins.iter().enumerate() {
                let pid = PinId(pin_idx as u32);
                for (_net_id, net) in board.nets_containing(component.id, pid) {
                    for ep in &net.endpoints {
                        if ep.component == component.id {
                            continue;
                        }
                        let Some(other) = board.component(ep.component) else {
                            continue;
                        };
                        let Some(other_part) = other.part.as_ref() else {
                            continue;
                        };
                        if other_part.kind != "capacitor" {
                            continue;
                        }
                        if let Some(farads) = other.value.as_deref().and_then(parse_capacitance) {
                            pins.push((pid, farads));
                        }
                    }
                }
            }

            // Not a two-cap-per-pin crystal (missing, shared, or
            // unvalued caps) — leave that to the topology rules.
            if pins.len() != 2 || pins[0].0 == pins[1].0 {
                continue;
            }

            let (v0, v1) = (pins[0].1, pins[1].1);
            let larger = v0.max(v1);
            let smaller = v0.min(v1);
            if smaller / larger < 1.0 - BALANCE_TOLERANCE {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "crystal load capacitors are unbalanced",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected(format!(
                        "load capacitors on {} within {:.0}% of each other",
                        component.refdes,
                        BALANCE_TOLERANCE * 100.0
                    ))
                    .found(format!(
                        "crystal `{}` load caps differ by {:.0}% ({} vs {})",
                        component.refdes,
                        (1.0 - smaller / larger) * 100.0,
                        fmt_farad(v0),
                        fmt_farad(v1)
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

/// Render a capacitance in farads back to a compact string for
/// diagnostics and BOM display.
pub fn fmt_farad(f: f64) -> String {
    if f >= 1e-6 {
        format!("{:.3}µF", f / 1e-6)
    } else if f >= 1e-9 {
        format!("{:.3}nF", f / 1e-9)
    } else {
        format!("{:.3}pF", f / 1e-12)
    }
}

#[derive(Debug)]
pub struct SchematicInvertedPowerSymbolRule;
impl ErcRule for SchematicInvertedPowerSymbolRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SCHEM-001"
    }
    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }
    fn check(&self, _board: &Board, _file: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
}

#[derive(Debug)]
pub struct SchematicWireCrossingRule;
impl ErcRule for SchematicWireCrossingRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SCHEM-002"
    }
    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }
    fn check(&self, _board: &Board, _file: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
}

#[derive(Debug)]
pub struct SchematicDecouplingDistanceRule;
impl ErcRule for SchematicDecouplingDistanceRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SCHEM-003"
    }
    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }
    fn check(&self, _board: &Board, _file: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
}

#[derive(Debug)]
pub struct SchematicLongWireRule;
impl ErcRule for SchematicLongWireRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SCHEM-004"
    }
    fn category(&self) -> ErcCategory {
        ErcCategory::Geometry
    }
    fn check(&self, _board: &Board, _file: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-005 — Voltage domain mismatch across net endpoints
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct PowerDomainMismatchRule;

impl ErcRule for PowerDomainMismatchRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-005"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let domain_map = synth_ir::power_domains::infer_power_domains(board);

        for net in &board.nets {
            // Check if any level shifter or isolator component is on this net.
            let has_level_shifter = net.endpoints.iter().any(|e| {
                board
                    .component(e.component)
                    .and_then(|c| c.part.as_ref())
                    .is_some_and(synth_registry::Part::is_level_shifter)
            });
            if has_level_shifter {
                continue;
            }

            let domain = domain_map.get(net.id);
            let net_voltage = domain.and_then(synth_ir::PowerDomainKind::nominal_voltage);

            // 1. Check for driver voltage overstressing receiver pin voltage_max_v
            if let Some(v_driver) = net_voltage {
                for endpoint in &net.endpoints {
                    let Some(component) = board.component(endpoint.component) else {
                        continue;
                    };
                    let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                        continue;
                    };

                    if let Some(v_max) = pin.voltage_max_v {
                        if v_driver > v_max + 0.3 {
                            out.push(
                                DiagnosticBuilder::new(
                                    self.code(),
                                    Severity::Error,
                                    "voltage domain mismatch detected",
                                )
                                .location(Location::from_span(file.to_string(), endpoint.source_span))
                                .expected(format!(
                                    "endpoint `{}.{}` max safe voltage ({:.1}V) to tolerate driven voltage ({:.1}V)",
                                    component.refdes, pin.name, v_max, v_driver
                                ))
                                .found(format!(
                                    "`{}.{}` on net `{}` operates at {:.1}V domain, exceeding max safe voltage ({:.1}V)",
                                    component.refdes, pin.name, net.name, v_driver, v_max
                                ))
                                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                                .smt_constraint(format!("(assert (<= voltage_driver {v_max:.1}))"))
                                .build(),
                            );
                        }
                    }
                }
            }

            // 2. Check for rail voltage short (multiple PowerOutputs with different voltages on same net)
            let power_outputs: Vec<_> = net
                .endpoints
                .iter()
                .filter_map(|e| {
                    let pin = board.pin(e.component, e.pin)?;
                    if pin.electrical_type == synth_registry::ElectricalType::PowerOutput {
                        let comp = board.component(e.component)?;
                        Some((comp, pin, e.source_span, pin.nominal_voltage_v()))
                    } else {
                        None
                    }
                })
                .collect();

            if power_outputs.len() >= 2 {
                let (c1, p1, _, v1_opt) = &power_outputs[0];
                let (c2, p2, span2, v2_opt) = &power_outputs[1];
                if let (Some(v1), Some(v2)) = (v1_opt, v2_opt) {
                    if (v1 - v2).abs() > 0.3 {
                        out.push(
                            DiagnosticBuilder::new(
                                self.code(),
                                Severity::Error,
                                "power rail voltage domain clash",
                            )
                            .location(Location::from_span(file.to_string(), *span2))
                            .expected("power outputs connected to the same net to operate at identical nominal voltages")
                            .found(format!(
                                "net `{}` connects `{}.{}` ({:.1}V) and `{}.{}` ({:.1}V)",
                                net.name, c1.refdes, p1.name, v1, c2.refdes, p2.name, v2
                            ))
                            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                            .build(),
                        );
                    }
                }
            }
        }

        out
    }
}

// -----------------------------------------------------------------------------
// W-SYNTH-SUPPLY-001 — Supply Chain Stock / EOL warning
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupplyChainRule;

impl ErcRule for SupplyChainRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SUPPLY-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        let Ok(cache) = synth_supply::SupplyCache::default_location() else {
            return diags;
        };

        for inst in &board.components {
            let Some(part) = &inst.part else {
                continue;
            };

            let target_pn = part
                .lcsc_pn
                .as_deref()
                .or(part.mpn.as_deref())
                .unwrap_or("");

            if target_pn.is_empty() {
                continue;
            }

            let status = cache
                .get("LCSC", target_pn)
                .or_else(|| cache.get("Nexar", target_pn));

            if let Some(s) = status {
                if s.lifecycle.is_problematic() || !s.in_stock {
                    let issue = if s.lifecycle == synth_supply::LifecycleStatus::Obsolete {
                        "marked Obsolete"
                    } else if s.lifecycle == synth_supply::LifecycleStatus::Nrnd {
                        "marked Not Recommended for New Designs (NRND)"
                    } else {
                        "out of stock (0 available)"
                    };

                    let ref_des = &inst.refdes;
                    let part_id = &part.id;

                    let mut b = DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        format!(
                            "Component {ref_des} ({part_id}) is {issue} at {} (PN: {})",
                            s.distributor, s.part_number
                        ),
                    )
                    .location(Location::from_span(file.to_string(), inst.source_span));

                    if !part.substitutes.is_empty() {
                        let subs = part
                            .substitutes
                            .iter()
                            .map(synth_registry::PartId::as_str)
                            .collect::<Vec<_>>()
                            .join(", ");
                        b = b.message(format!("Known substitute parts in registry: {subs}"));
                    }

                    diags.push(b.build());
                }
            }
        }

        diags
    }
}

// -----------------------------------------------------------------------------
// W-SYNTH-PART-UNVERIFIED — component uses a part without review
// -----------------------------------------------------------------------------

struct UnverifiedPartRule;

impl ErcRule for UnverifiedPartRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-PART-UNVERIFIED"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if !part.is_unverified() {
                continue;
            }
            let source = part
                .provenance
                .as_ref()
                .map(|p| format!(" ({:?})", p.source))
                .unwrap_or_default();
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "component uses an unverified part",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .primary_entity(EntityRef::Component {
                    id: component.refdes.clone(),
                })
                .expected(format!(
                    "part `{}` carries a `reviewed_by` entry before fab export",
                    part.id
                ))
                .found(format!(
                    "part `{}` has no reviewer{} — escape to PCB/assembly is at your own risk",
                    part.id, source
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .suggested_fix(synth_diagnostics::Patch {
                    confidence: 0.3,
                    rationale: Some(format!(
                        "add reviewed_by to `registry/parts/**/{}.synth.toml` provenance",
                        part.id
                    )),
                    patch_consequence_preview: None,
                    kind: synth_diagnostics::PatchKind::InsertAt {
                        at: component.source_span.byte_end.saturating_sub(1),
                        text: String::new(),
                    },
                })
                .build(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_residual_energy_anomaly_rule_code() {
        let rule = ResidualEnergyAnomalyRule;
        assert_eq!(rule.code(), "E-SYNTH-POWER-004");
        assert_eq!(rule.category(), ErcCategory::Power);
    }

    #[test]
    fn test_crystal_load_cap_balance_rule_code() {
        let rule = CrystalLoadCapBalanceRule;
        assert_eq!(rule.code(), "E-SYNTH-CRYSTAL-001");
        assert_eq!(rule.category(), ErcCategory::Analog);
    }

    #[test]
    fn unverified_part_emits_warning() {
        use synth_diagnostics::Span;
        use synth_ir::{Board, Component, ComponentId};
        use synth_registry::{
            ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber, Provenance, ProvenanceSource,
        };

        // A board with one component whose resolved part carries an
        // `authored` provenance with no reviewer → must warn.
        let part = Part {
            id: PartId("mystery".into()),
            kind: "ic".into(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::default(),
            signed_by: vec![],
            substitutes: vec![],
            mpn: None,
            lcsc_pn: None,
            pins: vec![Pin {
                name: "p1".into(),
                number: PinNumber("1".into()),
                electrical_type: ElectricalType::Input,
                capabilities: vec![],
                required: false,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            }],
            required_decoupling: vec![],
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: Some(Provenance {
                source: ProvenanceSource::Authored,
                reviewed_by: None,
                ..Default::default()
            }),
        };
        let board = Board {
            name: "b".into(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: vec![Component {
                id: ComponentId(0),
                refdes: "U1".into(),
                kind: "ic".into(),
                part: Some(part),
                value: None,
                placement_hint: None,
                group: None,
                source_span: Span::new(0, 0),
            }],
            nets: vec![],
            diff_pairs: vec![],
            keepouts: vec![],
            source_span: Span::new(0, 0),
        };

        let diags = run_erc(&board, "test.synth");
        let unverified = diags
            .iter()
            .filter(|d| d.code == "W-SYNTH-PART-UNVERIFIED")
            .count();
        assert_eq!(unverified, 1, "exactly one unverified-part warning");
    }
}
