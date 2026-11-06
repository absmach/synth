// SPDX-License-Identifier: Apache-2.0

//! Physical-pin reconciliation between Synth's logical part model and
//! the full pin set of the KiCad symbol it references.
//!
//! ## Why this exists
//!
//! SynthSpec parts declare the *logical* pins a design is expected to
//! use (`vdd`, `vss`, `pb6`, ...). The exported schematic references
//! the real KiCad symbol, which carries *every* physical pin — on an
//! STM32F103C8 that's 48 pins, including four VDD/VSS legs and VDDA/
//! VSSA that the registry never declares. If the exporter wires only
//! the declared pins, KiCad's ERC flags every undeclared physical pin:
//! `pin_not_connected` on unused GPIOs and `power_pin_not_driven` on
//! the power legs that the netlist silently left floating.
//!
//! This module closes that gap at export time:
//!
//! 1. **Power-leg fan-out** — every physical `power_in`/`power_out`
//!    pin on the referenced symbol that isn't already in the netlist
//!    is assigned to the matching rail net (the net that carries the
//!    component's declared VDD/VCC leg for positive rails, or
//!    GND/VSS for grounds). The schematic renders a power symbol on
//!    each fan-out leg and the PCB stamps its pad with the rail net,
//!    so KiCad ERC/DRC see a fully-powered part.
//! 2. **No-connect marking** — every physical non-power pin the
//!    netlist doesn't reach (unused GPIOs, boot straps, idle
//!    passives) is reported for a `(no_connect ...)` marker, which
//!    ERC recognises as "intentionally unconnected".
//!
//! The module is placement-agnostic: callers compute terminal
//! coordinates through [`physical_terminal`] using their own
//! placement model, so the schematic and PCB exporters share the same
//! reconciliation without sharing geometry types.

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, ComponentId, NetId};

use crate::kicad_lib_loader::PhysicalPin;
use crate::route::{natural_rotation_offset, pin_terminal_xy, snap_grid_127};
use crate::{ComponentPlacement, Rotation, POWER_STUB_LEN};

/// Which rail family a physical power pin belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RailFamily {
    /// Ground-family leg (GND, VSS, VSSA, VGND, ...).
    Ground,
    /// Positive supply leg (VDD, VDDA, VBAT, VCC, AVDD, 3V3, ...).
    Positive,
}

impl RailFamily {
    /// Classify a physical pin by its KiCad electrical type and name.
    /// Pins that aren't power return `None`.
    ///
    /// This stays name+type based by design: it classifies the
    /// *physical symbol* pin (KiCad reality — `VDDA`, `VSS`, …), where
    /// there is no declared voltage to consult. The *net* side instead
    /// goes through `net_family`, which prefers the declared
    /// `power "NAME" V` voltage and the inferred power domains and only
    /// falls back to name heuristics.
    pub fn for_pin(electrical_type: &str, name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let is_gnd = matches!(
            lower.as_str(),
            "gnd" | "vss" | "vssa" | "vsss" | "vgnd" | "gnda" | "agnd" | "ground" | "ep"
        ) || lower.starts_with("gnd")
            || lower.starts_with("vss");
        let is_vbus = matches!(
            lower.as_str(),
            "vbus" | "vbus1" | "vbus2" | "vin" | "vcc" | "vdd" | "3v3" | "5v"
        ) || lower.starts_with("vbus");
        let is_power = matches!(
            electrical_type,
            "power_in" | "power_output" | "power_out" | "power_input"
        ) || is_gnd
            || is_vbus;
        if !is_power {
            return None;
        }
        if is_gnd {
            Some(Self::Ground)
        } else {
            Some(Self::Positive)
        }
    }
}

/// One power leg to fan out onto a rail net.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerLeg {
    pub component: ComponentId,
    /// Physical pin number on the symbol (`"36"`, `"9"`, ...).
    pub pin_number: String,
    /// Pin name as written on the symbol (`"VDD"`, `"VDDA"`, ...).
    pub pin_name: String,
    /// Physical pin electrical type (`"power_in"`, ...).
    pub electrical_type: String,
    /// The rail net this leg joins.
    pub net: NetId,
    pub family: RailFamily,
}

/// A physical non-power pin to mark with `(no_connect ...)`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoConnectPin {
    pub component: ComponentId,
    pub pin_number: String,
    pub pin_name: String,
}

/// A power net that KiCad ERC would flag as undriven — one carrying
/// `power_in` / `power_out` flags but no on-sheet `power_out` driver
/// pin. `anchor_*` selects a terminal to attach a `power:PWR_FLAG`
/// (whose `power_out` pin satisfies the driver requirement).
#[derive(Debug, Clone, PartialEq)]
pub struct PowerDriver {
    pub net: NetId,
    /// A component whose declared pin already sits on this net and
    /// can host the flag's stub.
    pub anchor_component: ComponentId,
    pub anchor_pin_number: String,
    pub anchor_pin_name: String,
    /// Center (mm) of the anchor component's placement.
    pub anchor_center: (f64, f64),
    /// Terminal `(x, y, dx, dy)` of the anchor pin on the sheet, set when the
    /// anchor is drawn from a generated symbol: there is no stock symbol to
    /// read it from.
    pub anchor_terminal: Option<(f64, f64, f64, f64)>,
}

/// Result of reconciling one component's symbol pin set against the
/// netlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReconciledPins {
    pub power_legs: Vec<PowerLeg>,
    pub no_connects: Vec<NoConnectPin>,
}

/// Net-side rail family: the declared `power "NAME" V` voltage wins
/// (a `0V` declaration means ground), then the inferred power domains,
/// then the same name heuristics [`RailFamily::for_pin`] uses. A signal
/// net with no voltage, no rail/ground domain, and no rail-like name
/// returns `None`.
///
/// This is what lets fan-out follow real rail declarations instead of
/// guessing from the registry pin names: a regulator `vout` leg on a
/// declared `+3V3` net records a positive rail even when the pin name
/// itself is not in any hardcoded list.
fn net_family(net: &synth_ir::Net, domains: &synth_ir::PowerDomainMap) -> Option<RailFamily> {
    if let Some(v) = net.voltage.map(synth_ir::Voltage::to_v) {
        return Some(if v == 0.0 {
            RailFamily::Ground
        } else {
            RailFamily::Positive
        });
    }
    if let Some(kind) = domains.get(net.id) {
        if kind.is_ground() {
            return Some(RailFamily::Ground);
        }
        if kind.is_rail() {
            return Some(RailFamily::Positive);
        }
    }
    let lower = net.name.to_ascii_lowercase();
    let is_gnd = matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "vsss" | "vgnd" | "gnda" | "agnd" | "ground" | "0v"
    ) || lower.starts_with("gnd")
        || lower.starts_with("vss");
    if is_gnd {
        return Some(RailFamily::Ground);
    }
    let is_rail = matches!(
        lower.as_str(),
        "vbus" | "vbus1" | "vbus2" | "vin" | "vcc" | "vdd" | "3v3" | "5v" | "+3v3" | "+5v"
    ) || lower.starts_with("vbus")
        || lower.starts_with('+');
    if is_rail {
        return Some(RailFamily::Positive);
    }
    None
}

/// Normalise a net/pin name for affinity matching: lowercase alnum
/// only, so `+3V3`, `3v3`, and `3V3` compare equal.
fn normalize_rail_name(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Reconcile every component in `board`: returns the fan-out power
/// legs and the pins to mark no-connect. Components without a
/// `kicad_symbol` mapping (synthesized rectangles) are skipped — their
/// declared pin set *is* the full symbol, so there is nothing to fan
/// out or suppress.
///
/// Deterministic: iterates boards components in order and physical
/// pins in symbol order.
#[must_use]
pub fn reconcile_all(board: &Board) -> Vec<ReconciledPins> {
    board
        .components
        .iter()
        .map(|component| reconcile_component(board, component))
        .collect()
}

/// Find the power nets that KiCad ERC considers undriven: nets that
/// carry power-in flags but no `power_out` / `power_input`-driven
/// source. `placements` maps directive-driven centers; an anchor
/// point is only emitted when the anchor component has a placement.
///
/// In KiCad's ERC model a power net is "driven" only by a pin with an
/// *output* power electrical type (`PT_POWER_OUT`). A rail fed purely
/// from a connector or passive header (its supply originates off-
/// sheet) has no such pin, so ERC emits `power_pin_not_driven`. The
/// KiCad-idiomatic fix is a `power:PWR_FLAG` symbol (electrical type
/// `power_out`) placed on that net — the same `#FLG` marker KiCad's
/// own reference designs drop at board power-entry points.
#[must_use]
pub fn undriven_power_nets<S: ::std::hash::BuildHasher>(
    board: &Board,
    placements: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement, S>,
) -> Vec<PowerDriver> {
    let mut out = Vec::new();
    for net in &board.nets {
        // A rail is "undriven" when it carries a power-input endpoint
        // (per the netlist) but no power-output driver. We determine
        // `power_input` from the *registry* electrical type (synth's
        // authoritative model — pin numbers in the registry are what
        // define its logical pins) and `power_out` from the
        // *symbol's* physical pin types when available.
        //
        // Two traps this avoids:
        //  - A connector like Micro-USB declares VBUS/GND as
        //    `power_in` in the registry but `power_out` on the symbol
        //    — such rails ARE driven and must not get a flag.
        //  - A registry pin that's actually a signal (e.g. `usb_dp`)
        //    must not be treated as a power input just because the
        //    symbol's physical pin at that number is a power pin.
        let mut has_power_input = false;
        let mut has_registry_power_output = false;
        let mut has_symbol_power_output = false;
        for endpoint in &net.endpoints {
            let Some(component) = board.component(endpoint.component) else {
                continue;
            };
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                continue;
            };
            match pin.electrical_type {
                synth_registry::ElectricalType::PowerInput => has_power_input = true,
                synth_registry::ElectricalType::PowerOutput => {
                    has_registry_power_output = true;
                }
                synth_registry::ElectricalType::GroundReference => {
                    has_power_input = true;
                }
                _ => {}
            }
            // Symbol-level driver check (only supplements; registry is
            // authoritative for whether power inputs exist).
            if let Some(lib_id) = part.kicad_symbol.as_deref() {
                if let Some(physical) = crate::kicad_lib_loader::physical_pins(lib_id)
                    .and_then(|pins| pins.into_iter().find(|p| p.number == pin.number.0))
                {
                    if physical.electrical_type == "power_out"
                        || physical.electrical_type == "power_output"
                    {
                        has_symbol_power_output = true;
                    }
                }
            }
        }
        if has_power_input && !has_registry_power_output && !has_symbol_power_output {
            // Pick the first power-input endpoint with a placement as
            // the anchor for the flag's stub. A pin of a part with a stock
            // symbol is preferred; a generated symbol is the fallback, so a
            // board whose only power pins sit on generated symbols is still
            // driven.
            let mut anchor = None;
            for generated in [false, true] {
                if anchor.is_some() {
                    break;
                }
                for endpoint in &net.endpoints {
                    let Some(component) = board.component(endpoint.component) else {
                        continue;
                    };
                    let Some(part) = component.part.as_ref() else {
                        continue;
                    };
                    let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                        continue;
                    };
                    if pin.electrical_type != synth_registry::ElectricalType::PowerInput
                        && pin.electrical_type != synth_registry::ElectricalType::GroundReference
                    {
                        continue;
                    }
                    // The flag's stub starts at the pin's terminal. A stock
                    // symbol gives it from its library file; a generated
                    // one from the body Synth draws.
                    if part.kicad_symbol.is_none() != generated {
                        continue;
                    }
                    let Some(placement) = placements.get(&endpoint.component) else {
                        continue;
                    };
                    let anchor_terminal = if generated {
                        let by_id: std::collections::HashMap<
                            ComponentId,
                            &crate::ComponentPlacement,
                        > = placements.iter().map(|(id, p)| (*id, *p)).collect();
                        let Some(terminal) = crate::route::pin_terminal_xy(
                            board,
                            endpoint.component,
                            endpoint.pin,
                            &by_id,
                        ) else {
                            continue;
                        };
                        Some(terminal)
                    } else {
                        None
                    };
                    anchor = Some(PowerDriver {
                        net: net.id,
                        anchor_component: endpoint.component,
                        anchor_pin_number: pin.number.0.clone(),
                        anchor_pin_name: pin.name.clone(),
                        anchor_center: placement.center_mm,
                        anchor_terminal,
                    });
                    break;
                }
            }
            if let Some(anchor) = anchor {
                out.push(anchor);
            }
        }
    }
    out
}

/// Pick one rail net for a physical pin from the same-family
/// candidates (board-net order). An exact normalized name match wins
/// (`VDDA` joins the `VDDA` net, not the `VDD` one when a part touches
/// both); otherwise a net with a declared `power "NAME" V` voltage wins
/// over a purely heuristic one; otherwise the first candidate wins.
/// Deterministic: candidates arrive in board-net order and the tiebreak
/// never depends on hash order.
fn pick_rail_net(board: &Board, candidates: &[NetId], pin_name: &str) -> Option<NetId> {
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    let want = normalize_rail_name(pin_name);
    for id in candidates {
        if let Some(net) = board.net(*id) {
            if normalize_rail_name(&net.name) == want {
                return Some(*id);
            }
        }
    }
    for id in candidates {
        if let Some(net) = board.net(*id) {
            if net.voltage.is_some() {
                return Some(*id);
            }
        }
    }
    candidates.first().copied()
}

fn reconcile_component(board: &Board, component: &synth_ir::Component) -> ReconciledPins {
    let mut out = ReconciledPins::default();
    let Some(part) = component.part.as_ref() else {
        return out;
    };
    let lib_id = part.kicad_symbol.as_deref();

    // Physical pin set:
    //  - With a `kicad_symbol`, the symbol carries the authoritative
    //    full pin inventory (including power legs and stubs the
    //    registry never declares).
    //  - Without one (synthesized rectangle), the registry's declared
    //    pins ARE the full symbol; emit `no_connect` for any declared
    //    pin the netlist doesn't reach, and fan out undeclared power
    //    legs as usual.
    let physical: Vec<crate::kicad_lib_loader::PhysicalPin> = match lib_id {
        Some(lid) => match crate::kicad_lib_loader::physical_pins(lid) {
            Some(p) => p,
            None => return out,
        },
        None => part
            .pins
            .iter()
            .map(|p| PhysicalPin {
                number: p.number.0.clone(),
                name: p.name.clone(),
                electrical_type: match p.electrical_type {
                    synth_registry::ElectricalType::PowerOutput => "power_out".to_string(),
                    synth_registry::ElectricalType::PowerInput
                    | synth_registry::ElectricalType::GroundReference => "power_in".to_string(),
                    _ => "passive".to_string(),
                },
                x: 0.0,
                y: 0.0,
            })
            .collect(),
    };

    // Which physical pin numbers does the netlist already reach?
    let mut netlisted: HashSet<String> = HashSet::new();
    // Rail nets this component touches, keyed by family, in board-net
    // order. The net side prefers the declared `power "NAME" V`
    // voltage and the inferred power domains; the pin side keeps the
    // name+type heuristic (there is no declared voltage on a physical
    // symbol pin). A net is recorded when *either* side classifies it,
    // so a regulator `vout` leg on a declared `+3V3` net counts as a
    // positive rail even though `vout` matches no hardcoded name.
    let domains = synth_ir::infer_power_domains(board);
    let mut rail_nets: HashMap<RailFamily, Vec<NetId>> = HashMap::new();

    for net in &board.nets {
        for endpoint in &net.endpoints {
            if endpoint.component != component.id {
                continue;
            }
            let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                continue;
            };
            netlisted.insert(pin.number.0.clone());
            let pin_family = RailFamily::for_pin(
                match pin.electrical_type {
                    synth_registry::ElectricalType::PowerOutput => "power_out",
                    synth_registry::ElectricalType::PowerInput
                    | synth_registry::ElectricalType::GroundReference => "power_in",
                    _ => "non_power",
                },
                &pin.name,
            );
            let net_side = net_family(net, &domains);
            // Declared voltage is authoritative: when the net carries
            // one, the net side wins over a disagreeing pin heuristic
            // (a short between two named rails is ERC's story, not a
            // reason to fan out onto the wrong one).
            let family = if net.voltage.is_some() {
                net_side.or(pin_family)
            } else {
                pin_family.or(net_side)
            };
            if let Some(family) = family {
                rail_nets.entry(family).or_default().push(net.id);
            }
        }
    }
    // Deduplicate while keeping board-net order (a net with several
    // endpoints of this component must not outvote the others).
    for nets in rail_nets.values_mut() {
        let mut seen = HashSet::new();
        nets.retain(|id| seen.insert(*id));
    }

    for pin in &physical {
        if netlisted.contains(&pin.number) {
            continue;
        }
        // RP2040/RP2350's stock symbol contains a hidden passive DVDD pin
        // (pin 50) outside the visible symbol body. KiCad's ERC treats a
        // no-connect marker on that hidden pin as connected to an unrelated
        // power-symbol anchor. It is not a usable registry pin, so leave it
        // unannotated; hidden passive pins do not produce a floating-pin ERC
        // diagnostic and must not receive a graphical no-connect marker.
        if pin.number == "50" && pin.name.eq_ignore_ascii_case("DVDD") {
            continue;
        }
        // Only *input* power legs are fan-out candidates. An
        // undeclared power_output leg (e.g. an RP2350's VREG_LX/
        // VREG_VOUT, which drive a buck) must NOT be tied onto the
        // rail: it would short two power outputs together and trip
        // KiCad's `pin_to_pin` check. It also shouldn't be left
        // flagged as floating; report it as no-connect so the DRC/
        // ERC knows it's intentionally not used.
        let is_connectable_power = matches!(
            pin.electrical_type.as_str(),
            "power_in" | "power_input" | "passive"
        );
        if is_connectable_power {
            if let Some(family) = RailFamily::for_pin(&pin.electrical_type, &pin.name) {
                if let Some(candidates) = rail_nets.get(&family) {
                    if let Some(net) = pick_rail_net(board, candidates, &pin.name) {
                        out.power_legs.push(PowerLeg {
                            component: component.id,
                            pin_number: pin.number.clone(),
                            pin_name: pin.name.clone(),
                            electrical_type: pin.electrical_type.clone(),
                            net,
                            family,
                        });
                        continue;
                    }
                }
            }
        }
        out.no_connects.push(NoConnectPin {
            component: component.id,
            pin_number: pin.number.clone(),
            pin_name: pin.name.clone(),
        });
    }

    out
}

/// Compute the terminal `(x, y, dx, dy)` of a physical pin on a
/// symbol, given the component's center (mm) and total rotation
/// degrees. `dx`,`dy` is the outward stub direction, matching
/// [`crate::route::pin_terminal_xy`].
///
/// Returns `None` when the pin can't be located on the symbol.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn physical_terminal(
    lib_id: &str,
    pin_number: &str,
    center: (f64, f64),
    total_deg: f64,
) -> Option<(f64, f64, f64, f64)> {
    let pos = crate::kicad_lib_loader::pin_positions(lib_id)?;
    let (px, py, pin_angle) = pos.get(pin_number)?;
    // A multi-unit symbol draws its units as a stack, and the pin belongs to one
    // of them: shift it by that unit's slot, as the router does for wire ends,
    // or a marker lands on the pin of unit 1 that shares its coordinates.
    let (ux, uy) = crate::kicad_lib_loader::pin_unit_offset(lib_id, pin_number);
    let (px, py) = (px + ux, py - uy);
    let rad = total_deg.to_radians();
    let rot_x = px * rad.cos() - py * rad.sin();
    let rot_y = px * rad.sin() + py * rad.cos();
    // Snap the centre before rotating AND the terminal after,
    // exactly as `crate::route::pin_terminal_xy` does for
    // routed wire endpoints. Without this, reconcile-path artifacts
    // (fan-out power symbols, `no_connect` markers, `PWR_FLAG`
    // stubs) sit at the raw rotated position while wires land on the
    // snapped one — up to 0.635 mm apart for off-grid stock pins,
    // which KiCad reports as dangling-wire/off-grid warnings.
    let cx = snap_grid_127(center.0);
    let cy = snap_grid_127(center.1);
    let term_x = snap_grid_127(cx + rot_x);
    let term_y = snap_grid_127(cy - rot_y);
    // KiCad's pin `angle` points INWARD; the sheet is y-down, so the
    // outward stub direction is (-cos θ, +sin θ) with θ = pin_angle +
    // total_deg — mirroring `crate::route::pin_terminal_xy`.
    let dir_deg = (pin_angle + total_deg).rem_euclid(360.0);
    let dir_rad = dir_deg.to_radians();
    let dx = -dir_rad.cos().round();
    let dy = dir_rad.sin().round();
    Some((term_x, term_y, dx, dy))
}

/// A component's center on the sheet, snapped to the grid, and its total
/// rotation in degrees: the placement turn plus the symbol's natural one.
fn symbol_pose(part: &synth_registry::Part, placement: &ComponentPlacement) -> ((f64, f64), f64) {
    let center = (
        snap_grid_127(placement.center_mm.0),
        snap_grid_127(placement.center_mm.1),
    );
    let logical_deg = match placement.rotation {
        Rotation::Zero => 0.0,
        Rotation::Ninety => 90.0,
        Rotation::OneEighty => 180.0,
        Rotation::TwoSeventy => 270.0,
    };
    (
        center,
        (logical_deg + natural_rotation_offset(part)).rem_euclid(360.0),
    )
}

/// Where a fanned power leg's stub runs: from the pin terminal to the point
/// its power symbol sits at.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn fanned_leg_stub(
    board: &Board,
    leg: &PowerLeg,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<((f64, f64), (f64, f64))> {
    let component = board.component(leg.component)?;
    let part = component.part.as_ref()?;
    let lib_id = part.kicad_symbol.as_deref()?;
    let placement = placements.get(&component.id)?;
    let (center, total_deg) = symbol_pose(part, placement);
    let (x, y, dx, dy) = physical_terminal(lib_id, &leg.pin_number, center, total_deg)?;
    // Offset the symbol one grid unit outward from the pin so it
    // sits clear of the body.
    Some(((x, y), (x + dx * POWER_STUB_LEN, y + dy * POWER_STUB_LEN)))
}

/// Where the no-connect marker of `pin` sits: its terminal on the referenced
/// symbol, or on the synthesized rectangle for a part with no `kicad_symbol`.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn no_connect_position(
    board: &Board,
    pin: &NoConnectPin,
    placements: &HashMap<ComponentId, &ComponentPlacement>,
) -> Option<(f64, f64)> {
    let component = board.component(pin.component)?;
    let part = component.part.as_ref()?;
    let placement = placements.get(&component.id)?;
    let (center, total_deg) = symbol_pose(part, placement);
    if let Some(lib_id) = part.kicad_symbol.as_deref() {
        return physical_terminal(lib_id, &pin.pin_number, center, total_deg)
            .map(|(x, y, _, _)| (x, y));
    }
    let pin_idx = part
        .pins
        .iter()
        .position(|p| p.number.0 == pin.pin_number)
        .map(|i| i as u32)?;
    let (x, y, _dx, _dy) =
        pin_terminal_xy(board, component.id, synth_ir::PinId(pin_idx), placements)?;
    Some((snap_grid_127(x), snap_grid_127(y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A multi-unit symbol draws its units as a stack, so the terminal of a pin of
    /// unit 2 sits one unit pitch below where its unit-local coordinates put it.
    /// The no-connect marker and the fanned power legs use this terminal, and a
    /// marker left at the unit-local spot lands on a pin of unit 1 instead.
    #[test]
    fn a_pin_of_a_later_unit_is_shifted_by_its_units_slot() {
        let lib = "Amplifier_Operational:LM358";
        // Skipped when KiCad's bundled libraries are not installed.
        let Some(positions) = crate::kicad_lib_loader::pin_positions(lib) else {
            return;
        };
        let (_, local_y, _) = positions["7"];
        let center = (100.0, 100.0);
        let (_, y1, _, _) = physical_terminal(lib, "1", center, 0.0).expect("pin 1");
        let (_, y7, _, _) = physical_terminal(lib, "7", center, 0.0).expect("pin 7");
        let (_, local_y1, _) = positions["1"];
        let unshifted = snap_grid_127(100.0 - local_y);
        assert!(
            (y7 - unshifted - crate::kicad_lib_loader::UNIT_PITCH_MM).abs() < 0.2,
            "pin 7 (unit 2) should sit one unit pitch below {unshifted}, found {y7}"
        );
        assert!(
            (y1 - snap_grid_127(100.0 - local_y1)).abs() < 1e-9,
            "pin 1 (unit 1) is not shifted"
        );
    }
}
