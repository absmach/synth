// SPDX-License-Identifier: Apache-2.0

//! Circuit-design knowledge graph.
//!
//! Encodes *production support circuits* — the subcircuits every
//! production board needs around its principal components (debounce
//! networks for switches, pull-ups, current limiting for LEDs,
//! decoupling for ICs, flyback diodes for coils) — as declarative
//! TOML templates instead of hardcoded passes. Two consumers:
//!
//! 1. **The compiler**: `synth-validate` runs [`check_board`] as ERC
//!    rule `E-SYNTH-KG-001` and turns violations into diagnostics
//!    with machine-applicable insertion patches.
//! 2. **Agents**: the MCP tool `synth_query_knowledge` serves the
//!    catalog so a model drafting `.synth` source knows which support
//!    circuitry production designs require before it is told.
//!
//! Templates with a non-empty `enforced_by` are catalog-only: an
//! existing ERC rule, layout cluster, or human review already covers
//! them, and the checker here skips them (no double-flagging). Only
//! templates with an empty `enforced_by` are enforced by
//! [`check_board`].

use serde::{Deserialize, Deserializer};
use synth_diagnostics::{Patch, PatchKind, Severity};
use synth_ir::{Board, Component, ComponentId, Net, PinId};
use synth_registry::ElectricalType;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A support-circuit template: the knowledge-graph node. See the
/// module docs and `knowledge/circuits.toml` for the schema.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SupportTemplate {
    pub id: String,
    pub description: String,
    /// Why production hardware needs this — surfaced verbatim in
    /// diagnostics and the MCP catalog so the *reasoning* travels
    /// with the rule.
    pub rationale: String,
    pub severity: Severity,
    /// Component kinds (the `kind` declared in `.synth`) that trigger
    /// the requirement.
    pub applies_to_kinds: Vec<String>,
    pub condition: Condition,
    /// Non-empty ⇒ catalog-only (enforced elsewhere); the checker
    /// skips the template.
    #[serde(default)]
    pub enforced_by: String,
}

/// The built-in matching predicates. Deliberately a closed set
/// implemented in this crate, not an expression language — adding a
/// new condition means adding (and testing) a predicate here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// Catalog-only entry; no checker predicate.
    Always,
    /// A switch output reaching an IC input without a debounce
    /// network (series/pull resistor + shunt capacitor).
    SignalToIcWithoutRc,
    /// A switch wired to ground driving an IC input with no pull-up
    /// on the signal node.
    SwitchToGndWithoutPullup,
    /// An LED with no series current-limiting resistor on either
    /// terminal net.
    SeriesPathWithoutResistor,
    /// A relay coil without an antiparallel flyback diode.
    CoilWithoutFlybackDiode,
    /// An IC with power pins but no decoupling capacitor on any of
    /// its power nets (and no manifest-declared decoupling, which
    /// `E-SYNTH-POWER-001` already enforces).
    IcWithoutDecoupling,
    /// A polarised capacitor (KiCad `C_Polarized` convention: pin 1
    /// = plus) whose ground-side terminal is the plus pin — reversed
    /// bias.
    PolarizedCapGroundOnPlus,
}

impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "always" => Self::Always,
            "signal_to_ic_without_rc" => Self::SignalToIcWithoutRc,
            "switch_to_gnd_without_pullup" => Self::SwitchToGndWithoutPullup,
            "series_path_without_resistor" => Self::SeriesPathWithoutResistor,
            "coil_without_flyback_diode" => Self::CoilWithoutFlybackDiode,
            "ic_without_decoupling" => Self::IcWithoutDecoupling,
            "polarized_cap_ground_on_plus" => Self::PolarizedCapGroundOnPlus,
            other => {
                return Err(serde::de::Error::unknown_variant(
                    other,
                    &[
                        "always",
                        "signal_to_ic_without_rc",
                        "switch_to_gnd_without_pullup",
                        "series_path_without_resistor",
                        "coil_without_flyback_diode",
                        "ic_without_decoupling",
                        "polarized_cap_ground_on_plus",
                    ],
                ))
            }
        })
    }
}

/// One violation of a knowledge template found on a [`Board`].
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// Template that fired, e.g. `"switch_debounce_rc"`.
    pub template_id: String,
    /// Severity from the template.
    pub severity: Severity,
    /// Source span of the principal component (diagnostic location).
    pub span: synth_diagnostics::Span,
    /// Refdes of the principal component missing the support circuit.
    pub refdes: String,
    /// Human explanation (what is missing and why it matters).
    pub detail: String,
    /// Machine-applicable insertion patch, when the topology allows
    /// completing the circuit by pure insertion (no rewiring).
    pub suggested_fix: Option<Patch>,
}

/// The parsed knowledge graph.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeGraph {
    templates: Vec<SupportTemplate>,
}

impl KnowledgeGraph {
    /// Parse the graph from TOML text (the `[[template]]` array
    /// format shipped in `knowledge/circuits.toml`).
    pub fn from_toml_str(toml_src: &str) -> Result<Self, toml::de::Error> {
        #[derive(Deserialize)]
        struct File {
            #[serde(rename = "template")]
            templates: Vec<SupportTemplate>,
        }
        Ok(Self {
            templates: toml::from_str::<File>(toml_src)?.templates,
        })
    }

    /// The seed knowledge compiled into the binary, so the checker
    /// and MCP tool work without a workspace checkout.
    pub fn embedded() -> Self {
        static EMBEDDED: std::sync::OnceLock<KnowledgeGraph> = std::sync::OnceLock::new();
        EMBEDDED
            .get_or_init(|| {
                let dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/knowledge");
                let src = dir
                    .get_file("circuits.toml")
                    .unwrap_or_else(|| panic!("embedded knowledge must contain circuits.toml"));
                Self::from_toml_str(
                    std::str::from_utf8(src.contents()).expect("embedded knowledge must be UTF-8"),
                )
                .expect("embedded seed knowledge must always parse")
            })
            .clone()
    }

    pub fn templates(&self) -> &[SupportTemplate] {
        &self.templates
    }

    /// Templates that apply to a component kind — the "what does a
    /// production design need around X?" query used by the MCP tool.
    pub fn templates_for_kind(&self, kind: &str) -> Vec<&SupportTemplate> {
        self.templates
            .iter()
            .filter(|t| t.applies_to_kinds.iter().any(|k| k.as_str() == kind))
            .collect()
    }

    /// Templates this crate's checker enforces (empty `enforced_by`).
    pub fn enforced_templates(&self) -> impl Iterator<Item = &SupportTemplate> {
        self.templates.iter().filter(|t| t.enforced_by.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Board checking
// ---------------------------------------------------------------------------

/// Check `board` against every enforced template, returning one
/// [`Violation`] per principal component that fails a condition.
pub fn check_board(board: &Board, kg: &KnowledgeGraph) -> Vec<Violation> {
    let mut out = Vec::new();
    for template in kg.enforced_templates() {
        // ICs are identified by their *pins* (power input), not by a
        // kind name — the registry's kind taxonomy (mcu, sensor,
        // regulator, opamp, …) is open-ended, so a static
        // `applies_to_kinds` list can never be complete. That
        // condition therefore sees every component and filters
        // internally; the rest pre-filter by declared kind.
        let pin_identified = matches!(
            template.condition,
            Condition::IcWithoutDecoupling | Condition::PolarizedCapGroundOnPlus
        );
        let targets: Vec<&Component> = if pin_identified {
            board.components.iter().collect()
        } else {
            board
                .components
                .iter()
                .filter(|c| {
                    template
                        .applies_to_kinds
                        .iter()
                        .any(|k| k.as_str() == c.kind)
                })
                .collect()
        };
        if targets.is_empty() {
            continue;
        }
        match template.condition {
            Condition::Always => {} // catalog-only, skipped above
            Condition::SignalToIcWithoutRc => check_debounce(board, template, &targets, &mut out),
            Condition::SwitchToGndWithoutPullup => {
                check_switch_pullup(board, template, &targets, &mut out)
            }
            Condition::SeriesPathWithoutResistor => {
                check_led_series(board, template, &targets, &mut out)
            }
            Condition::CoilWithoutFlybackDiode => {
                check_flyback(board, template, &targets, &mut out)
            }
            Condition::IcWithoutDecoupling => {
                check_ic_decoupling(board, template, &targets, &mut out)
            }
            Condition::PolarizedCapGroundOnPlus => {
                check_polarized_caps(board, template, &targets, &mut out)
            }
        }
    }
    out
}

fn check_debounce(
    board: &Board,
    template: &SupportTemplate,
    switches: &[&Component],
    out: &mut Vec<Violation>,
) {
    for sw in switches {
        let Some(part) = sw.part.as_ref() else {
            continue;
        };
        for (pin_idx, p) in part.pins.iter().enumerate() {
            let pid = PinId(pin_idx as u32);
            let Some(net) = board.nets_containing(sw.id, pid).next().map(|(_, n)| n) else {
                continue;
            };
            // Does this pin's net reach an IC digital input?
            let Some(peer) = ic_input_peer(board, net, sw.id) else {
                continue;
            };
            if debounce_present(board, net) {
                continue;
            }
            let sw_side = switch_reference_net(board, sw, p);
            out.push(Violation {
                template_id: template.id.clone(),
                severity: template.severity,
                span: sw.source_span,
                refdes: sw.refdes.clone(),
                detail: format!(
                    "`{}.{}` drives `{}.{}` with no debounce network on net `{}`: \
                     contact bounce will produce phantom edges at the input",
                    sw.refdes,
                    p.name,
                    peer.refdes,
                    peer_pin_name(board, peer, net),
                    net.name,
                ),
                suggested_fix: debounce_patch(board, sw, pin_idx, &sw_side, peer),
            });
            break; // one violation per switch
        }
    }
}

fn check_switch_pullup(
    board: &Board,
    template: &SupportTemplate,
    switches: &[&Component],
    out: &mut Vec<Violation>,
) {
    for sw in switches {
        let Some(part) = sw.part.as_ref() else {
            continue;
        };
        // The pin whose net drives an IC input (the "wiper").
        let mut wiper: Option<(usize, Net)> = None;
        for (pin_idx, _) in part.pins.iter().enumerate() {
            let pid = PinId(pin_idx as u32);
            let Some(net) = board.nets_containing(sw.id, pid).next().map(|(_, n)| n) else {
                continue;
            };
            if ic_input_peer(board, net, sw.id).is_some() {
                wiper = Some((pin_idx, net.clone()));
                break;
            }
        }
        let Some((wiper_idx, wiper_net)) = wiper else {
            continue;
        };
        // Pull-up missing when the signal node has no resistor.
        if has_two_pin_kind(board, &wiper_net, "resistor") {
            continue;
        }
        let peer = match ic_input_peer(board, &wiper_net, sw.id) {
            Some(p) => p,
            None => continue,
        };
        out.push(Violation {
            template_id: template.id.clone(),
            severity: template.severity,
            span: sw.source_span,
            refdes: sw.refdes.clone(),
            detail: format!(
                "signal net `{}` has no pull-up: with the switch open the \
                 input at `{}.{}` floats to an undefined level",
                wiper_net.name,
                peer.refdes,
                peer_pin_name(board, peer, &wiper_net),
            ),
            suggested_fix: pullup_patch(board, sw, wiper_idx, &wiper_net, peer),
        });
    }
}

fn check_led_series(
    board: &Board,
    template: &SupportTemplate,
    leds: &[&Component],
    out: &mut Vec<Violation>,
) {
    for led in leds {
        let Some(part) = led.part.as_ref() else {
            continue;
        };
        let limited = part.pins.iter().enumerate().any(|(idx, _)| {
            board
                .nets_containing(led.id, PinId(idx as u32))
                .next()
                .map(|(_, n)| has_two_pin_kind(board, n, "resistor"))
                .unwrap_or(false)
        });
        if limited {
            continue;
        }
        out.push(Violation {
            template_id: template.id.clone(),
            severity: template.severity,
            span: led.source_span,
            refdes: led.refdes.clone(),
            detail: "LED has no series current-limiting resistor on either \
                     terminal: once forward-biased it overcurrents the driving \
                     pin or rail"
                .to_string(),
            // Fixing requires splitting an existing connect (rewiring),
            // which pure insertion cannot express — diagnostic only.
            suggested_fix: None,
        });
    }
}

fn check_flyback(
    board: &Board,
    template: &SupportTemplate,
    relays: &[&Component],
    out: &mut Vec<Violation>,
) {
    for relay in relays {
        let Some(part) = relay.part.as_ref() else {
            continue;
        };
        // Coil = the two non-supply-like passive pins with distinct nets.
        let coil_nets: Vec<&Net> = part
            .pins
            .iter()
            .enumerate()
            .filter(|(_, p)| p.electrical_type == ElectricalType::Passive)
            .filter_map(|(idx, _)| {
                board
                    .nets_containing(relay.id, PinId(idx as u32))
                    .next()
                    .map(|(_, n)| n)
            })
            .collect();
        if coil_nets.len() < 2 {
            continue;
        }
        let (net_a, net_b) = (coil_nets[0], coil_nets[1]);
        let flyback = board.components.iter().any(|c| {
            c.kind == "diode"
                && (0..pin_count(board, c.id)).all(|idx| {
                    board
                        .nets_containing(c.id, PinId(idx))
                        .next()
                        .map(|(_, n)| n.id == net_a.id || n.id == net_b.id)
                        .unwrap_or(false)
                })
        });
        if flyback {
            continue;
        }
        out.push(Violation {
            template_id: template.id.clone(),
            severity: template.severity,
            span: relay.source_span,
            refdes: relay.refdes.clone(),
            detail: format!(
                "relay coil between nets `{}` and `{}` has no flyback diode: \
                 opening the drive path collapses the coil field into a \
                 destructive voltage spike",
                net_a.name, net_b.name,
            ),
            suggested_fix: None, // antiparallel placement is topology-specific
        });
    }
}

fn check_ic_decoupling(
    board: &Board,
    template: &SupportTemplate,
    ics: &[&Component],
    out: &mut Vec<Violation>,
) {
    for ic in ics {
        let Some(part) = ic.part.as_ref() else {
            continue;
        };
        // Parts that declare their decoupling are E-SYNTH-POWER-001's
        // job — never double-flag.
        if !part.required_decoupling.is_empty() {
            continue;
        }
        let mut any_power_net = false;
        let mut all_power_nets_decoupled = true;
        for (idx, pin) in part.pins.iter().enumerate() {
            if !matches!(
                pin.electrical_type,
                ElectricalType::PowerInput | ElectricalType::PowerOutput
            ) {
                continue;
            }
            if is_ground_pin_name(&pin.name) {
                continue;
            }
            let Some(net) = board
                .nets_containing(ic.id, PinId(idx as u32))
                .next()
                .map(|(_, n)| n)
            else {
                continue; // floating power pin: CONNECT rules fire
            };
            any_power_net = true;
            if !net_has_cap_to_ground(board, net) {
                all_power_nets_decoupled = false;
            }
        }
        if !any_power_net || all_power_nets_decoupled {
            continue;
        }
        out.push(Violation {
            template_id: template.id.clone(),
            severity: template.severity,
            span: ic.source_span,
            refdes: ic.refdes.clone(),
            detail: format!(
                "`{}` ({}) has power pins with no local decoupling capacitor: \
                 switching current bursts turn supply inductance into rail \
                 droop and ground bounce",
                ic.refdes, ic.kind,
            ),
            suggested_fix: None, // E-SYNTH-POWER-001 owns insertion patches
        });
    }
}

/// Polarised-capacitor bias check. Identifies polarised parts via
/// the registry's KiCad symbol mapping (`Device:C_Polarized`), where
/// KiCad convention makes pin 1 the plus terminal. A polarised cap
/// with its plus pin on a ground net while the minus pin sits on a
/// non-ground net is installed backwards: it fails in the field, not
/// at the workbench.
fn check_polarized_caps(
    board: &Board,
    template: &SupportTemplate,
    caps: &[&Component],
    out: &mut Vec<Violation>,
) {
    for cap in caps {
        let Some(part) = cap.part.as_ref() else {
            continue;
        };
        let polarized = part
            .kicad_symbol
            .as_deref()
            .is_some_and(|k| k.to_lowercase().contains("polarized"));
        if !polarized {
            continue;
        }
        // Plus = pin numbered 1 (KiCad C_Polarized), or a pin named
        // pos/plus/+ as a fallback for alternate registry spellings.
        let plus_idx = part.pins.iter().position(|p| {
            p.number.0 == "1"
                || p.name.eq_ignore_ascii_case("pos")
                || p.name.eq_ignore_ascii_case("plus")
                || p.name.starts_with('+')
        });
        let Some(plus_idx) = plus_idx else {
            continue;
        };
        let Some(plus_net) = board
            .nets_containing(cap.id, PinId(plus_idx as u32))
            .next()
            .map(|(_, n)| n)
        else {
            continue;
        };
        if !net_has_ground_endpoint(board, plus_net) {
            continue; // plus is not grounded — orientation plausible
        }
        // Confirm the minus pin actually reaches a non-ground net, so
        // we don't flag floating/unwired caps.
        let minus_driven = part.pins.iter().enumerate().any(|(idx, _p)| {
            idx != plus_idx
                && board
                    .nets_containing(cap.id, PinId(idx as u32))
                    .next()
                    .map(|(_, n)| !net_has_ground_endpoint(board, n))
                    .unwrap_or(false)
        });
        if !minus_driven {
            continue;
        }
        out.push(Violation {
            template_id: template.id.clone(),
            severity: template.severity,
            span: cap.source_span,
            refdes: cap.refdes.clone(),
            detail: format!(
                "`{}` ({}) has its plus terminal (pin {}) on a ground net                  while the minus terminal drives a rail — reversed bias;                  polarised caps fail (vent or burst) installed backwards",
                cap.refdes, part.id, plus_idx + 1,
            ),
            suggested_fix: None, // fix is swapping the two connects: a rewire
        });
    }
}

// ---------------------------------------------------------------------------
// Topology helpers
// ---------------------------------------------------------------------------

/// Any other component on `net` (excluding `exclude`) that terminates
/// `net` at a digital input/bidirectional pin — the "IC input" a
/// switch or sensor drives.
fn ic_input_peer<'a>(board: &'a Board, net: &Net, exclude: ComponentId) -> Option<&'a Component> {
    net.endpoints.iter().find_map(|e| {
        if e.component == exclude {
            return None;
        }
        let comp = board.component(e.component)?;
        let part = comp.part.as_ref()?;
        let pin = part.pins.get(e.pin.0 as usize)?;
        matches!(
            pin.electrical_type,
            ElectricalType::Input | ElectricalType::Bidirectional
        )
        .then_some(comp)
    })
}

fn peer_pin_name(_board: &Board, peer: &Component, net: &Net) -> String {
    net.endpoints
        .iter()
        .find(|e| e.component == peer.id)
        .and_then(|e| {
            peer.part
                .as_ref()
                .and_then(|p| p.pins.get(e.pin.0 as usize))
                .map(|p| p.name.clone())
        })
        .unwrap_or_else(|| "?".to_string())
}

fn pin_count(board: &Board, id: ComponentId) -> u32 {
    board
        .component(id)
        .and_then(|c| c.part.as_ref())
        .map_or(0, |p| p.pins.len() as u32)
}

fn is_ground_pin_name(name: &str) -> bool {
    let n = name.to_lowercase();
    matches!(
        n.as_str(),
        "gnd" | "vss" | "vssa" | "vee" | "agnd" | "dgnd" | "vneg" | "ground"
    ) || n.starts_with("gnd")
        || n.starts_with("vss")
}

/// Ground-ness of a net endpoint: the *pin* decides (named ground pin
/// or `GroundReference` electrical type). Lowered nets are
/// auto-named, so net-name checks alone are unreliable.
fn endpoint_is_ground(board: &Board, e: &synth_ir::NetEndpoint) -> bool {
    board.pin(e.component, e.pin).is_some_and(|p| {
        matches!(p.electrical_type, ElectricalType::GroundReference) || is_ground_pin_name(&p.name)
    })
}

fn net_has_ground_endpoint(board: &Board, net: &Net) -> bool {
    net.endpoints.iter().any(|e| endpoint_is_ground(board, e))
}

/// Canonical debounce detection: the signal node (or a node one
/// resistor hop away) carries a shunt capacitor to a ground net.
/// The pull-up/series resistor itself is checked by
/// [`has_two_pin_kind`] inside `debounce_present`.
fn net_has_cap_to_ground(board: &Board, net: &Net) -> bool {
    net.endpoints.iter().any(|e| {
        board
            .component(e.component)
            .is_some_and(|c| c.kind == "capacitor")
            && other_pin_net_is_ground(board, e.component, e.pin)
    })
}

/// True when the two-pin component `id` (with terminal `pin` on some
/// net) has its *other* terminal on a ground net.
fn other_pin_net_is_ground(board: &Board, id: ComponentId, pin: PinId) -> bool {
    let Some(comp) = board.component(id) else {
        return false;
    };
    let Some(part) = comp.part.as_ref() else {
        return false;
    };
    // Find the other passive pin index (two-pin parts).
    let other: Vec<usize> = part
        .pins
        .iter()
        .enumerate()
        .filter(|(idx, p)| {
            p.electrical_type == ElectricalType::Passive && PinId(*idx as u32) != pin
        })
        .map(|(idx, _)| idx)
        .collect();
    other.iter().any(|idx| {
        board
            .nets_containing(id, PinId(*idx as u32))
            .next()
            .map(|(_, n)| net_has_ground_endpoint(board, n))
            .unwrap_or(false)
    })
}

/// The far side of a two-pin resistor on `net`, if any.
fn resistor_hop(board: &Board, net: &Net) -> Option<Net> {
    for e in &net.endpoints {
        let Some(comp) = board.component(e.component) else {
            continue;
        };
        if comp.kind != "resistor" {
            continue;
        }
        let Some(part) = comp.part.as_ref() else {
            continue;
        };
        for (idx, _) in part.pins.iter().enumerate() {
            let pid = PinId(idx as u32);
            if pid == e.pin {
                continue;
            }
            if let Some((_, far)) = board.nets_containing(comp.id, pid).next() {
                if far.id != net.id {
                    return Some(far.clone());
                }
            }
        }
    }
    None
}

/// Debounce present: shunt cap to ground on the node, or on a node
/// one resistor hop away (the classic SW—R—node(R, C)—IC topology).
fn debounce_present(board: &Board, net: &Net) -> bool {
    if net_has_cap_to_ground(board, net) {
        return true;
    }
    resistor_hop(board, net).is_some_and(|far| net_has_cap_to_ground(board, &far))
}

fn has_two_pin_kind(board: &Board, net: &Net, kind: &str) -> bool {
    net.endpoints
        .iter()
        .any(|e| board.component(e.component).is_some_and(|c| c.kind == kind))
}

/// How the switch is referenced: `Ground` (other terminal on a ground
/// net), `Supply` (on a supply net), or `Unknown` (SPDT and friends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchReference {
    Ground,
    Supply,
    Unknown,
}

fn switch_reference_net(
    board: &Board,
    sw: &Component,
    driven_pin: &synth_ir::Pin,
) -> SwitchReference {
    let Some(part) = sw.part.as_ref() else {
        return SwitchReference::Unknown;
    };
    for (idx, p) in part.pins.iter().enumerate() {
        if p.name == driven_pin.name {
            continue;
        }
        if let Some((_, net)) = board.nets_containing(sw.id, PinId(idx as u32)).next() {
            if net_has_ground_endpoint(board, net) {
                return SwitchReference::Ground;
            }
            if net_has_supply_endpoint(board, net) {
                return SwitchReference::Supply;
            }
        }
    }
    SwitchReference::Unknown
}

fn net_has_supply_endpoint(board: &Board, net: &Net) -> bool {
    net.endpoints.iter().any(|e| {
        board.component(e.component).is_some_and(|c| {
            c.part.as_ref().is_some_and(|p| {
                p.pins
                    .get(e.pin.0 as usize)
                    .is_some_and(|pin| pin.electrical_type == ElectricalType::PowerOutput)
            })
        })
    })
}

/// The first non-ground power-input pin (with its net) on `comp` —
/// the supply rail a pull-up should reference.
fn supply_pin_for(board: &Board, comp: &Component) -> Option<(String, String)> {
    let part = comp.part.as_ref()?;
    for (idx, pin) in part.pins.iter().enumerate() {
        if pin.electrical_type != ElectricalType::PowerInput || is_ground_pin_name(&pin.name) {
            continue;
        }
        if let Some((_, net)) = board.nets_containing(comp.id, PinId(idx as u32)).next() {
            return Some((pin.name.clone(), net.name.clone()));
        }
    }
    None
}

/// The first ground pin (with its net) on `comp`.
fn ground_pin_for(board: &Board, comp: &Component) -> Option<(String, String)> {
    let part = comp.part.as_ref()?;
    for (idx, pin) in part.pins.iter().enumerate() {
        if !matches!(
            pin.electrical_type,
            ElectricalType::PowerInput | ElectricalType::GroundReference
        ) || !is_ground_pin_name(&pin.name)
        {
            continue;
        }
        if let Some((_, net)) = board.nets_containing(comp.id, PinId(idx as u32)).next() {
            return Some((pin.name.clone(), net.name.clone()));
        }
    }
    None
}

/// Next free refdes for `prefix`, mirroring the allocator in
/// `synth-validate` (kept local to avoid a dependency cycle).
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

/// Next free refdes pair for a two-component insertion (`("R7",
/// "C8")`), mirroring the allocator in `synth-validate` (kept local to
/// avoid a dependency cycle).
fn next_two_free_refdes(board: &Board, a: &str, b: &str) -> (String, String) {
    let first = next_free_refdes(board, a);
    let mut bumped = board.clone();
    bumped.components.push(Component {
        id: ComponentId(u32::MAX),
        refdes: first.clone(),
        kind: String::new(),
        part: None,
        value: None,
        placement_hint: None,
        group: None,
        source_span: synth_diagnostics::Span::new(0, 0),
    });
    (first, next_free_refdes(&bumped, b))
}

// ---------------------------------------------------------------------------
// Insertion patches
// ---------------------------------------------------------------------------

/// Complete a switch input by insertion: pull resistor + shunt
/// capacitor onto the signal node. Works for switch-to-ground
/// (pull-up form) and switch-to-supply (pull-down form); unknown
/// references (SPDT) get no patch — the engineer must decide.
fn debounce_patch(
    board: &Board,
    sw: &Component,
    wiper_idx: usize,
    reference: &SwitchReference,
    peer: &Component,
) -> Option<Patch> {
    use std::fmt::Write as _;
    let part = sw.part.as_ref()?;
    let wiper_pin = part.pins.get(wiper_idx)?.name.clone();

    // Ground/supply references discovered from the *peer IC* — its
    // pins define which rails exist on this board.
    let (gnd_pin, _gnd_net) = ground_pin_for(board, peer)?;
    let (supply_pin, _supply_net) = supply_pin_for(board, peer)?;

    let (res_to, res_what) = match reference {
        SwitchReference::Ground => (supply_pin, "pull-up"),
        SwitchReference::Supply => (gnd_pin.clone(), "pull-down"),
        SwitchReference::Unknown => return None,
    };

    let (r, c) = next_two_free_refdes(board, "R", "C");

    let mut text = String::new();
    let _ = writeln!(
        text,
        "\n  component {r}: resistor \"r_generic_0603\" // auto-inserted debounce {res_what}"
    );
    let _ = writeln!(
        text,
        "  component {c}: capacitor \"c_generic_0603\" // auto-inserted debounce filter"
    );
    let _ = writeln!(text, "  connect {}.{} -> {r}.p1", sw.refdes, wiper_pin);
    let _ = writeln!(text, "  connect {}.{} -> {c}.p1", sw.refdes, wiper_pin);
    let _ = writeln!(text, "  connect {r}.p2 -> {}.{}", peer.refdes, res_to);
    let _ = writeln!(text, "  connect {c}.p2 -> {}.{}", peer.refdes, gnd_pin);

    Some(Patch {
        confidence: 0.8,
        rationale: Some(format!(
            "debounce `{}.{}`: {res_what} resistor {r} + filter capacitor {c} on the signal node",
            sw.refdes, wiper_pin
        )),
        patch_consequence_preview: None,
        kind: PatchKind::InsertAt {
            at: sw.source_span.byte_end,
            text,
        },
    })
}

/// Pull-up only (no capacitor): switch-to-ground input with a bare
/// signal node.
fn pullup_patch(
    board: &Board,
    sw: &Component,
    wiper_idx: usize,
    wiper_net: &Net,
    peer: &Component,
) -> Option<Patch> {
    use std::fmt::Write as _;
    let part = sw.part.as_ref()?;
    let wiper_pin = part.pins.get(wiper_idx)?.name.clone();
    let peer_input_pin = wiper_net
        .endpoints
        .iter()
        .find(|e| e.component == peer.id)
        .and_then(|e| {
            peer.part
                .as_ref()
                .and_then(|p| p.pins.get(e.pin.0 as usize))
                .map(|p| p.name.clone())
        })?;
    let (supply_pin, _supply_net) = supply_pin_for(board, peer)?;
    let (gnd_pin, _gnd_net) = ground_pin_for(board, peer)?;

    // Orientation: switch to ground ⇒ pull to supply; switch to
    // supply ⇒ pull to ground.
    let other_side = match switch_reference_net(board, sw, part.pins.get(wiper_idx)?) {
        SwitchReference::Ground => supply_pin,
        SwitchReference::Supply => gnd_pin,
        SwitchReference::Unknown => return None,
    };

    let r = next_free_refdes(board, "R");
    let mut text = String::new();
    let _ = writeln!(
        text,
        "\n  component {r}: resistor \"r_generic_0603\" // auto-inserted pull-up"
    );
    let _ = writeln!(
        text,
        "  connect {}.{} -> {r}.p1",
        peer.refdes, peer_input_pin
    );
    let _ = writeln!(text, "  connect {r}.p2 -> {}.{}", peer.refdes, other_side);

    Some(Patch {
        confidence: 0.85,
        rationale: Some(format!(
            "pull up `{}.{}` to {} with {r} so the input has a defined level \
             when the switch is open",
            sw.refdes, wiper_pin, other_side
        )),
        patch_consequence_preview: None,
        kind: PatchKind::InsertAt {
            at: sw.source_span.byte_end,
            text,
        },
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use synth_ir::{NetEndpoint, NetId};
    use synth_registry::{ElectricalType as ET, Part, PartId, Pin, PinNumber};

    fn pin(name: &str, t: ET) -> Pin {
        Pin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
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
            id: PartId(id.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: synth_registry::Lifecycle::Active,
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

    fn comp(id: u32, refdes: &str, kind: &str, part: Part) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: kind.to_string(),
            part: Some(part),
            value: None,
            placement_hint: None,
            group: None,
            source_span: synth_diagnostics::Span::new(10, 20),
        }
    }

    fn net(id: u32, name: &str, endpoints: Vec<(u32, u32)>) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .into_iter()
                .map(|(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: synth_diagnostics::Span::new(0, 0),
                })
                .collect(),
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            name: "kg_test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components,
            nets,
            diff_pairs: vec![],
            keepouts: vec![],
            source_span: synth_diagnostics::Span::new(0, 100),
        }
    }

    fn kg() -> KnowledgeGraph {
        KnowledgeGraph::embedded()
    }

    fn switch_part() -> Part {
        part(
            "spst",
            "switch",
            vec![pin("p1", ET::Passive), pin("p2", ET::Passive)],
        )
    }

    fn mcu_part() -> Part {
        part(
            "mcu",
            "mcu",
            vec![
                pin("vdd", ET::PowerInput),
                pin("gnd", ET::GroundReference),
                pin("gpio", ET::Input),
            ],
        )
    }

    fn r_part() -> Part {
        part(
            "r",
            "resistor",
            vec![pin("p1", ET::Passive), pin("p2", ET::Passive)],
        )
    }

    fn c_part() -> Part {
        part(
            "c",
            "capacitor",
            vec![pin("p1", ET::Passive), pin("p2", ET::Passive)],
        )
    }

    fn led_part() -> Part {
        part(
            "led",
            "led",
            vec![pin("anode", ET::Passive), pin("cathode", ET::Passive)],
        )
    }

    #[test]
    fn embedded_seed_knowledge_parses() {
        let kg = kg();
        assert!(kg.templates().len() >= 8, "seed catalog shipped");
        let debounce = kg
            .templates()
            .iter()
            .find(|t| t.id == "switch_debounce_rc")
            .expect("debounce template");
        assert_eq!(debounce.severity, Severity::Error);
        assert!(debounce.enforced_by.is_empty(), "debounce is KG-enforced");
        let i2c = kg
            .templates()
            .iter()
            .find(|t| t.id == "i2c_pullups")
            .expect("i2c template");
        assert_eq!(i2c.enforced_by, "E-SYNTH-I2C-001", "catalog-only entry");
    }

    #[test]
    fn templates_for_kind_matches_declared_kinds() {
        let kg = kg();
        let switch_templates = kg.templates_for_kind("switch");
        assert_eq!(switch_templates.len(), 2, "debounce + pull-up");
        let crystal = kg.templates_for_kind("crystal");
        assert_eq!(crystal.len(), 1);
        assert_eq!(crystal[0].id, "crystal_load_caps");
    }

    #[test]
    fn bare_switch_to_ground_violates_debounce_and_pullup() {
        // SW1.p1 on "sig" (with MCU gpio input), SW1.p2 on "gnd".
        // No pull-up, no cap — both switch templates must fire.
        let b = board(
            vec![
                comp(0, "SW1", "switch", switch_part()),
                comp(1, "U1", "mcu", mcu_part()),
            ],
            vec![
                net(1, "sig", vec![(0, 0), (1, 2)]),
                net(2, "gnd", vec![(0, 1), (1, 1)]),
                net(3, "3v3", vec![(1, 0)]),
            ],
        );
        let mut v = check_board(&b, &kg());
        v.sort_by(|a, b| a.template_id.cmp(&b.template_id));
        // Both switch templates fire, plus ic_decoupling (U1's vdd
        // net has no cap — correct, the board is a bare skeleton).
        assert_eq!(v.len(), 3, "debounce + pull_up + ic_decoupling");
        assert_eq!(v[0].template_id, "ic_decoupling");
        assert_eq!(v[1].template_id, "switch_debounce_rc");
        assert_eq!(v[2].template_id, "switch_pull_up");
        // The debounce violation carries an insertion patch: pull-up
        // resistor + filter capacitor referenced to the peer IC's rails.
        let debounce_text = match &v[1].suggested_fix.as_ref().expect("debounce patch").kind {
            PatchKind::InsertAt { text, .. } => text.clone(),
            other => panic!("unexpected patch kind: {other:?}"),
        };
        assert!(
            debounce_text.contains("connect SW1.p1 -> R1.p1"),
            "{debounce_text}"
        );
        assert!(
            debounce_text.contains("connect R1.p2 -> U1.vdd"),
            "{debounce_text}"
        );
        assert!(
            debounce_text.contains("connect C1.p2 -> U1.gnd"),
            "{debounce_text}"
        );
        // The pull-up violation carries an insertion patch that wires
        // the resistor from the MCU input pin to its supply pin.
        let pullup_text = match &v[2].suggested_fix.as_ref().expect("pullup patch").kind {
            PatchKind::InsertAt { text, .. } => text.clone(),
            other => panic!("unexpected patch kind: {other:?}"),
        };
        assert!(
            pullup_text.contains("connect U1.gpio -> R1.p1"),
            "{pullup_text}"
        );
        assert!(
            pullup_text.contains("connect R1.p2 -> U1.vdd"),
            "{pullup_text}"
        );
    }

    #[test]
    fn debounced_switch_input_is_clean() {
        // SW1—R1—node with C1 to gnd, IC input on the node: the
        // classic pull-up + filter topology satisfies both templates.
        let b = board(
            vec![
                comp(0, "SW1", "switch", switch_part()),
                comp(1, "U1", "mcu", mcu_part()),
                comp(2, "R1", "resistor", r_part()),
                comp(3, "C1", "capacitor", c_part()),
            ],
            vec![
                net(1, "sig", vec![(0, 0), (2, 0)]), // SW1.p1 — R1.p1
                net(2, "gnd", vec![(0, 1), (1, 1), (3, 1)]),
                net(3, "node", vec![(2, 1), (3, 0), (1, 2)]), // R1.p2, C1.p1, U1.gpio
            ],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "switch_debounce_rc"),
            "debounced input must not violate: {v:?}"
        );
        assert!(
            v.iter().all(|x| x.template_id != "switch_pull_up"),
            "pulled-up input must not violate: {v:?}"
        );
    }

    #[test]
    fn led_without_series_resistor_violates() {
        // LED directly between gnd and the MCU output net.
        let b = board(
            vec![
                comp(0, "D1", "led", led_part()),
                comp(1, "U1", "mcu", mcu_part()),
            ],
            vec![
                net(1, "gnd", vec![(0, 0), (1, 1)]),
                net(2, "led_sig", vec![(0, 1), (1, 2)]),
            ],
        );
        let v = check_board(&b, &kg());
        let led: Vec<_> = v
            .iter()
            .filter(|x| x.template_id == "led_current_limit")
            .collect();
        assert_eq!(led.len(), 1, "{v:?}");
        assert!(
            led[0].suggested_fix.is_none(),
            "rewire cannot be an insertion"
        );
    }

    #[test]
    fn led_with_series_resistor_is_clean() {
        let b = board(
            vec![
                comp(0, "D1", "led", led_part()),
                comp(1, "U1", "mcu", mcu_part()),
                comp(2, "R1", "resistor", r_part()),
            ],
            vec![
                net(1, "gnd", vec![(0, 0), (1, 1)]),
                net(2, "led_sig", vec![(0, 1), (2, 0)]), // D1.cathode — R1.p1
                net(3, "gpio", vec![(2, 1), (1, 2)]),    // R1.p2 — U1.gpio
            ],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "led_current_limit"),
            "current-limited LED must not violate: {v:?}"
        );
    }

    #[test]
    fn ic_without_decoupling_warns() {
        // U1 has no required_decoupling manifest and no cap on vdd.
        let b = board(
            vec![comp(0, "U1", "mcu", mcu_part())],
            vec![net(1, "3v3", vec![(0, 0)]), net(2, "gnd", vec![(0, 1)])],
        );
        let v = check_board(&b, &kg());
        let dec: Vec<_> = v
            .iter()
            .filter(|x| x.template_id == "ic_decoupling")
            .collect();
        assert_eq!(dec.len(), 1, "{v:?}");
        assert_eq!(dec[0].severity, Severity::Warning);
    }

    #[test]
    fn ic_with_cap_on_power_net_is_clean() {
        let b = board(
            vec![
                comp(0, "U1", "mcu", mcu_part()),
                comp(1, "C1", "capacitor", c_part()),
            ],
            vec![
                net(1, "3v3", vec![(0, 0), (1, 0)]), // C1.p1 on vdd net
                net(2, "gnd", vec![(0, 1), (1, 1)]),
            ],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "ic_decoupling"),
            "decoupled IC must not violate: {v:?}"
        );
    }

    #[test]
    fn manifest_declared_decoupling_is_not_double_flagged() {
        // POWER-001's territory: the part declares required_decoupling,
        // so the KG must stay silent even with no cap present.
        let mut p = mcu_part();
        p.required_decoupling = vec![synth_registry::RequiredDecoupling {
            net: "vdd".to_string(),
            value: "100n".to_string(),
            count: 1,
            max_distance_mm: None,
        }];
        let b = board(
            vec![comp(0, "U1", "mcu", p)],
            vec![net(1, "3v3", vec![(0, 0)]), net(2, "gnd", vec![(0, 1)])],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "ic_decoupling"),
            "manifest-declared decoupling belongs to E-SYNTH-POWER-001: {v:?}"
        );
    }

    #[test]
    fn catalog_templates_never_fire_from_the_checker() {
        // A board with an IC but no ESD array, no reset RC, and no
        // crystal: catalog-only templates (usb_esd, reset_rc,
        // crystal_load_caps, i2c_pullups) must not appear.
        let b = board(
            vec![comp(0, "U1", "mcu", mcu_part())],
            vec![net(1, "3v3", vec![(0, 0)]), net(2, "gnd", vec![(0, 1)])],
        );
        let v = check_board(&b, &kg());
        for violation in &v {
            assert_ne!(violation.template_id, "usb_esd");
            assert_ne!(violation.template_id, "reset_rc");
            assert_ne!(violation.template_id, "crystal_load_caps");
            assert_ne!(violation.template_id, "i2c_pullups");
        }
    }
    fn polarized_cap_part() -> Part {
        // Real registry entries number polarised-cap pins "1"/"2"
        // (KiCad C_Polarized: pin 1 = plus).
        let mut p = part(
            "c_electrolytic",
            "capacitor",
            vec![pin("p1", ET::Passive), pin("p2", ET::Passive)],
        );
        p.kicad_symbol = Some("Device:C_Polarized".to_string());
        p.pins[0].number = PinNumber("1".to_string());
        p.pins[1].number = PinNumber("2".to_string());
        p
    }

    #[test]
    fn polarized_cap_ground_on_plus_violates() {
        // C1 plus (p1) on gnd, minus (p2) on the rail: reversed bias.
        let b = board(
            vec![
                comp(0, "C1", "capacitor", polarized_cap_part()),
                comp(1, "U1", "mcu", mcu_part()),
            ],
            vec![
                net(1, "3v3", vec![(0, 1)]),         // C1.p2 — plus net is grounded below
                net(2, "gnd", vec![(0, 0), (1, 1)]), // C1.p1 (PLUS) on gnd
            ],
        );
        let v = check_board(&b, &kg());
        let pol: Vec<_> = v
            .iter()
            .filter(|x| x.template_id == "polarized_cap_miswired")
            .collect();
        assert_eq!(pol.len(), 1, "{v:?}");
        assert_eq!(pol[0].severity, Severity::Error);
    }

    #[test]
    fn polarized_cap_minus_on_ground_is_clean() {
        // Correct orientation: plus on the rail, minus on ground.
        let b = board(
            vec![
                comp(0, "C1", "capacitor", polarized_cap_part()),
                comp(1, "U1", "mcu", mcu_part()),
            ],
            vec![
                net(1, "3v3", vec![(0, 0)]), // C1.p1 (plus) on rail
                net(2, "gnd", vec![(0, 1), (1, 1)]),
            ],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "polarized_cap_miswired"),
            "correct polarity must not fire: {v:?}"
        );
    }

    #[test]
    fn non_polarized_cap_never_triggers_polarity_check() {
        let b = board(
            vec![
                comp(0, "C1", "capacitor", c_part()),
                comp(1, "U1", "mcu", mcu_part()),
            ],
            vec![
                net(1, "3v3", vec![(0, 1)]),
                net(2, "gnd", vec![(0, 0), (1, 1)]),
            ],
        );
        let v = check_board(&b, &kg());
        assert!(
            v.iter().all(|x| x.template_id != "polarized_cap_miswired"),
            "generic caps have no polarity: {v:?}"
        );
    }

    #[test]
    fn capacitor_kind_now_maps_to_polarity_template() {
        let kg = kg();
        assert!(kg
            .templates_for_kind("capacitor")
            .iter()
            .any(|t| t.id == "polarized_cap_miswired"));
    }
}
