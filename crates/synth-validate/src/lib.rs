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
pub mod config;
pub mod deep_erc;
pub use anomaly::{extract_features, BoardFeatureVec, GraphAnomalyDetectorRule};
pub use config::{ErcConfig, PinConflictTable};

pub mod value;
pub use value::{parse_capacitance, parse_resistance, parse_voltage};

pub mod patch_mlp;
pub use patch_mlp::PatchMlp;

pub mod placement;
pub use placement::validate_placement;

pub fn run_erc(board: &Board, file: &str) -> Vec<Diagnostic> {
    run_erc_with_config(board, file, &ErcConfig::default())
}

/// [`run_erc`] with an explicit [`ErcConfig`]: the pin-type conflict
/// table and the thresholds the deeper checks use.
pub fn run_erc_with_config(board: &Board, file: &str, config: &ErcConfig) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for rule in all_rules(config) {
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

fn all_rules(config: &ErcConfig) -> Vec<Box<dyn ErcRule>> {
    vec![
        Box::new(RequiredPinsConnectedRule),
        Box::new(SingleEndpointNetRule),
        Box::new(NoConnectMismatchRule),
        Box::new(OutputCollisionRule),
        Box::new(NoDriverRule),
        Box::new(OrphanComponentRule),
        Box::new(PowerOutputShortRule),
        Box::new(MissingDecouplingRule),
        Box::new(DecouplingValueRule),
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
        Box::new(SourcingIdentityRule),
        Box::new(UnverifiedPartRule),
        Box::new(DividerRatioRule),
        // Phase 6 — deeper ERC.
        Box::new(deep_erc::PinConflictRule::new(config)),
        Box::new(deep_erc::PullupRailMismatchRule::new(config)),
        Box::new(deep_erc::RegulatorInputRangeRule),
        Box::new(deep_erc::PowerBudgetRule::new(config)),
        Box::new(deep_erc::FloatingCmosInputRule),
        Box::new(deep_erc::OpenDrainPullupRule::new(config)),
        Box::new(deep_erc::ConnectorProtectionRule::new(config)),
        Box::new(deep_erc::LedCurrentRule::new(config)),
        Box::new(deep_erc::CaseOnlyNetCollisionRule),
        Box::new(deep_erc::SingleUseLabelRule),
        Box::new(deep_erc::GroundPinOffGroundNetRule),
        Box::new(deep_erc::MultiUnitRailSplitRule),
        Box::new(deep_erc::PinFunctionSupportRule),
        Box::new(deep_erc::CeramicDcBiasDeratingRule::new(config)),
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
                            "pin {} is marked required by part `{}` and must be \
                             connected",
                            component.describe_pin(&pin.name),
                            part.id,
                        ))
                        .found(format!("{} is floating", component.describe_pin(&pin.name)))
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
                    "{} on net `{}` carries none of those capabilities",
                    component.describe_pin(&pin.name),
                    net.name,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{code}"))
                .build(),
        );
    }
}

pub(crate) fn endpoint_has_any_capability(
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
            // A *declared* name that joins one endpoint is
            // `E-SYNTH-NAME-008`'s case (a label used only once); this
            // rule owns the unnamed auto-net case so exactly one of the
            // two fires.
            if net.name.starts_with("net_") && net.endpoints.len() == 1 {
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
                        Severity::Warning,
                        "net has only one endpoint",
                    )
                    .location(Location::from_span(file.to_string(), endpoint.source_span))
                    .expected("at least two endpoints (a wire must connect something to something)")
                    .found(format!(
                        "net `{}` has only {}",
                        net.name,
                        component.describe_pin(&pin.name),
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
                            "{} is declared no_connect on part `{}` but is on net `{}`",
                            component.describe_pin(&pin.name),
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

fn next_free_refdes_number(board: &Board, prefix: &str) -> i64 {
    board
        .components
        .iter()
        .filter_map(|component| component.refdes.strip_prefix(prefix)?.parse::<i64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// Find the ground pin connected to `component` — a `PowerInput` pin
/// named gnd/vss/... Returns the pin's name, or `None` if the component
/// has no connected ground pin (then the cap cannot be completed).
fn ground_pin_for(board: &Board, component: &Component) -> Option<String> {
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
            if let Some((_, _net)) = board.nets_containing(component.id, pid).next() {
                return Some(pin.name.clone());
            }
        }
    }
    None
}

/// Build a textual patch that inserts `count` decoupling capacitors and
/// their two connects each, right after `component`'s declaration, so
/// the source becomes fixable with a single byte-range patch.
fn decoupling_cap_patch(
    component: &Component,
    net: &str,
    gnd_pin: &str,
    count: usize,
    next_cap_number: &mut i64,
) -> PatchKind {
    use std::fmt::Write as _;
    let mut text = String::new();
    for _ in 0..count {
        let cap = format!("C{}", *next_cap_number);
        *next_cap_number += 1;
        let _ = writeln!(
            text,
            "\n  component {cap}: capacitor \"c_generic_0603\" // auto-inserted decoupling"
        );
        let _ = writeln!(text, "  connect {}.{net} -> {cap}.p1", component.refdes);
        let _ = writeln!(text, "  connect {}.{gnd_pin} -> {cap}.p2", component.refdes);
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
        let mut next_cap_number = next_free_refdes_number(board, "C");
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
                        "{required} capacitor(s) on the net carrying {}",
                        component.describe_pin(&decoupling.net),
                    ))
                    .found(format!(
                        "{cap_count} capacitor(s) found on net `{}`",
                        net.name,
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .smt_constraint(format!("(assert (>= decoupling_count {required}))"));

                    // Auto-insert the missing cap(s) into the source
                    // when a ground net is available to complete them.
                    if let Some(gnd_pin) = ground_pin_for(board, component) {
                        let shortfall = required - cap_count;
                        builder = builder.suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.9,
                            rationale: Some(format!(
                                "auto-insert {shortfall} 100nF decoupling cap(s) on {}",
                                component.describe_pin(&decoupling.net),
                            )),
                            patch_consequence_preview: None,
                            kind: decoupling_cap_patch(
                                component,
                                &decoupling.net,
                                &gnd_pin,
                                shortfall,
                                &mut next_cap_number,
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
// E-SYNTH-POWER-006 — decoupling capacitance below required value
// -----------------------------------------------------------------------------

/// POWER-001 counts capacitors; this rule weighs them. A part whose
/// manifest requires e.g. `10uF` on `vin` is not decoupled by a lone
/// `100nF` cap even though the count is satisfied. The rule sums the
/// parseable capacitance on the net and warns when the total is below
/// the manifest value. Nets with any unparseable or missing cap value
/// are skipped (topology-only judgement would false-positive on e.g.
/// `4k7`-style strings the value parser rejects); POWER-001 still
/// owns the count check, so nothing is double-flagged.
struct DecouplingValueRule;

impl ErcRule for DecouplingValueRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-006"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            for decoupling in &part.required_decoupling {
                let Some(required_f) = parse_capacitance(&decoupling.value) else {
                    continue;
                };
                let Some(pin_idx) = part.pins.iter().position(|p| p.name == decoupling.net) else {
                    continue;
                };
                let pid = PinId(pin_idx as u32);
                let Some((_, net)) = board.nets_containing(component.id, pid).next() else {
                    // Pin floating — E-SYNTH-CONNECT-001 owns that.
                    continue;
                };
                let caps: Vec<&Component> = net
                    .endpoints
                    .iter()
                    .filter_map(|e| board.component(e.component))
                    .filter(|c| c.part.as_ref().is_some_and(|p| p.kind == "capacitor"))
                    .collect();
                if caps.is_empty() {
                    // No caps at all — POWER-001 owns the count check.
                    continue;
                }
                // Any cap whose value cannot be weighed vetoes the
                // judgement for this net: guessing would false-positive.
                let mut total_f = 0.0;
                let mut all_parseable = true;
                for cap in &caps {
                    if let Some(f) = cap.value.as_deref().and_then(parse_capacitance) {
                        total_f += f;
                    } else {
                        all_parseable = false;
                        break;
                    }
                }
                if !all_parseable {
                    continue;
                }
                if total_f < required_f {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Warning,
                            "decoupling capacitance below required value",
                        )
                        .location(Location::from_span(file.to_string(), component.source_span))
                        .expected(format!(
                            "at least {} of decoupling on the net carrying {}",
                            decoupling.value,
                            component.describe_pin(&decoupling.net),
                        ))
                        .found(format!(
                            "{} capacitor(s) totalling {:.3}µF on net `{}`",
                            caps.len(),
                            total_f * 1e6,
                            net.name,
                        ))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .smt_constraint(format!(
                            "(assert (>= decoupling_capacitance {required_f:.9}))"
                        ))
                        .build(),
                    );
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
                    "net `{}` carries {} and {}, both power outputs",
                    net.name,
                    comp1.describe_pin(&pin1.name),
                    comp2.describe_pin(&pin2.name),
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
                        "a verified resistor must connect `{}` to a verified power rail; add one pullup on each SDA/SCL net",
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
                    "pin {} is declared rf_feed but no keepouts exist",
                    component.describe_pin(&pin.name),
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
                    "{} is the {}th output pin on net `{}`",
                    comp.describe_pin(&pin.name),
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
                        "all {} endpoints on net `{}` are input-only (e.g. {})",
                        net.endpoints.len(),
                        net.name,
                        comp.describe_pin(&pin.name),
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
                        "component {} to participate in at least one `connect` statement",
                        component.describe(),
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
                        "net `{}` carries power_input pin {} but no power source",
                        net.name,
                        comp.describe_pin(&pin.name),
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
                    "{} is the {}th rf_feed pin on this net",
                    comp.describe_pin(&pin.name),
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
                    "{} is the {}th clock_output pin on net `{}`",
                    comp.describe_pin(&pin.name),
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
                        "net `{}` carries clock_input {} but no clock source",
                        net.name,
                        comp.describe_pin(&pin.name),
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
                            "pin {} (reset capability) to be connected to a pull-up or reset button",
                            component.describe_pin(&pin.name),
                        ))
                        .found(format!("{} is floating", component.describe_pin(&pin.name)))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .suggested_fix(synth_diagnostics::Patch {
                            confidence: 0.7,
                            rationale: Some(format!("wire reset pin {} to connector", component.describe_pin(&pin.name))),
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
                            "pin {} (boot_mode capability) to be strapped high or low",
                            component.describe_pin(&pin.name),
                        ))
                        .found(format!("{} is floating", component.describe_pin(&pin.name)))
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
                        "analog pin {} to connect to an analog signal, not a digital output",
                        a_comp.describe_pin(&a_pin.name),
                    ))
                    .found(format!(
                        "{} (analog) shares net `{}` with {} (digital output)",
                        a_comp.describe_pin(&a_pin.name),
                        net.name,
                        d_comp.describe_pin(&d_pin.name),
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
            if seen
                .insert(component.refdes.as_str(), component.refdes.as_str())
                .is_some()
            {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "duplicate component reference designator",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected("each refdes to appear at most once")
                    .found(format!(
                        "{} was already used by another component",
                        component.describe()
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
                    "{} declared as kind `{}`",
                    component.describe(),
                    component.kind
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
                    .found(format!("{} starts with a non-letter", component.describe()))
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
            // Named nets resolve at lowering (`net "USB_DP" { … }`
            // joined with `diff_pair USB_DP USB_DN`): count the
            // resolved nets directly. Unresolved legs (legacy designs
            // without named nets) fall back to endpoint-name matching.
            let pos_len = dp.positive_net.map_or_else(
                || {
                    board
                        .nets
                        .iter()
                        .find(|n| net_matches_name(board, n, &dp.positive))
                        .map_or(0, |n| n.endpoints.len())
                },
                |id| board.net(id).map_or(0, |n| n.endpoints.len()),
            );
            let neg_len = dp.negative_net.map_or_else(
                || {
                    board
                        .nets
                        .iter()
                        .find(|n| net_matches_name(board, n, &dp.negative))
                        .map_or(0, |n| n.endpoints.len())
                },
                |id| board.net(id).map_or(0, |n| n.endpoints.len()),
            );

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
                            "controlled impedance constraint for RF feed pin {} on net `{}`",
                            component.describe_pin(&pin.name),
                            net.name
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
                        component.describe(),
                        BALANCE_TOLERANCE * 100.0
                    ))
                    .found(format!(
                        "crystal {} load caps differ by {:.0}% ({} vs {})",
                        component.describe(),
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
                                    "endpoint {} max safe voltage ({:.1}V) to tolerate driven voltage ({:.1}V)",
                                    component.describe_pin(&pin.name),
                                    v_max,
                                    v_driver
                                ))
                                .found(format!(
                                    "{} on net `{}` operates at {:.1}V domain, exceeding max safe voltage ({:.1}V)",
                                    component.describe_pin(&pin.name),
                                    net.name,
                                    v_driver,
                                    v_max
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
                                "net `{}` connects {} ({:.1}V) and {} ({:.1}V)",
                                net.name,
                                c1.describe_pin(&p1.name),
                                v1,
                                c2.describe_pin(&p2.name),
                                v2
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
// W-SYNTH-SUPPLY-002 — part has no distributor identity
// -----------------------------------------------------------------------------

/// `W-SYNTH-SUPPLY-001` reasons about cached stock/lifecycle for a
/// known part number; this rule fires one step earlier, when the
/// part has *no* number to look up. The exporter stamps hidden
/// `MPN`/`LCSC` fields and the fab BOM plugins read exactly those
/// spellings — a part with neither `mpn` nor `lcsc_pn` cannot be
/// quoted, checked for stock, or substituted at release. Generic
/// passives (`r_generic_0603`) are exempt: their value field plus
/// the generic footprint is orderable without a part number.
struct SourcingIdentityRule;

impl ErcRule for SourcingIdentityRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-SUPPLY-002"
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
            if part.mpn.is_some() || part.lcsc_pn.is_some() {
                continue;
            }
            if part.id.as_str().starts_with("r_generic")
                || part.id.as_str().starts_with("c_generic")
            {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "part has no distributor identity",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .primary_entity(EntityRef::Component {
                    id: component.refdes.clone(),
                })
                .expected(format!(
                    "part `{}` carries an `mpn` and/or `lcsc_pn` so the BOM is quotable",
                    part.id
                ))
                .found(format!(
                    "{} ({}) has neither `mpn` nor `lcsc_pn` — stock, pricing, and \
                     substitution checks (W-SYNTH-SUPPLY-001) cannot run for it",
                    component.describe(),
                    part.id,
                ))
                .message(format!(
                    "add mpn/lcsc_pn to `registry/parts/**/{}.synth.toml` (or a Tier-2 overlay)",
                    part.id
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
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

// -----------------------------------------------------------------------------
// W-SYNTH-DIVIDER-001 — degenerate resistor-divider ratio
// -----------------------------------------------------------------------------

/// Value-based check on rail→R1→mid→R2→gnd dividers (the topology
/// `synth-layout` recognises as its `Divider` cluster). The compiler
/// cannot know the intended output voltage, but a divider whose
/// mid-point sits below 5% or above 95% of the rail is almost never
/// intended: the usual causes are R1/R2 swapped in placement or an
/// order-of-magnitude value typo (`10k` vs `100k`). Either resistor
/// with a missing or unparseable `value` vetoes the judgement;
/// non-rail-anchored resistor pairs (ladders, feedback networks)
/// never match, because the rail side must carry a power-output pin
/// and the foot side a ground pin.
struct DividerRatioRule;

fn net_has_power_output(board: &Board, net: &synth_ir::Net) -> bool {
    net.endpoints.iter().any(|e| {
        board
            .pin(e.component, e.pin)
            .is_some_and(|p| p.electrical_type == ElectricalType::PowerOutput)
    })
}

fn net_has_ground_pin(board: &Board, net: &synth_ir::Net) -> bool {
    net.endpoints.iter().any(|e| {
        board.pin(e.component, e.pin).is_some_and(|p| {
            matches!(
                p.electrical_type,
                ElectricalType::PowerInput | ElectricalType::GroundReference
            ) && {
                let n = p.name.to_lowercase();
                matches!(
                    n.as_str(),
                    "gnd" | "vss" | "vssa" | "vee" | "agnd" | "dgnd" | "vneg" | "ground"
                ) || n.starts_with("gnd")
                    || n.starts_with("vss")
            }
        })
    })
}

fn is_two_pin_resistor(board: &Board, id: ComponentId) -> bool {
    board.component(id).is_some_and(|c| {
        c.part
            .as_ref()
            .is_some_and(|p| p.kind == "resistor" && p.pins.len() == 2)
    })
}

impl ErcRule for DividerRatioRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-DIVIDER-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Analog
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for r1 in &board.components {
            if !is_two_pin_resistor(board, r1.id) {
                continue;
            }
            // Try each pin as the mid-side pin; the other is the rail side.
            for mid_idx in [0u32, 1u32] {
                let rail_idx = 1 - mid_idx;
                let Some((_, mid_net)) = board.nets_containing(r1.id, PinId(mid_idx)).next() else {
                    continue;
                };
                // Mid net: exactly this resistor plus its partner.
                if mid_net.endpoints.len() != 2 {
                    continue;
                }
                let Some(partner_ep) = mid_net.endpoints.iter().find(|e| e.component != r1.id)
                else {
                    continue;
                };
                if !is_two_pin_resistor(board, partner_ep.component) {
                    continue;
                }
                let Some(r2) = board.component(partner_ep.component) else {
                    continue;
                };
                // Rail side must be a driven rail; foot side ground.
                let rail_ok = board
                    .nets_containing(r1.id, PinId(rail_idx))
                    .next()
                    .is_some_and(|(_, n)| net_has_power_output(board, n));
                if !rail_ok {
                    continue;
                }
                let r2_other: Vec<PinId> = r2
                    .part
                    .as_ref()
                    .map(|p| {
                        (0..p.pins.len())
                            .map(|i| PinId(i as u32))
                            .filter(|pid| *pid != partner_ep.pin)
                            .collect()
                    })
                    .unwrap_or_default();
                let gnd_ok = r2_other.into_iter().any(|pid| {
                    board
                        .nets_containing(r2.id, pid)
                        .next()
                        .is_some_and(|(_, n)| net_has_ground_pin(board, n))
                });
                if !gnd_ok {
                    continue;
                }
                let (Some(r1_ohms), Some(r2_ohms)) = (
                    r1.value.as_deref().and_then(parse_resistance),
                    r2.value.as_deref().and_then(parse_resistance),
                ) else {
                    continue;
                };
                if r1_ohms <= 0.0 || r2_ohms <= 0.0 {
                    continue;
                }
                let ratio = r2_ohms / (r1_ohms + r2_ohms);
                if !(0.05..=0.95).contains(&ratio) {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Warning,
                            "degenerate resistor-divider ratio",
                        )
                        .location(Location::from_span(file.to_string(), r1.source_span))
                        .expected(
                            "divider mid-point between 5% and 95% of the rail \
                             (check R1/R2 placement and value magnitudes)",
                        )
                        .found(format!(
                            "{}/{} ratio gives mid at {:.1}% of rail",
                            r1.describe(),
                            r2.describe(),
                            ratio * 100.0,
                        ))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .build(),
                    );
                }
                // One judgement per divider: the partner resistor
                // would otherwise re-derive the same net from its
                // side only when it is also rail-driven, which the
                // power-output anchor above excludes.
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

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

    fn pin(name: &str, t: ElectricalType) -> Pin {
        Pin {
            name: name.into(),
            number: PinNumber(name.into()),
            electrical_type: t,
            capabilities: vec![],
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(id: &str, kind: &str, pins: Vec<Pin>) -> Part {
        Part {
            id: PartId(id.into()),
            kind: kind.into(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::default(),
            signed_by: vec![],
            substitutes: vec![],
            mpn: None,
            lcsc_pn: None,
            pins,
            required_decoupling: vec![],
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }

    fn divider_test_board(r1_value: Option<&str>, r2_value: Option<&str>) -> Board {
        use synth_diagnostics::Span;
        use synth_ir::{Net, NetEndpoint, NetId};
        use synth_registry::{ElectricalType, Part, Pin};

        let reg = part(
            "reg",
            "regulator",
            vec![
                pin("vin", ElectricalType::PowerInput),
                pin("gnd", ElectricalType::PowerInput),
                pin("vout", ElectricalType::PowerOutput),
            ],
        );
        let r = |pins: Vec<Pin>| part("r", "resistor", pins);
        let rpins = || {
            vec![
                pin("p1", ElectricalType::Passive),
                pin("p2", ElectricalType::Passive),
            ]
        };
        let ep = |c: u32, p: u32| NetEndpoint {
            component: ComponentId(c),
            pin: PinId(p),
            source_span: Span::new(0, 0),
        };
        let comp = |i: u32, refdes: &str, kind: &str, p: Part, value: Option<&str>| Component {
            id: ComponentId(i),
            refdes: refdes.into(),
            kind: kind.into(),
            part: Some(p),
            value: value.map(str::to_string),
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        };
        Board {
            name: "b".into(),
            layers: 2,
            manufacturer: None,
            company: None,
            revision: None,
            components: vec![
                comp(0, "U1", "regulator", reg, None),
                comp(1, "R1", "resistor", r(rpins()), r1_value),
                comp(2, "R2", "resistor", r(rpins()), r2_value),
            ],
            nets: vec![
                Net {
                    id: NetId(0),
                    name: "rail".into(),
                    endpoints: vec![ep(0, 2), ep(1, 0)],
                    netclass: None,
                    voltage: None,
                },
                Net {
                    id: NetId(1),
                    name: "mid".into(),
                    endpoints: vec![ep(1, 1), ep(2, 0)],
                    netclass: None,
                    voltage: None,
                },
                Net {
                    id: NetId(2),
                    name: "gnd".into(),
                    endpoints: vec![ep(0, 1), ep(2, 1)],
                    netclass: None,
                    voltage: None,
                },
            ],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn decoupling_test_board(cap_value: Option<&str>) -> Board {
        use synth_diagnostics::Span;
        use synth_ir::{Net, NetEndpoint, NetId};
        use synth_registry::RequiredDecoupling;
        let mut reg = part(
            "reg",
            "regulator",
            vec![
                pin("vin", ElectricalType::PowerInput),
                pin("gnd", ElectricalType::PowerInput),
                pin("vout", ElectricalType::PowerOutput),
            ],
        );
        reg.required_decoupling = vec![RequiredDecoupling {
            net: "vin".into(),
            value: "10u".into(),
            count: 1,
            max_distance_mm: None,
        }];
        let cap = part(
            "c",
            "capacitor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("p2", ElectricalType::Passive),
            ],
        );
        let ep = |c: u32, p: u32| NetEndpoint {
            component: ComponentId(c),
            pin: PinId(p),
            source_span: Span::new(0, 0),
        };
        Board {
            name: "b".into(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: vec![
                Component {
                    id: ComponentId(0),
                    refdes: "U1".into(),
                    kind: "regulator".into(),
                    part: Some(reg),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
                Component {
                    id: ComponentId(1),
                    refdes: "C1".into(),
                    kind: "capacitor".into(),
                    part: Some(cap),
                    value: cap_value.map(str::to_string),
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                },
            ],
            nets: vec![
                Net {
                    id: NetId(0),
                    name: "vin_net".into(),
                    endpoints: vec![ep(0, 0), ep(1, 0)],
                    netclass: None,
                    voltage: None,
                },
                Net {
                    id: NetId(1),
                    name: "gnd".into(),
                    endpoints: vec![ep(0, 1), ep(1, 1)],
                    netclass: None,
                    voltage: None,
                },
            ],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn identity_test_part(id: &str, mpn: Option<&str>, lcsc: Option<&str>) -> synth_registry::Part {
        use synth_registry::{Lifecycle, Part, PartId};
        Part {
            id: PartId(id.into()),
            kind: "sensor".into(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::default(),
            signed_by: vec![],
            substitutes: vec![],
            mpn: mpn.map(str::to_string),
            lcsc_pn: lcsc.map(str::to_string),
            pins: vec![],
            required_decoupling: vec![],
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }

    fn identity_test_board(parts: Vec<(synth_registry::Part, &str)>) -> Board {
        use synth_diagnostics::Span;
        Board {
            name: "b".into(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: parts
                .into_iter()
                .enumerate()
                .map(|(i, (part, refdes))| Component {
                    id: ComponentId(i as u32),
                    refdes: refdes.into(),
                    kind: part.kind.clone(),
                    part: Some(part),
                    value: None,
                    dnp: false,
                    properties: std::collections::BTreeMap::new(),
                    placement_hint: None,
                    group: None,
                    sheet: None,
                    source_span: Span::new(0, 0),
                })
                .collect(),
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn degenerate_divider_ratio_warns() {
        // mid at ~0.1% of rail — almost certainly swapped or mistyped.
        let board = divider_test_board(Some("1M"), Some("1k"));
        let diags = DividerRatioRule.check(&board, "test.synth");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, "W-SYNTH-DIVIDER-001");
    }

    #[test]
    fn balanced_divider_is_clean() {
        let board = divider_test_board(Some("10k"), Some("10k"));
        let diags = DividerRatioRule.check(&board, "test.synth");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn undervalued_bulk_cap_warns_power_006() {
        let board = decoupling_test_board(Some("100n"));
        let diags = DecouplingValueRule.check(&board, "test.synth");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, "E-SYNTH-POWER-006");
    }

    #[test]
    fn sufficient_bulk_cap_is_clean() {
        let board = decoupling_test_board(Some("22u"));
        let diags = DecouplingValueRule.check(&board, "test.synth");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn part_without_identity_warns_supply_002() {
        let board = identity_test_board(vec![(identity_test_part("bme680_env", None, None), "U3")]);
        let diags = SourcingIdentityRule.check(&board, "test.synth");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, "W-SYNTH-SUPPLY-002");
    }

    #[test]
    fn part_with_mpn_or_lcsc_is_clean() {
        let board = identity_test_board(vec![
            (
                identity_test_part("stm32f103c8", Some("STM32F103C8T6"), Some("C8734")),
                "U2",
            ),
            (
                identity_test_part("ams1117_3v3", Some("AMS1117-3.3"), None),
                "U1",
            ),
        ]);
        let diags = SourcingIdentityRule.check(&board, "test.synth");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unparseable_divider_value_skips_check() {
        let board = divider_test_board(Some("10k"), None);
        let diags = DividerRatioRule.check(&board, "test.synth");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unparseable_cap_value_skips_power_006() {
        for cap_value in [None, Some("4k7")] {
            let board = decoupling_test_board(cap_value);
            let diags = DecouplingValueRule.check(&board, "test.synth");
            assert!(diags.is_empty(), "{cap_value:?}: {diags:?}");
        }
    }

    #[test]
    fn generic_passives_are_exempt_from_identity() {
        let board = identity_test_board(vec![
            (identity_test_part("r_generic_0603", None, None), "R1"),
            (identity_test_part("c_generic_0805", None, None), "C1"),
        ]);
        let diags = SourcingIdentityRule.check(&board, "test.synth");
        assert!(diags.is_empty(), "{diags:?}");
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
            company: None,
            components: vec![Component {
                id: ComponentId(0),
                refdes: "U1".into(),
                kind: "ic".into(),
                part: Some(part),
                value: None,
                dnp: false,
                properties: std::collections::BTreeMap::new(),
                placement_hint: None,
                group: None,
                sheet: None,
                source_span: Span::new(0, 0),
            }],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        };

        let diags = run_erc(&board, "test.synth");
        let unverified = diags
            .iter()
            .filter(|d| d.code == "W-SYNTH-PART-UNVERIFIED")
            .count();
        assert_eq!(unverified, 1, "exactly one unverified-part warning");
    }

    #[test]
    fn grouped_pin_mentions_group_in_message() {
        use synth_diagnostics::Span;
        use synth_ir::Component;
        use synth_registry::{ElectricalType, Pin};

        let part = part(
            "u",
            "mcu",
            vec![Pin {
                name: "VDD".into(),
                number: PinNumber("1".into()),
                electrical_type: ElectricalType::PowerInput,
                capabilities: vec![],
                required: true,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            }],
        );
        let board = Board {
            name: "b".into(),
            layers: 2,
            manufacturer: None,
            company: None,
            revision: None,
            components: vec![Component {
                id: ComponentId(0),
                refdes: "U1".into(),
                kind: "mcu".into(),
                part: Some(part),
                value: None,
                dnp: false,
                properties: std::collections::BTreeMap::new(),
                placement_hint: None,
                group: Some("Power".into()),
                sheet: None,
                source_span: Span::new(0, 0),
            }],
            nets: vec![],
            diff_pairs: vec![],
            notes: vec![],
            keepouts: vec![],
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        };
        let diags = run_erc(&board, "test.synth");
        let diag = diags
            .iter()
            .find(|d| d.code == "E-SYNTH-CONNECT-001")
            .expect("floating required pin must error");
        for field in [&diag.expected, &diag.found] {
            let text = field.as_deref().unwrap_or("");
            assert!(
                text.contains("(group \"Power\")"),
                "group must appear in the message, got: {text}"
            );
        }
    }
}
