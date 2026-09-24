// SPDX-License-Identifier: Apache-2.0

//! Deeper ERC (Phase 6): the configurable pin-type conflict table, the
//! voltage-domain checks that named rails make possible, protection
//! checks on external connectors, and the naming/label hygiene rules.
//!
//! These complement the rules in `lib.rs` rather than replacing them:
//! where a dedicated rule already owns a case with a better message
//! and a patch (power-output shorts, output collisions, no-connect
//! misuse), the generic table and these rules stay silent — see
//! [`PinConflictTable::owned_by_dedicated_rule`] and the per-rule docs.
//!
//! Every quantity used here comes from the registry (pin electrical
//! types and `voltage_*`/`operating_conditions`), from a declared rail
//! voltage (`power "+3V3" 3.3v`), or from a component `value`
//! (`parse_resistance`); nothing is invented. Where the data is absent
//! the rule declines to fire rather than guessing.

use std::collections::{BTreeMap, BTreeSet};

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::{Board, ComponentId, NetId, PinId};
use synth_registry::{ElectricalType, Part, PinCapability};

use crate::config::{ErcConfig, PinConflictTable};
use crate::{endpoint_has_any_capability, ErcCategory, ErcRule};

// -----------------------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------------------

/// Voltage domain map for the board, computed once per `check`.
type Domains = synth_ir::PowerDomainMap;

fn domains(board: &Board) -> Domains {
    synth_ir::infer_power_domains(board)
}

/// Nominal voltage of a net, from the inferred power domains.
fn net_voltage(domains: &Domains, net: NetId) -> Option<f64> {
    domains
        .get(net)
        .and_then(synth_ir::PowerDomainKind::nominal_voltage)
}

fn is_ground_net(domains: &Domains, net: &synth_ir::Net) -> bool {
    if domains
        .get(net.id)
        .is_some_and(synth_ir::PowerDomainKind::is_ground)
    {
        return true;
    }
    let lower = net.name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "gnda" | "agnd" | "dgnd" | "ground" | "0v"
    ) || lower.starts_with("gnd")
        || lower.starts_with("vss")
}

fn is_rail_net(domains: &Domains, net: NetId) -> bool {
    domains
        .get(net)
        .is_some_and(synth_ir::PowerDomainKind::is_rail)
}

/// The single net a `(component, pin)` sits on, if any.
fn pin_net(board: &Board, component: ComponentId, pin: PinId) -> Option<NetId> {
    board
        .nets_containing(component, pin)
        .next()
        .map(|(id, _)| id)
}

/// A component's positive supply voltage: the voltage of the net feeding
/// its first non-ground power-input pin. `None` when the rail's voltage
/// is unknown or the part has no power-input pin.
fn component_supply_v(
    board: &Board,
    domains: &Domains,
    component: &synth_ir::Component,
) -> Option<f64> {
    let part = component.part.as_ref()?;
    for (idx, pin) in part.pins.iter().enumerate() {
        if !matches!(pin.electrical_type, ElectricalType::PowerInput) {
            continue;
        }
        let lower = pin.name.to_ascii_lowercase();
        if matches!(lower.as_str(), "gnd" | "vss" | "vssa" | "ground")
            || lower.starts_with("gnd")
            || lower.starts_with("vss")
        {
            continue;
        }
        let net = pin_net(board, component.id, PinId(idx as u32))?;
        if let Some(v) = net_voltage(domains, net) {
            return Some(v);
        }
    }
    None
}

/// The other pin's net on a 2-pin part (given one of its pins).
fn other_pin_net(board: &Board, component: &synth_ir::Component, from: PinId) -> Option<NetId> {
    let part = component.part.as_ref()?;
    if part.pins.len() != 2 {
        return None;
    }
    let other = PinId(u32::from(from.0 == 0));
    pin_net(board, component.id, other)
}

/// A 2-pin component of `kind` bridging `net` to a known-voltage rail.
/// Returns `(resistor_component_id, rail_voltage)`.
fn pullup_on_net<'a>(
    board: &'a Board,
    domains: &Domains,
    net: NetId,
    kind: &str,
) -> Option<(&'a synth_ir::Component, f64)> {
    let target = board.net(net)?;
    for ep in &target.endpoints {
        let Some(component) = board.component(ep.component) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if !part.kind.eq_ignore_ascii_case(kind) {
            continue;
        }
        let Some(other) = other_pin_net(board, component, ep.pin) else {
            continue;
        };
        if other == net {
            continue;
        }
        if is_rail_net(domains, other) {
            if let Some(v) = net_voltage(domains, other) {
                return Some((component, v));
            }
        }
    }
    None
}

/// True when any component on `net` belongs to a protection family —
/// a TVS/ESD diode, a fused or reverse-polarity protected path.
fn net_has_protection(board: &Board, net: NetId) -> bool {
    let Some(target) = board.net(net) else {
        return false;
    };
    target.endpoints.iter().any(|ep| {
        let Some(component) = board.component(ep.component) else {
            return false;
        };
        let Some(part) = component.part.as_ref() else {
            return false;
        };
        let hay = format!(
            "{} {} {}",
            part.id.as_str(),
            part.kind,
            part.description.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase();
        part.kind.eq_ignore_ascii_case("protection")
            || hay.contains("esd")
            || hay.contains("tvs")
            || hay.contains("varistor")
            || hay.contains("fuse")
            || hay.contains("reverse")
            || hay.contains("schottky")
            || hay.contains("ideal diode")
    })
}

/// The three prose fields of a diagnostic, grouped so [`net_diag`]
/// stays within the argument budget.
struct DiagText {
    title: &'static str,
    message: String,
    expected: String,
    found: String,
}

/// Emit a diagnostic anchored to a net's first endpoint.
fn net_diag(
    code: &str,
    severity: Severity,
    board: &Board,
    net: &synth_ir::Net,
    file: &str,
    text: DiagText,
) -> Diagnostic {
    let span = net
        .endpoints
        .first()
        .map_or(synth_diagnostics::Span::new(0, 0), |e| e.source_span);
    let mut b = DiagnosticBuilder::new(code, severity, text.title)
        .location(Location::from_span(file.to_string(), span))
        .expected(text.expected)
        .found(text.found)
        .message(text.message)
        .explanation_url(format!("synth.docs/diagnostics/{code}"));
    if let Some(component) = net
        .endpoints
        .first()
        .and_then(|e| board.component(e.component))
    {
        b = b.entity(synth_diagnostics::EntityRef::Component {
            id: component.refdes.clone(),
        });
    }
    b.build()
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-007 — pin-type conflict table
// -----------------------------------------------------------------------------

/// Configurable pin-to-pin conflict table, modelled on KiCad's ERC
/// matrix. Fires once per (net, type-pair) so a multi-drop bus does not
/// repeat the same finding per endpoint.
pub(crate) struct PinConflictRule {
    table: PinConflictTable,
}

impl PinConflictRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            table: config.pin_conflicts.clone(),
        }
    }
}

impl ErcRule for PinConflictRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-007"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
            let pins: Vec<(usize, ElectricalType, String, synth_diagnostics::Span)> = net
                .endpoints
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    let component = board.component(e.component)?;
                    let pin = board.pin(e.component, e.pin)?;
                    Some((
                        i,
                        pin.electrical_type,
                        component.describe_pin(&pin.name),
                        e.source_span,
                    ))
                })
                .collect();
            for (i, (_, ta, na, _)) in pins.iter().enumerate() {
                for (j, (_, tb, nb, span)) in pins.iter().enumerate() {
                    if j <= i {
                        continue;
                    }
                    if PinConflictTable::owned_by_dedicated_rule(*ta, *tb) {
                        continue;
                    }
                    let Some(severity) = self.table.severity(*ta, *tb) else {
                        continue;
                    };
                    // One finding per unordered type pair per net.
                    let mut key = (
                        crate::config::type_name(*ta).to_string(),
                        crate::config::type_name(*tb).to_string(),
                    );
                    if key.0 > key.1 {
                        key = (key.1, key.0);
                    }
                    if !seen.insert(key) {
                        continue;
                    }
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            severity,
                            "conflicting pin types on one net",
                        )
                        .location(Location::from_span(file.to_string(), *span))
                        .expected(format!(
                            "net `{}` to carry at most one driver of each type",
                            net.name
                        ))
                        .found(format!(
                            "net `{}` joins {na} ({}) and {nb} ({}), a `{}`/`{}` conflict",
                            net.name,
                            crate::config::type_name(*ta),
                            crate::config::type_name(*tb),
                            crate::config::type_name(*ta),
                            crate::config::type_name(*tb),
                        ))
                        .message(format!(
                            "`{}` and `{}` are both driving net `{}`; disable one driver or insert \
                             isolation (series resistor, buffer). Configure this pair in \
                             `[pin_conflicts]` to change the severity.",
                            crate::config::type_name(*ta),
                            crate::config::type_name(*tb),
                            net.name
                        ))
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
// E-SYNTH-POWER-008 — pull-up rail above a device's operating voltage
// -----------------------------------------------------------------------------

/// A pull-up to a rail above the bus device's own VDD overstresses every
/// input on that net. `E-SYNTH-POWER-005` catches a rail above a pin's
/// *absolute maximum*; this catches the subtler case where the pull-up
/// merely exceeds the device's *nominal* supply, which is the usual
/// 5V-pull-up-on-a-3V3-bus mistake.
pub(crate) struct PullupRailMismatchRule {
    margin_v: f64,
}

impl PullupRailMismatchRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            margin_v: config.pullup_margin_v,
        }
    }
}

impl ErcRule for PullupRailMismatchRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-008"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for net in &board.nets {
            let Some((resistor, rail_v)) = pullup_on_net(board, &domains, net.id, "resistor")
            else {
                continue;
            };
            // Victim: a device pin on this net whose own supply we know.
            for ep in &net.endpoints {
                let Some(component) = board.component(ep.component) else {
                    continue;
                };
                if component.id == resistor.id {
                    continue;
                }
                let Some(pin) = board.pin(ep.component, ep.pin) else {
                    continue;
                };
                if !matches!(
                    pin.electrical_type,
                    ElectricalType::Input
                        | ElectricalType::Bidirectional
                        | ElectricalType::OpenDrainLow
                        | ElectricalType::OpenDrainHigh
                ) {
                    continue;
                }
                let Some(device_v) = component_supply_v(board, &domains, component) else {
                    continue;
                };
                if rail_v > device_v + self.margin_v {
                    out.push(
                        DiagnosticBuilder::new(
                            self.code(),
                            Severity::Error,
                            "pull-up rail exceeds the bus device's supply",
                        )
                        .location(Location::from_span(file.to_string(), ep.source_span))
                        .expected(format!(
                            "a pull-up on net `{}` at or below {device_v:.1}V (the supply of {})",
                            net.name,
                            component.describe()
                        ))
                        .found(format!(
                            "pull-up {} to a {rail_v:.1}V rail on net `{}`, which carries {} \
                             running at {device_v:.1}V",
                            resistor.describe(),
                            net.name,
                            component.describe_pin(&pin.name),
                        ))
                        .message(format!(
                            "pull net `{}` up to {device_v:.1}V instead, or level-shift it; a \
                             {rail_v:.1}V pull-up will forward-bias the input protection of {}",
                            net.name,
                            component.describe()
                        ))
                        .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                        .build(),
                    );
                    break; // one finding per net
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-009 — regulator input outside its operating range
// -----------------------------------------------------------------------------

/// A regulator fed from a rail outside the input range its registry entry
/// declares (`operating_conditions.min_voltage_v`/`max_voltage_v`).
pub(crate) struct RegulatorInputRangeRule;

impl ErcRule for RegulatorInputRangeRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-009"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let Some(conditions) = part.operating_conditions.as_ref() else {
                continue;
            };
            // Has an output pin but no input range to check? skip.
            let has_output = part
                .pins
                .iter()
                .any(|p| p.electrical_type == ElectricalType::PowerOutput);
            if !has_output {
                continue;
            }
            // Input pin: the first non-ground power-input.
            let input = part.pins.iter().enumerate().find(|(_, p)| {
                p.electrical_type == ElectricalType::PowerInput
                    && !matches!(
                        p.name.to_ascii_lowercase().as_str(),
                        "gnd" | "vss" | "vssa" | "ground"
                    )
            });
            let Some((idx, pin)) = input else {
                continue;
            };
            let Some(net) = pin_net(board, component.id, PinId(idx as u32)) else {
                continue;
            };
            let Some(v_in) = net_voltage(&domains, net) else {
                continue;
            };
            let too_low = conditions
                .min_voltage_v
                .is_some_and(|min| v_in < min - 1e-6);
            let too_high = conditions
                .max_voltage_v
                .is_some_and(|max| v_in > max + 1e-6);
            if !(too_low || too_high) {
                continue;
            }
            let range = format!(
                "{}–{}V",
                conditions
                    .min_voltage_v
                    .map_or("?".to_string(), |v| format!("{v:.1}")),
                conditions
                    .max_voltage_v
                    .map_or("?".to_string(), |v| format!("{v:.1}"))
            );
            let direction = if too_low { "below" } else { "above" };
            let mut b = DiagnosticBuilder::new(
                self.code(),
                Severity::Error,
                "regulator input outside its operating range",
            )
            .location(Location::from_span(file.to_string(), component.source_span))
            .expected(format!(
                "the input rail of {} to sit inside its declared range {range}",
                component.describe()
            ))
            .found(format!(
                "net `{}` supplies {v_in:.1}V to {}, whose input range is {range}",
                board.net(net).map_or("?", |n| n.name.as_str()),
                component.describe_pin(&pin.name),
            ))
            .message(format!(
                "the rail feeding {} is {v_in:.1}V, {direction} the {range} its datasheet allows; \
                 change the source rail or pick a regulator rated for {v_in:.1}V",
                component.describe()
            ))
            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()));
            b = b.entity(synth_diagnostics::EntityRef::Component {
                id: component.refdes.clone(),
            });
            out.push(b.build());
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-POWER-010 — load current above the regulator's maximum
// -----------------------------------------------------------------------------

/// Sums the declared `max_current_ma` of every load on a regulator's
/// output rail and compares it with the regulator's own maximum.
pub(crate) struct PowerBudgetRule {
    headroom_pct: f64,
}

impl PowerBudgetRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            headroom_pct: config.power_budget_headroom_pct,
        }
    }
}

impl ErcRule for PowerBudgetRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-POWER-010"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for regulator in &board.components {
            let Some(part) = regulator.part.as_ref() else {
                continue;
            };
            let Some(max_ma) = part
                .operating_conditions
                .as_ref()
                .and_then(|c| c.max_current_ma)
            else {
                continue;
            };
            let Some(out_idx) = part
                .pins
                .iter()
                .position(|p| p.electrical_type == ElectricalType::PowerOutput)
            else {
                continue;
            };
            let Some(rail) = pin_net(board, regulator.id, PinId(out_idx as u32)) else {
                continue;
            };
            let Some(net) = board.net(rail) else {
                continue;
            };
            // Sum declared draw over the *other* components on the rail.
            let mut total_ma = 0.0_f64;
            let mut counted: BTreeSet<ComponentId> = BTreeSet::new();
            let mut loads: Vec<(String, f64)> = Vec::new();
            for ep in &net.endpoints {
                if ep.component == regulator.id || !counted.insert(ep.component) {
                    continue;
                }
                let Some(load) = board.component(ep.component) else {
                    continue;
                };
                let Some(draw) = load
                    .part
                    .as_ref()
                    .and_then(|p| p.operating_conditions.as_ref())
                    .and_then(|c| c.max_current_ma)
                else {
                    continue;
                };
                total_ma += draw;
                loads.push((load.refdes.clone(), draw));
            }
            if loads.is_empty() {
                continue;
            }
            let budget = max_ma * (1.0 - self.headroom_pct / 100.0);
            if total_ma <= budget + 1e-6 {
                continue;
            }
            loads.sort_by(|a, b| a.0.cmp(&b.0));
            let breakdown: Vec<String> = loads
                .iter()
                .map(|(refdes, ma)| format!("{refdes} {ma:.0}mA"))
                .collect();
            let mut b = DiagnosticBuilder::new(
                self.code(),
                Severity::Error,
                "rail load exceeds the regulator's current limit",
            )
            .location(Location::from_span(file.to_string(), regulator.source_span))
            .expected(format!(
                "the load on `{}` to stay at or below {max_ma:.0}mA ({}'s maximum)",
                net.name,
                regulator.describe()
            ))
            .found(format!(
                "{total_ma:.0}mA drawn on `{}` by {} load(s): {}",
                net.name,
                loads.len(),
                breakdown.join(", ")
            ))
            .message(format!(
                "the loads on `{}` total {total_ma:.0}mA against a {max_ma:.0}mA regulator; add \
                 headroom, split the rail, or choose a higher-current regulator",
                net.name
            ))
            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
            .smt_constraint(format!("(assert (<= rail_current_ma {max_ma:.0}))"));
            b = b.entity(synth_diagnostics::EntityRef::Component {
                id: regulator.refdes.clone(),
            });
            out.push(b.build());
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CONNECT-008 — floating digital input on a used part
// -----------------------------------------------------------------------------

/// An unconnected `input` pin on a part that is otherwise in use. A
/// floating CMOS input sits at an undefined level and draws crowbar
/// current, so it must be tied off or explicitly no-connect. Only
/// non-`required` pins reach here, so `E-SYNTH-CONNECT-001` still owns
/// the required-power/control cases.
pub(crate) struct FloatingCmosInputRule;

/// Component kinds whose unconnected inputs are a real hazard (as opposed
/// to a connector pin or a passive terminal, which floats harmlessly).
const ACTIVE_KINDS: [&str; 11] = [
    "mcu",
    "ic",
    "logic",
    "gate",
    "opamp",
    "comparator",
    "buffer",
    "inverter",
    "flip_flop",
    "memory",
    "regulator",
];

impl ErcRule for FloatingCmosInputRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-008"
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
            if !ACTIVE_KINDS
                .iter()
                .any(|k| part.kind.eq_ignore_ascii_case(k))
            {
                continue;
            }
            // Only flag a part that is actually used: a wholly
            // unconnected part is `E-SYNTH-CONNECT-006`'s story.
            let used = board
                .nets
                .iter()
                .any(|net| net.endpoints.iter().any(|e| e.component == component.id));
            if !used {
                continue;
            }
            for (idx, pin) in part.pins.iter().enumerate() {
                if pin.electrical_type != ElectricalType::Input || pin.required {
                    continue;
                }
                let pid = PinId(idx as u32);
                if board.nets_containing(component.id, pid).next().is_some() {
                    continue;
                }
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "floating digital input",
                    )
                    .location(Location::from_span(file.to_string(), component.source_span))
                    .expected(format!(
                        "{} to be driven, tied to a rail, or marked no-connect",
                        component.describe_pin(&pin.name)
                    ))
                    .found(format!(
                        "input {} is unconnected on a part that is otherwise in use",
                        component.describe_pin(&pin.name)
                    ))
                    .message(format!(
                        "a floating CMOS input is undefined and draws crowbar current; tie {} to a \
                         rail through a resistor or add a no-connect marker",
                        component.describe_pin(&pin.name)
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
// E-SYNTH-CONNECT-009 — open-drain net with no pull-up
// -----------------------------------------------------------------------------

/// An open-drain / open-collector output needs a pull-up to reach a high
/// level. `E-SYNTH-I2C-002` owns the I²C case (its pull-up is a protocol
/// requirement); this covers every other open-drain bus.
pub(crate) struct OpenDrainPullupRule {
    severity: Severity,
}

impl OpenDrainPullupRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            severity: config.open_drain_no_pullup,
        }
    }
}

impl ErcRule for OpenDrainPullupRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CONNECT-009"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Connectivity
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for net in &board.nets {
            let open_drains: Vec<(&synth_ir::Component, &synth_registry::Pin)> = net
                .endpoints
                .iter()
                .filter_map(|e| {
                    let component = board.component(e.component)?;
                    let pin = board.pin(e.component, e.pin)?;
                    matches!(
                        pin.electrical_type,
                        ElectricalType::OpenDrainLow | ElectricalType::OpenDrainHigh
                    )
                    .then_some((component, pin))
                })
                .collect();
            if open_drains.is_empty() {
                continue;
            }
            // I²C is covered by E-SYNTH-I2C-002.
            let is_i2c = net.endpoints.iter().any(|e| {
                endpoint_has_any_capability(
                    board,
                    e.component,
                    e.pin,
                    &[
                        synth_registry::PinCapability::I2cSda,
                        synth_registry::PinCapability::I2cScl,
                    ],
                )
            });
            if is_i2c {
                continue;
            }
            // A pull-up anywhere on the net satisfies the requirement.
            let has_pullup = net.endpoints.iter().any(|e| {
                board
                    .component(e.component)
                    .and_then(|c| c.part.as_ref())
                    .is_some_and(|p| p.kind.eq_ignore_ascii_case("resistor"))
                    && other_pin_net(board, board.component(e.component).expect("checked"), e.pin)
                        .is_some_and(|other| is_rail_net(&domains, other))
            });
            if has_pullup {
                continue;
            }
            let (component, pin) = open_drains[0];
            out.push(
                DiagnosticBuilder::new(self.code(), self.severity, "open-drain net has no pull-up")
                    .location(Location::from_span(
                        file.to_string(),
                        net.endpoints
                            .first()
                            .map_or(synth_diagnostics::Span::new(0, 0), |e| e.source_span),
                    ))
                    .expected(format!(
                        "net `{}` to have a pull-up resistor to its logic rail",
                        net.name
                    ))
                    .found(format!(
                        "{} is open-drain on net `{}` and no pull-up resistor is present",
                        component.describe_pin(&pin.name),
                        net.name
                    ))
                    .message(format!(
                        "an open-drain output cannot drive high; add a pull-up on `{}` sized for the \
                         bus capacitance and clock rate",
                        net.name
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-ESD-001 — unprotected external connector signals
// -----------------------------------------------------------------------------

/// Signals leaving the board through a connector need ESD protection, and
/// a connector-fed rail needs reverse-polarity protection. "External" is
/// decided from the connector kind plus the registry capabilities
/// (USB / bus pins), so an internal board-to-board header is not forced
/// to carry TVS diodes.
pub(crate) struct ConnectorProtectionRule {
    enabled: bool,
}

impl ConnectorProtectionRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            enabled: config.require_connector_protection,
        }
    }
}

/// Capabilities that mark a *signal* pin as reachable from outside the
/// enclosure. `UsbVbus` is deliberately excluded — a connector-fed rail
/// is a reverse-polarity question, not an ESD one.
fn is_external_signal(caps: &[synth_registry::PinCapability]) -> bool {
    use synth_registry::PinCapability as C;
    caps.iter()
        .any(|c| matches!(c, C::UsbDp | C::UsbDn | C::UsbCc | C::UartTx | C::UartRx))
}

/// A connector that plausibly brings *power* in from outside (USB, a DC
/// jack, a battery lead). Used to decide whether a connector-fed rail
/// should carry reverse-polarity protection; a plain debug header is
/// deliberately not treated as a DC input.
fn is_power_connector(part: &Part) -> bool {
    let hay = format!(
        "{} {} {}",
        part.id.as_str(),
        part.kind,
        part.description.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase();
    let tagged = part.pins.iter().any(|p| {
        p.capabilities
            .contains(&synth_registry::PinCapability::UsbVbus)
    });
    tagged
        || [
            "usb", "vbus", "dc_jack", "barrel", "battery", "batt", "power_in", "vin",
        ]
        .iter()
        .any(|needle| hay.contains(needle))
}

impl ErcRule for ConnectorProtectionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-ESD-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        if !self.enabled {
            return Vec::new();
        }
        let domains = domains(board);
        let mut out = Vec::new();
        let mut checked: BTreeSet<(ComponentId, NetId)> = BTreeSet::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let kind = part.kind.to_ascii_lowercase();
            if !matches!(kind.as_str(), "connector" | "jack" | "receptacle") {
                continue;
            }
            for (idx, pin) in part.pins.iter().enumerate() {
                let pid = PinId(idx as u32);
                let Some(net) = pin_net(board, component.id, pid) else {
                    continue;
                };
                if !checked.insert((component.id, net)) {
                    continue;
                }
                let Some(target) = board.net(net) else {
                    continue;
                };
                // A ground net needs no protection device.
                if is_ground_net(&domains, target) {
                    continue;
                }
                // A supply feed: an inferred rail, or a connector pin
                // whose own type says it brings power in (a USB VBUS net
                // is not an inferred rail until something declares it).
                let supplies = is_rail_net(&domains, net)
                    || pin.electrical_type == ElectricalType::PowerInput
                    || pin
                        .capabilities
                        .contains(&synth_registry::PinCapability::UsbVbus);
                if supplies {
                    if !is_power_connector(part) || net_has_protection(board, net) {
                        continue;
                    }
                    out.push(net_diag(
                        self.code(),
                        Severity::Warning,
                        board,
                        target,
                        file,
                        DiagText {
                            title: "connector-fed rail has no reverse-polarity protection",
                            message: format!(
                                "rail `{}` is fed straight from {}; a reversed supply will \
                                 destroy every part on it",
                                target.name,
                                component.describe()
                            ),
                            expected: "a series diode, ideal-diode, or load switch between the \
                                       connector and the rail"
                                .to_string(),
                            found: format!(
                                "{} pin `{}` feeds rail `{}` with no protection device",
                                component.describe(),
                                pin.name,
                                target.name
                            ),
                        },
                    ));
                    continue;
                }
                // An externally-reachable signal net: the registry must
                // have tagged the pin as an interface pin, so a generic
                // header pin does not drag in a TVS requirement.
                if !is_external_signal(&pin.capabilities) {
                    continue;
                }
                if net_has_protection(board, net) {
                    continue;
                }
                out.push(net_diag(
                    self.code(),
                    Severity::Warning,
                    board,
                    target,
                    file,
                    DiagText {
                        title: "external connector signal has no ESD protection",
                        message: format!(
                            "net `{}` is reachable from {} and carries no TVS/ESD device",
                            target.name,
                            component.describe()
                        ),
                        expected: "an ESD/TVS array between the connector pin and ground"
                            .to_string(),
                        found: format!(
                            "{} pin `{}` reaches net `{}` with no protection device",
                            component.describe(),
                            pin.name,
                            target.name
                        ),
                    },
                ));
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-LED-001 — LED current and the series resistor
// -----------------------------------------------------------------------------

/// Checks each LED's series resistor: that one exists, and that the
/// resulting current and resistor dissipation are sane. `Vf` comes from
/// the part description/id (the registry carries it textually) and the
/// rail voltage from the power domain map; with either unknown the rule
/// declines to fire.
pub(crate) struct LedCurrentRule {
    max_current_ma: f64,
}

impl LedCurrentRule {
    #[must_use]
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            max_current_ma: config.led_max_current_ma,
        }
    }
}

/// All decimal numbers in `text`, in order. Deliberately tiny: LED
/// descriptions are short and a full float parser is not needed.
fn numbers_in(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() || c == '.' {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse() {
                out.push(v);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse() {
            out.push(v);
        }
    }
    out
}

/// A plausible forward voltage: numbers outside 0.5–10 V are package
/// sizes and model numbers (`0603`, `1117`), not a Vf.
fn plausible_vf(v: f64) -> bool {
    (0.5..10.0).contains(&v)
}

/// Forward voltage implied by an LED part, in volts. A number stated
/// next to `Vf` wins; otherwise the colour family decides. `None` when
/// neither is available, so the rule declines to check that part.
fn forward_voltage_v(part: &Part) -> Option<f64> {
    let hay = format!(
        "{} {}",
        part.id.as_str(),
        part.description.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase();
    // A stated Vf: the nearest plausible number to the `vf` token,
    // preferring the one before it ("~2.0V Vf").
    if let Some(idx) = hay.find("vf") {
        if let Some(v) = numbers_in(&hay[..idx])
            .into_iter()
            .rev()
            .find(|v| plausible_vf(*v))
        {
            return Some(v);
        }
        if let Some(v) = numbers_in(&hay[idx + 2..])
            .into_iter()
            .find(|v| plausible_vf(*v))
        {
            return Some(v);
        }
    }
    if hay.contains("infrared") || hay.contains("ir_led") {
        return Some(1.2);
    }
    if hay.contains("red") {
        return Some(1.9);
    }
    if hay.contains("yellow") || hay.contains("amber") {
        return Some(2.1);
    }
    if hay.contains("green") {
        return Some(2.2);
    }
    if hay.contains("blue") || hay.contains("white") {
        return Some(3.2);
    }
    None
}

/// The LED's series resistor: a resistor on either LED leg, with the
/// voltage the other leg is fed from. The feed is either a rail or the
/// supply of whatever drives the leg (a GPIO-driven LED is fed from its
/// MCU's rail). `None` when there is no resistor, or when the feed
/// voltage cannot be resolved.
fn led_series_resistor(
    board: &Board,
    domains: &Domains,
    part: &Part,
    led: ComponentId,
) -> Option<(Option<f64>, f64, String)> {
    for (i, _) in part.pins.iter().enumerate() {
        let Some(net) = pin_net(board, led, PinId(i as u32)) else {
            continue;
        };
        let Some(target) = board.net(net) else {
            continue;
        };
        for ep in &target.endpoints {
            let Some(resistor) = board.component(ep.component) else {
                continue;
            };
            let Some(rpart) = resistor.part.as_ref() else {
                continue;
            };
            if !rpart.kind.eq_ignore_ascii_case("resistor") {
                continue;
            }
            let Some(r) = resistor
                .value
                .as_deref()
                .and_then(crate::value::parse_resistance)
            else {
                continue;
            };
            if r <= 0.0 {
                continue;
            }
            let Some(far) = other_pin_net(board, resistor, ep.pin) else {
                continue;
            };
            // The feed: a rail directly, or the supply of the driver.
            let feed_v = net_voltage(domains, far).or_else(|| {
                board.net(far).and_then(|far_net| {
                    far_net.endpoints.iter().find_map(|e| {
                        board
                            .component(e.component)
                            .and_then(|c| component_supply_v(board, domains, c))
                    })
                })
            });
            return Some((feed_v, r, resistor.refdes.clone()));
        }
    }
    None
}

/// True when the LED has *no* series resistor on either leg.
fn led_has_no_series_resistor(board: &Board, part: &Part, led: ComponentId) -> bool {
    for (i, _) in part.pins.iter().enumerate() {
        let Some(net) = pin_net(board, led, PinId(i as u32)) else {
            continue;
        };
        if board.net(net).is_some_and(|target| {
            target.endpoints.iter().any(|e| {
                board
                    .component(e.component)
                    .and_then(|c| c.part.as_ref())
                    .is_some_and(|p| p.kind.eq_ignore_ascii_case("resistor"))
            })
        }) {
            return false;
        }
    }
    true
}

impl ErcRule for LedCurrentRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-LED-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for led in &board.components {
            let Some(part) = led.part.as_ref() else {
                continue;
            };
            if !part.kind.eq_ignore_ascii_case("led") {
                continue;
            }
            let Some(vf) = forward_voltage_v(part) else {
                continue;
            };
            if led_has_no_series_resistor(board, part, led.id) {
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Warning,
                        "LED without a series resistor",
                    )
                    .location(Location::from_span(file.to_string(), led.source_span))
                    .expected(format!(
                        "{} to connect in series with a current-limiting resistor",
                        led.describe()
                    ))
                    .found("no series resistor found on either LED leg".to_string())
                    .message(format!(
                        "{} has no series resistor; an LED driven without one will either not \
                         light or burn out",
                        led.describe()
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
                continue;
            }
            let Some((feed_v, r_ohms, resistor_refdes)) =
                led_series_resistor(board, &domains, part, led.id)
            else {
                continue;
            };
            // Without a resolvable feed voltage the current cannot be
            // judged — an unterminated value is not evidence of a fault.
            let Some(feed_v) = feed_v else {
                continue;
            };
            let i_ma = (feed_v - vf) / r_ohms * 1000.0;
            if i_ma <= 0.0 {
                continue;
            }
            out.extend(self.judge_current(&LedReading {
                led,
                file,
                feed_v,
                vf,
                r_ohms,
                resistor_refdes: &resistor_refdes,
                i_ma,
            }));
        }
        out
    }
}

/// One LED's measured operating point, for [`LedCurrentRule::judge_current`].
#[derive(Clone, Copy)]
struct LedReading<'a> {
    led: &'a synth_ir::Component,
    file: &'a str,
    feed_v: f64,
    vf: f64,
    r_ohms: f64,
    resistor_refdes: &'a str,
    i_ma: f64,
}

impl LedCurrentRule {
    /// The over-current (error) and over-dissipation (warning) findings
    /// for one LED, if either applies.
    fn judge_current(&self, reading: &LedReading<'_>) -> Vec<Diagnostic> {
        let LedReading {
            led,
            file,
            feed_v,
            vf,
            r_ohms,
            resistor_refdes,
            i_ma,
        } = *reading;
        let mut out = Vec::new();
        if i_ma > self.max_current_ma + 1e-6 {
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "LED current above the part's maximum",
                )
                .location(Location::from_span(file.to_string(), led.source_span))
                .expected(format!(
                    "{resistor_refdes} to limit {} to at most {:.0}mA",
                    led.describe(),
                    self.max_current_ma
                ))
                .found(format!(
                    "{i_ma:.1}mA through {} via {resistor_refdes}: ({feed_v:.1}V − {vf:.1}V Vf) / {}",
                    led.describe(),
                    format_resistance(r_ohms)
                ))
                .message(format!(
                    "raise the series resistor so the current stays under {:.0}mA, or the LED will                      be over-driven",
                    self.max_current_ma
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .smt_constraint(format!("(assert (<= led_current_ma {i_ma:.1}))"))
                .build(),
            );
            return out;
        }
        // Resistor dissipation: P = I²R. 0603 is good for ~0.1W.
        let p_w = (i_ma / 1000.0).powi(2) * r_ohms;
        if p_w > 0.1 {
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "LED series resistor dissipation above 0.1W",
                )
                .location(Location::from_span(file.to_string(), led.source_span))
                .expected(format!("{resistor_refdes} to dissipate at most 0.1W (0603)"))
                .found(format!(
                    "{resistor_refdes} dissipates {p_w:.3}W at {i_ma:.1}mA"
                ))
                .message(
                    "use a larger package or a higher resistance; a 0603 resistor is rated around                      0.1W"
                        .to_string(),
                )
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

fn format_resistance(ohms: f64) -> String {
    if ohms >= 1_000_000.0 {
        format!("{:.1}MΩ", ohms / 1_000_000.0)
    } else if ohms >= 1000.0 {
        format!("{:.1}kΩ", ohms / 1000.0)
    } else {
        format!("{ohms:.0}Ω")
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-007 — net names differing only by case
// -----------------------------------------------------------------------------

/// Two distinct nets whose names differ only by case. KiCad's net
/// resolution and most fab netlists fold case, so `SDA` and `Sda` can
/// silently merge (or, worse, one is simply lost) downstream.
pub(crate) struct CaseOnlyNetCollisionRule;

impl ErcRule for CaseOnlyNetCollisionRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-007"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut seen: BTreeMap<String, (&str, NetId)> = BTreeMap::new();
        for net in &board.nets {
            // Auto-generated names can't collide meaningfully.
            if net.name.starts_with("net_") {
                continue;
            }
            let key = net.name.to_ascii_lowercase();
            match seen.get(&key) {
                Some((first_name, first_id)) => out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "net names differ only by case",
                    )
                    .location(Location::from_span(
                        file.to_string(),
                        net.endpoints
                            .first()
                            .map_or(synth_diagnostics::Span::new(0, 0), |e| e.source_span),
                    ))
                    .expected(format!(
                        "net `{first_name}` and `{}` to use distinct spellings",
                        net.name
                    ))
                    .found(format!(
                        "nets `{first_name}` (id {}) and `{}` (id {}) collide case-insensitively",
                        first_id.0, net.name, net.id.0
                    ))
                    .message(format!(
                        "`{first_name}` and `{}` are different nets that many tools treat as the \
                         same name; rename one so the distinction survives",
                        net.name
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                ),
                None => {
                    seen.insert(key, (net.name.as_str(), net.id));
                }
            }
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-008 — declared label used only once
// -----------------------------------------------------------------------------

/// A user-declared net name that ends up on fewer than two endpoints: the
/// label joins nothing. `E-SYNTH-CONNECT-002` reports single-endpoint
/// nets in general; this rule owns the *declared-name* case so exactly
/// one rule fires for it (see `SingleEndpointNetRule`, which skips
/// declared names).
pub(crate) struct SingleUseLabelRule;

impl ErcRule for SingleUseLabelRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-008"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            if net.name.starts_with("net_") || net.endpoints.len() >= 2 {
                continue;
            }
            let uses = net.endpoints.len();
            let span = net
                .endpoints
                .first()
                .map_or(synth_diagnostics::Span::new(0, 0), |e| e.source_span);
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "declared net label used only once",
                )
                .location(Location::from_span(file.to_string(), span))
                .expected(format!(
                    "net `{}` to be referenced by at least two endpoints",
                    net.name
                ))
                .found(format!(
                    "declared net `{}` is used by {uses} endpoint(s)",
                    net.name
                ))
                .message(format!(
                    "the label `{}` connects nothing; either wire a second endpoint to it or drop \
                     the declaration",
                    net.name
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-NAME-009 — ground pin not on a ground net
// -----------------------------------------------------------------------------

/// A pin the part declares as a ground reference (or that is named
/// `gnd`/`vss`) sitting on a net that is not a ground: the classic
/// mis-wired ground, or a ground net that got renamed into a signal.
pub(crate) struct GroundPinOffGroundNetRule;

fn looks_like_ground_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "gnda" | "agnd" | "dgnd" | "ground" | "0v" | "vee"
    ) || lower.starts_with("gnd")
        || lower.starts_with("vss")
}

impl ErcRule for GroundPinOffGroundNetRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-009"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for net in &board.nets {
            if is_ground_net(&domains, net) {
                continue;
            }
            // A net that only *looks* unclassified is not evidence of a
            // mis-wire: an auto-named net joining a `gnd` pin to a
            // bypass cap is an ordinary ground. Only fire when the net
            // is positively something else — an inferred rail, a
            // declared non-ground name, or a net carrying a supply
            // output.
            let gnd_named_pins = net
                .endpoints
                .iter()
                .filter(|e| {
                    board
                        .pin(e.component, e.pin)
                        .is_some_and(|p| looks_like_ground_name(&p.name))
                })
                .count();
            let declared_name = !net.name.starts_with("net_");
            let positively_non_ground = is_rail_net(&domains, net.id)
                || crate::net_has_power_output(board, net)
                || declared_name;
            if gnd_named_pins >= 2 || !positively_non_ground {
                continue;
            }
            for ep in &net.endpoints {
                let Some(component) = board.component(ep.component) else {
                    continue;
                };
                let Some(pin) = board.pin(ep.component, ep.pin) else {
                    continue;
                };
                let declared_ground = pin.electrical_type == ElectricalType::GroundReference
                    || looks_like_ground_name(&pin.name);
                if !declared_ground {
                    continue;
                }
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "ground pin on a non-ground net",
                    )
                    .location(Location::from_span(file.to_string(), ep.source_span))
                    .expected("the pin to sit on a ground net (`GND`, `VSS`, …)".to_string())
                    .found(format!(
                        "{} is a ground pin on net `{}`",
                        component.describe_pin(&pin.name),
                        net.name
                    ))
                    .message(format!(
                        "{} is tied to `{}`, which is not a ground net; check for a mis-wire or a \
                         ground net that was renamed",
                        component.describe_pin(&pin.name),
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
// E-SYNTH-NAME-010 — units of one part on different rails
// -----------------------------------------------------------------------------

/// A multi-unit part (a dual op-amp, a quad gate, …) whose units are
/// powered from different positive rails. Each unit is elsewhere in the
/// schematic, so the mistake is invisible locally; the regulator/tie
/// must be consistent across the whole package.
pub(crate) struct MultiUnitRailSplitRule;

impl ErcRule for MultiUnitRailSplitRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-NAME-010"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Naming
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            // Group positive power pins by their unit tag.
            let mut by_unit: BTreeMap<String, BTreeSet<NetId>> = BTreeMap::new();
            for (idx, pin) in part.pins.iter().enumerate() {
                if pin.electrical_type != ElectricalType::PowerInput
                    || looks_like_ground_name(&pin.name)
                {
                    continue;
                }
                let unit = pin.unit.clone().unwrap_or_else(|| "1".to_string());
                if let Some(net) = pin_net(board, component.id, PinId(idx as u32)) {
                    by_unit.entry(unit).or_default().insert(net);
                }
            }
            if by_unit.len() < 2 {
                continue;
            }
            // Collect each unit's rail voltage(s).
            let mut unit_rails: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for (unit, nets) in &by_unit {
                for net in nets {
                    let label = board
                        .net(*net)
                        .map_or_else(|| format!("net_{}", net.0), |n| n.name.clone());
                    unit_rails.entry(unit.clone()).or_default().insert(label);
                }
            }
            let distinct: BTreeSet<&String> = unit_rails.values().flat_map(|s| s.iter()).collect();
            if distinct.len() < 2 {
                continue;
            }
            let _ = &domains;
            let detail: Vec<String> = unit_rails
                .iter()
                .map(|(unit, rails)| {
                    format!(
                        "unit {unit} → {}",
                        rails.iter().cloned().collect::<Vec<_>>().join(", ")
                    )
                })
                .collect();
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Error,
                    "units of one part powered from different rails",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .expected(format!(
                    "every unit of {} to share one positive supply rail",
                    component.describe()
                ))
                .found(detail.join("; "))
                .message(format!(
                    "{} is a multi-unit package whose units are tied to different positive rails; \
                     a package has one supply, so this is either a mis-wire or the wrong symbol",
                    component.describe()
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-PINMUX-002 — a named function routed to a pin that cannot carry it
// -----------------------------------------------------------------------------

/// A net whose *name* names a function (`I2C1_SCL`, `UART0_TX`,
/// `USB_DP`) must only reach pins that declare that function. The
/// capability-consistency rules (`E-SYNTH-I2C-001`, `-SPI-001`,
/// `-UART-001`, `-USB-001`) already cover a net that carries a
/// *dedicated* peripheral pin; this rule covers the remaining case — a
/// net named for a function whose endpoints are all muxable pins, so no
/// dedicated peer exists to demand the protocol. A pin that declares no
/// capabilities at all (a resistor, a capacitor) is never judged: it
/// legitimately sits on the net.
pub(crate) struct PinFunctionSupportRule;

impl ErcRule for PinFunctionSupportRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-PINMUX-002"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Protocol
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for net in &board.nets {
            let Some(func) = PinCapability::from_net_name(&net.name) else {
                continue;
            };
            // If a dedicated peripheral pin for this family is present,
            // the protocol-consistency rule already owns the net.
            if net
                .endpoints
                .iter()
                .any(|e| is_dedicated_for(board, e.component, e.pin, func))
            {
                continue;
            }
            for endpoint in &net.endpoints {
                let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                    continue;
                };
                if pin.capabilities.is_empty() || pin.capabilities.contains(&func) {
                    continue;
                }
                let Some(component) = board.component(endpoint.component) else {
                    continue;
                };
                out.push(
                    DiagnosticBuilder::new(
                        self.code(),
                        Severity::Error,
                        "function routed to a pin that does not support it",
                    )
                    .location(Location::from_span(file.to_string(), endpoint.source_span))
                    .expected(format!(
                        "every pin on net `{}` to declare `{}`",
                        net.name,
                        crate::cap_name(func)
                    ))
                    .found(format!(
                        "{} has no `{}` capability",
                        component.describe_pin(&pin.name),
                        crate::cap_name(func)
                    ))
                    .message(format!(
                        "net `{}` names the `{}` function, but {} cannot carry it; move the net to \
                         a pin that lists `{}`, or rename the net if it is not that function",
                        net.name,
                        crate::cap_name(func),
                        component.describe_pin(&pin.name),
                        crate::cap_name(func),
                    ))
                    .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                    .build(),
                );
            }
        }
        out
    }
}

/// True when the pin is a *dedicated* peer for `func`'s protocol family:
/// it declares a capability from that family and none from a competing
/// family. Muxable MCU pins (many families at once) are not dedicated.
fn is_dedicated_for(board: &Board, c: ComponentId, p: PinId, func: PinCapability) -> bool {
    let Some((family, competing)) = protocol_family(func) else {
        return false;
    };
    endpoint_has_any_capability(board, c, p, family)
        && !endpoint_has_any_capability(board, c, p, competing)
}

/// The capability family `func` belongs to, paired with the families it
/// competes with on a muxable pin. `None` for a capability that is not
/// a muxed protocol function, so the mux rules never judge it.
#[allow(clippy::too_many_lines)]
fn protocol_family(
    func: PinCapability,
) -> Option<(&'static [PinCapability], &'static [PinCapability])> {
    use PinCapability::{
        Gpio, I2cScl, I2cSda, SpiCs, SpiMiso, SpiMosi, SpiSck, UartRx, UartTx, UsbCc, UsbDn, UsbDp,
        UsbVbus,
    };
    const I2C: &[PinCapability] = &[I2cSda, I2cScl];
    const SPI: &[PinCapability] = &[SpiMosi, SpiMiso, SpiSck, SpiCs];
    const UART: &[PinCapability] = &[UartTx, UartRx];
    const USB: &[PinCapability] = &[UsbDp, UsbDn, UsbVbus, UsbCc];
    const I2C_RIVALS: &[PinCapability] = &[
        Gpio, SpiMosi, SpiMiso, SpiSck, SpiCs, UartTx, UartRx, UsbDp, UsbDn, UsbVbus, UsbCc,
    ];
    const SPI_RIVALS: &[PinCapability] = &[
        Gpio, I2cSda, I2cScl, UartTx, UartRx, UsbDp, UsbDn, UsbVbus, UsbCc,
    ];
    const UART_RIVALS: &[PinCapability] = &[
        Gpio, I2cSda, I2cScl, SpiMosi, SpiMiso, SpiSck, SpiCs, UsbDp, UsbDn, UsbVbus, UsbCc,
    ];
    const USB_RIVALS: &[PinCapability] = &[
        Gpio, I2cSda, I2cScl, SpiMosi, SpiMiso, SpiSck, SpiCs, UartTx, UartRx,
    ];
    match func {
        I2cSda | I2cScl => Some((I2C, I2C_RIVALS)),
        SpiMosi | SpiMiso | SpiSck | SpiCs => Some((SPI, SPI_RIVALS)),
        UartTx | UartRx => Some((UART, UART_RIVALS)),
        UsbDp | UsbDn | UsbVbus | UsbCc => Some((USB, USB_RIVALS)),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// E-SYNTH-CAP-001 — Class-II ceramic at a high DC bias
// -----------------------------------------------------------------------------

/// A Class-II ceramic capacitor (X5R/X7R/X7S/Y5V/Z5U) sitting on a rail
/// at a large fraction of its rated voltage. Class-II dielectrics lose
/// effective capacitance under DC bias — an X7R "10 µF" at 80 % of its
/// rating can deliver a small fraction of nominal — so a design that
/// trusts the nominal value is under-decoupled. The rule needs the
/// dielectric and voltage rating (Phase 7 structured values) *and* a
/// known rail voltage; if any is missing it declines rather than
/// guessing.
pub(crate) struct CeramicDcBiasDeratingRule {
    threshold: f64,
}

impl CeramicDcBiasDeratingRule {
    pub(crate) fn new(config: &ErcConfig) -> Self {
        Self {
            threshold: config.ceramic_dc_bias_threshold,
        }
    }
}

impl ErcRule for CeramicDcBiasDeratingRule {
    fn code(&self) -> &'static str {
        "E-SYNTH-CAP-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Power
    }

    fn check(&self, board: &Board, file: &str) -> Vec<Diagnostic> {
        let domains = domains(board);
        let mut out = Vec::new();
        for component in &board.components {
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "capacitor" {
                continue;
            }
            let Some(dielectric) = component.properties.get("Dielectric") else {
                continue;
            };
            if !is_class_two_ceramic(dielectric) {
                continue;
            }
            let Some(rated_v) = component
                .properties
                .get("Voltage")
                .and_then(|v| crate::parse_voltage(v))
            else {
                continue;
            };
            if rated_v <= 0.0 {
                continue;
            }
            // The highest rail voltage the capacitor bridges.
            let mut bias: Option<(f64, &str)> = None;
            for idx in 0..part.pins.len() {
                for (net_id, net) in board.nets_containing(component.id, PinId(idx as u32)) {
                    let Some(v) = net_voltage(&domains, net_id) else {
                        continue;
                    };
                    if v > bias.map_or(f64::NEG_INFINITY, |(bv, _)| bv) {
                        bias = Some((v, net.name.as_str()));
                    }
                }
            }
            let Some((bias_v, rail)) = bias else {
                continue;
            };
            if bias_v <= 0.0 {
                continue;
            }
            let ratio = bias_v / rated_v;
            if ratio <= self.threshold {
                continue;
            }
            out.push(
                DiagnosticBuilder::new(
                    self.code(),
                    Severity::Warning,
                    "Class-II ceramic used at a high DC bias",
                )
                .location(Location::from_span(file.to_string(), component.source_span))
                .expected(format!(
                    "a Class-II ceramic to run at or below {:.0}% of its rated voltage, or a \
                     higher-rated / Class-I (C0G/NP0) part",
                    self.threshold * 100.0
                ))
                .found(format!(
                    "{} ({dielectric}, {rated_v:.0} V) sits on `{rail}` at {bias_v:.1} V ({:.0}% of rating)",
                    component.describe(),
                    ratio * 100.0
                ))
                .message(format!(
                    "{} is a Class-II ceramic at {:.0}% of its rated voltage; DC bias can cut its \
                     effective capacitance well below nominal — derate it in the design or use a \
                     higher-voltage / C0G part",
                    component.describe(),
                    ratio * 100.0
                ))
                .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
                .build(),
            );
        }
        out
    }
}

/// Class-II and Class-III ceramic dielectrics: the ones whose effective
/// capacitance falls under DC bias. Class-I (C0G/NP0) is stable and
/// never flagged.
fn is_class_two_ceramic(dielectric: &str) -> bool {
    matches!(
        dielectric.trim().to_ascii_uppercase().as_str(),
        "X5R" | "X7R" | "X7S" | "X7T" | "X6S" | "Y5V" | "Z5U"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_voltage_reads_the_description() {
        let mut part = Part {
            id: synth_registry::PartId("led_red_0603".into()),
            kind: "led".into(),
            description: Some("Red LED, 0603 package, ~2.0V Vf".into()),
            ..default_part()
        };
        // "2.0V Vf" is stated, so it wins over the colour table.
        assert_eq!(forward_voltage_v(&part), Some(2.0));
        part.description = None;
        assert_eq!(forward_voltage_v(&part), Some(1.9), "red fallback");
        part.id = synth_registry::PartId("led_blue_0603".into());
        assert_eq!(forward_voltage_v(&part), Some(3.2));
    }

    #[test]
    fn resistance_formatting() {
        assert_eq!(format_resistance(330.0), "330Ω");
        assert_eq!(format_resistance(4700.0), "4.7kΩ");
        assert_eq!(format_resistance(1_000_000.0), "1.0MΩ");
    }

    fn default_part() -> Part {
        Part {
            id: synth_registry::PartId("x".into()),
            kind: "led".into(),
            description: None,
            version: 0,
            lifecycle: synth_registry::Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            pins: Vec::new(),
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }
}
