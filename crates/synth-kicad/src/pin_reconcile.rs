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

use crate::sexp::{num, Sexp};
use synth_layout::kicad_lib_loader::PhysicalPin;
use synth_layout::route::snap_grid_127;

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
}

/// Result of reconciling one component's symbol pin set against the
/// netlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReconciledPins {
    pub power_legs: Vec<PowerLeg>,
    pub no_connects: Vec<NoConnectPin>,
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
    placements: &std::collections::HashMap<ComponentId, &synth_layout::ComponentPlacement, S>,
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
                if let Some(physical) = synth_layout::kicad_lib_loader::physical_pins(lib_id)
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
            // the anchor for the flag's stub.
            let mut anchor = None;
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
                let Some(placement) = placements.get(&endpoint.component) else {
                    continue;
                };
                anchor = Some(PowerDriver {
                    net: net.id,
                    anchor_component: endpoint.component,
                    anchor_pin_number: pin.number.0.clone(),
                    anchor_pin_name: pin.name.clone(),
                    anchor_center: placement.center_mm,
                });
                break;
            }
            if let Some(anchor) = anchor {
                out.push(anchor);
            }
        }
    }
    out
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
    let physical: Vec<synth_layout::kicad_lib_loader::PhysicalPin> = match lib_id {
        Some(lid) => match synth_layout::kicad_lib_loader::physical_pins(lid) {
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
    // Rail nets this component touches, keyed by family.
    let mut rail_nets: HashMap<RailFamily, NetId> = HashMap::new();

    for net in &board.nets {
        for endpoint in &net.endpoints {
            if endpoint.component != component.id {
                continue;
            }
            let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                continue;
            };
            netlisted.insert(pin.number.0.clone());
            if let Some(family) = RailFamily::for_pin(
                match pin.electrical_type {
                    synth_registry::ElectricalType::PowerOutput => "power_out",
                    synth_registry::ElectricalType::PowerInput
                    | synth_registry::ElectricalType::GroundReference => "power_in",
                    _ => "non_power",
                },
                &pin.name,
            ) {
                rail_nets.entry(family).or_insert(net.id);
            }
        }
    }

    for pin in &physical {
        if netlisted.contains(&pin.number) {
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
                if let Some(net) = rail_nets.get(&family) {
                    out.power_legs.push(PowerLeg {
                        component: component.id,
                        pin_number: pin.number.clone(),
                        pin_name: pin.name.clone(),
                        electrical_type: pin.electrical_type.clone(),
                        net: *net,
                        family,
                    });
                    continue;
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
/// [`synth_layout::route::pin_terminal_xy`].
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
    let pos = synth_layout::kicad_lib_loader::pin_positions(lib_id)?;
    let (px, py, pin_angle) = pos.get(pin_number)?;
    let rad = total_deg.to_radians();
    let rot_x = px * rad.cos() - py * rad.sin();
    let rot_y = px * rad.sin() + py * rad.cos();
    // Snap the centre before rotating AND the terminal after,
    // exactly as `synth_layout::route::pin_terminal_xy` does for
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
    // total_deg — mirroring `synth_layout::route::pin_terminal_xy`.
    let dir_deg = (pin_angle + total_deg).rem_euclid(360.0);
    let dir_rad = dir_deg.to_radians();
    let dx = -dir_rad.cos().round();
    let dy = dir_rad.sin().round();
    Some((term_x, term_y, dx, dy))
}

/// Render a `(no_connect (at x y) (uuid ...))` s-expression for a pin
/// terminal. `project` namespaces a deterministic uuid from `key`.
#[must_use]
pub fn no_connect_sexp(x: f64, y: f64, uuid: &str) -> Sexp {
    Sexp::list(
        "no_connect",
        vec![
            Sexp::list("at", vec![num(x), num(y)]),
            Sexp::list("uuid", vec![Sexp::str(uuid)]),
        ],
    )
}
