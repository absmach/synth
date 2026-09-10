// SPDX-License-Identifier: Apache-2.0

//! Embedded `.kicad_sym` library generator.
//!
//! For each unique [`Part`] referenced by the IR, we synthesize a
//! single symbol entry: a rectangular body with pins placed by side
//! per [`classify_ic_pin`] — ground/power on top or bottom, clock/
//! reset/boot/RF pins and data outputs on the right, everything else
//! (inputs, bidirectional, passive, ...) on the left. The body is
//! sized to fit `2.54 mm` pin pitch on whichever side has the most
//! pins.
//!
//! Each pin is rendered with a KiCad electrical type derived from
//! the registry [`ElectricalType`] enum (see [`map_electrical_type`]).
//!
//! Remaining V1 layout limitation: rectangles only, no per-capability
//! grouping *within* a side (e.g. all GPIOs on the left render in
//! declaration order, not clustered by peripheral).

// KiCad coordinates are millimetres in `f64`. usize/u32 → f64 casts
// at the export boundary are intentional and well-bounded by pin
// counts and column indices.
#![allow(clippy::cast_precision_loss, clippy::cast_lossless)]

use std::collections::BTreeMap;

use synth_ir::Board;
use synth_layout::PinSide;
use synth_registry::{ElectricalType, Part};

use synth_layout::kicad_lib_loader;

use crate::sexp::{num, pair, str_pair, Sexp};

/// Width of the symbol body in millimetres. Half-widths land at
/// ±`BODY_HALF_WIDTH`.
const BODY_HALF_WIDTH: f64 = 7.62; // 0.3" — KiCad symbol convention.

/// Spacing between pins along the body's left edge.
const PIN_PITCH: f64 = 2.54;

/// How far the pin tip extends from the body. `length = 2.54 mm`
/// puts the pin tip at `x = -10.16 mm`.
const PIN_LENGTH: f64 = 2.54;

/// Minimum body height regardless of pin count (so single-pin
/// stubs are still readable).
const MIN_BODY_HEIGHT: f64 = 10.16;

const BODY_PIN_PADDING: f64 = 2.54;
const TWOPIN_HALF_W: f64 = 5.08;

/// Library identifier. The schematic file references symbols as
/// `<LIBRARY_NICKNAME>:<part_id>`.
pub const LIBRARY_NICKNAME: &str = "synth";

/// Build the full library s-expression for a board's parts.
///
/// Parts are emitted in lexicographic order of part id; embedded
/// power symbols (one per unique flag label) follow in
/// lexicographic order of label so the output is deterministic
/// across runs.
/// Load a single stock KiCad symbol's raw s-expression text by
/// `lib_id` (e.g. `"power:PWR_FLAG"`), suitable for direct embedding
/// into a `lib_symbols` block. Returns `None` when KiCad's bundled
/// library isn't available or the symbol can't be found.
#[must_use]
pub fn load_stock_symbol(lib_id: &str) -> Option<String> {
    kicad_lib_loader::load_symbol(lib_id)
}

pub fn build_library(board: &Board, layout: &synth_layout::Layout) -> Sexp {
    let unique = unique_parts(board);
    let mut children = vec![
        pair("version", Sexp::atom("20251024")),
        str_pair("generator", "synth-eda"),
        str_pair("generator_version", "10.0"),
    ];
    // Track which stock `lib_id`s we've already embedded so multiple
    // registry parts mapping to the same KiCad symbol
    // (e.g. led_red / led_green / led_blue → "Device:LED") don't
    // emit duplicate definitions.
    let mut stock_seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for part in unique.values() {
        match part.kicad_symbol.as_deref() {
            Some(lib_id) => {
                if !stock_seen.insert(lib_id.to_string()) {
                    continue;
                }
                // Loaded from the user's KiCad install when present;
                // otherwise (CI / no KiCad) fall back to a
                // synthesized rectangle so the export still opens.
                if let Some(text) = kicad_lib_loader::load_symbol(lib_id) {
                    children.push(Sexp::Raw(text));
                } else {
                    children.push(build_symbol_for_lib_id(part, lib_id));
                }
            }
            None => children.push(build_symbol(part)),
        }
    }
    // Embedded power-flag symbols. Prefer stock KiCad `power:`
    // artwork when available; fall back to synthesized `synth:`
    // symbols so the schematic still opens on machines without
    // KiCad installed.
    let mut power_labels: BTreeMap<String, synth_layout::PowerFlagKind> = BTreeMap::new();
    for flag in &layout.power_flags {
        power_labels.entry(flag.label.clone()).or_insert(flag.kind);
    }
    for (label, kind) in &power_labels {
        let power_lib_id = format!("power:{label}");
        if let Some(text) = synth_layout::kicad_lib_loader::load_symbol(&power_lib_id) {
            children.push(Sexp::Raw(text));
        } else {
            // Publish the fallback definition under the same name the
            // *instances* reference: `power_symbol_lib_id` emits
            // `synth:<label>` whenever the stock `power:` library
            // lacks the symbol (e.g. BAT, OUT), so embedding it as
            // `power:<label>` would leave the instances unresolvable
            // and KiCad ERC would report their pins as unconnected.
            let synth_lib_id = format!("{LIBRARY_NICKNAME}:{label}");
            let mut def = build_power_symbol_def(label, *kind, "power_in");
            rename_symbol_outer(&mut def, &synth_lib_id);
            children.push(def);
        }
    }
    Sexp::list("kicad_symbol_lib", children)
}

/// Build a synthesized symbol whose embedded outer name matches
/// `lib_id` (e.g. `"Device:R"`) rather than our own
/// `<NICKNAME>:<part_id>` namespace.
///
/// KiCad resolves every `(lib_id "X:Y")` instance against the
/// schematic's `(lib_symbols ...)` block by exact name. When the
/// stock artwork can't be loaded (no KiCad install / CI), instances
/// still reference the registry's stock lib_id, so the fallback
/// definition must be published under that same name — embedding it
/// as `synth:<part_id>` would leave every part an unresolved `?`
/// placeholder.
#[must_use]
pub fn build_symbol_for_lib_id(part: &Part, lib_id: &str) -> Sexp {
    let mut sym = build_symbol(part);
    rename_symbol_outer(&mut sym, lib_id);
    sym
}

/// Synthesized stand-in for KiCad's stock `power:PWR_FLAG`, used
/// when the bundled `power` library isn't available to embed the
/// real definition. Carries a `power_out` pin — that electrical type
/// is the flag's entire ERC purpose (satisfying
/// `power_pin_not_driven`) — unlike synthesized rail arrows, whose
/// single pin is `power_in`.
#[must_use]
pub fn build_pwr_flag_fallback() -> Sexp {
    let mut def = build_power_symbol_def("PWR_FLAG", synth_layout::PowerFlagKind::Vcc, "power_out");
    rename_symbol_outer(&mut def, "power:PWR_FLAG");
    def
}

/// Rewrite the outer symbol name of a synthesized definition.
///
/// KiCad resolves unit sub-symbols by the `<symbolname>_<unit>_<body>`
/// convention, where `<symbolname>` is the part of the lib_id after
/// the colon. A synthesized part whose unit names derive from the
/// registry value (e.g. `osc_smd_25mhz_0_1`) must be re-keyed to the
/// renamed wrapper (e.g. `Oscillator:SiT8008B` -> `SiT8008B_0_1`) or
/// KiCad refuses to load the schematic. Power symbols are unaffected:
/// their units already use the label, which equals the name after the
/// colon for both `power:<label>` and `synth:<label>`.
fn rename_symbol_outer(sym: &mut Sexp, new_name: &str) {
    if let Sexp::List { head, children } = sym {
        if head.as_str() == "symbol" && !children.is_empty() {
            children[0] = Sexp::str(new_name);
            let symbol_name = new_name.rsplit(':').next().unwrap_or(new_name);
            for child in children.iter_mut().skip(1) {
                if let Sexp::List {
                    head: sub,
                    children: sub_children,
                } = child
                {
                    if sub.as_str() == "symbol" && !sub_children.is_empty() {
                        if let Sexp::Str(name) = &sub_children[0] {
                            if let Some(unit_body) = unit_body_suffix(name) {
                                sub_children[0] = Sexp::str(format!("{symbol_name}{unit_body}"));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Return the trailing `_<unit>_<body>` suffix of a unit sub-symbol
/// name (e.g. `_0_1`, `_1_1`, `_0_0`) if `name` has the shape
/// `<prefix>_<digit>_<digit>`.
fn unit_body_suffix(name: &str) -> Option<&str> {
    let bytes = name.as_bytes();
    let n = bytes.len();
    if n < 5 {
        return None;
    }
    let is_digit = |b: u8| b.is_ascii_digit();
    if !(bytes[n - 1].is_ascii_digit()
        && bytes[n - 2] == b'_'
        && is_digit(bytes[n - 3])
        && bytes[n - 4] == b'_')
    {
        return None;
    }
    Some(&name[n - 4..])
}

/// Emit one embedded power-flag symbol (e.g. `synth:GND`,
/// `synth:VBUS`). The graphic is a small triangle (GND) or upward
/// arrow (VCC) at the pin tip; reference designator follows
/// KiCad's `#PWR` convention so the symbol is treated as a power
/// flag and not promoted into the BOM.
///
/// `pin_electrical` selects the single pin's KiCad electrical type:
/// rail arrows are `power_in`; the PWR_FLAG stand-in is `power_out`
/// (see [`build_pwr_flag_fallback`]).
#[allow(clippy::too_many_lines)]
fn build_power_symbol_def(
    label: &str,
    kind: synth_layout::PowerFlagKind,
    pin_electrical: &str,
) -> Sexp {
    let lib_name = format!("{LIBRARY_NICKNAME}:{label}");
    // Graphic: GND triangle points down (from origin to y=-2.54),
    // VCC arrow points up (origin to y=+2.54). Pin tip sits at the
    // origin so the symbol's `(at x y)` in the schematic is the
    // pin connection point.
    let graphic = match kind {
        synth_layout::PowerFlagKind::Gnd => Sexp::list(
            "polyline",
            vec![
                Sexp::list(
                    "pts",
                    vec![
                        Sexp::list("xy", vec![num(0.0), num(0.0)]),
                        Sexp::list("xy", vec![num(0.0), num(-1.27)]),
                        Sexp::list("xy", vec![num(1.27), num(-1.27)]),
                        Sexp::list("xy", vec![num(0.0), num(-2.54)]),
                        Sexp::list("xy", vec![num(-1.27), num(-1.27)]),
                        Sexp::list("xy", vec![num(0.0), num(-1.27)]),
                    ],
                ),
                Sexp::list(
                    "stroke",
                    vec![
                        Sexp::list("width", vec![num(0.0)]),
                        Sexp::list("type", vec![Sexp::atom("default")]),
                    ],
                ),
                Sexp::list("fill", vec![Sexp::list("type", vec![Sexp::atom("none")])]),
            ],
        ),
        synth_layout::PowerFlagKind::Vcc => Sexp::list(
            "polyline",
            vec![
                Sexp::list(
                    "pts",
                    vec![
                        Sexp::list("xy", vec![num(-0.762), num(1.27)]),
                        Sexp::list("xy", vec![num(0.0), num(2.54)]),
                        Sexp::list("xy", vec![num(0.762), num(1.27)]),
                        Sexp::list("xy", vec![num(0.0), num(0.0)]),
                        Sexp::list("xy", vec![num(0.0), num(2.54)]),
                    ],
                ),
                Sexp::list(
                    "stroke",
                    vec![
                        Sexp::list("width", vec![num(0.0)]),
                        Sexp::list("type", vec![Sexp::atom("default")]),
                    ],
                ),
                Sexp::list("fill", vec![Sexp::list("type", vec![Sexp::atom("none")])]),
            ],
        ),
    };
    // Pin direction: GND faces down (angle 270), VCC faces up
    // (angle 90). The pin has length 0 because its connection
    // point sits at the symbol origin.
    let pin_angle = match kind {
        synth_layout::PowerFlagKind::Gnd => 270.0,
        synth_layout::PowerFlagKind::Vcc => 90.0,
    };
    let value_y = match kind {
        synth_layout::PowerFlagKind::Gnd => -3.81,
        synth_layout::PowerFlagKind::Vcc => 3.81,
    };
    let pin = Sexp::list(
        "pin",
        vec![
            Sexp::atom(pin_electrical),
            Sexp::atom("line"),
            Sexp::list("at", vec![num(0.0), num(0.0), num(pin_angle)]),
            Sexp::list("length", vec![num(0.0)]),
            Sexp::list(
                "name",
                vec![
                    Sexp::str(""),
                    Sexp::list(
                        "effects",
                        vec![Sexp::list(
                            "font",
                            vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                        )],
                    ),
                ],
            ),
            Sexp::list(
                "number",
                vec![
                    Sexp::str("1"),
                    Sexp::list(
                        "effects",
                        vec![Sexp::list(
                            "font",
                            vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                        )],
                    ),
                ],
            ),
        ],
    );
    let body_sym = Sexp::list("symbol", vec![Sexp::str(format!("{label}_0_1")), graphic]);
    let pin_sym = Sexp::list("symbol", vec![Sexp::str(format!("{label}_1_1")), pin]);

    Sexp::list(
        "symbol",
        vec![
            Sexp::str(lib_name),
            Sexp::list("power", vec![Sexp::atom("global")]),
            Sexp::list(
                "pin_numbers",
                vec![Sexp::list("hide", vec![Sexp::atom("yes")])],
            ),
            Sexp::list(
                "pin_names",
                vec![
                    Sexp::list("offset", vec![num(0.0)]),
                    Sexp::list("hide", vec![Sexp::atom("yes")]),
                ],
            ),
            Sexp::list("in_bom", vec![Sexp::atom("no")]),
            Sexp::list("on_board", vec![Sexp::atom("no")]),
            property("Reference", "#PWR", 0.0, -6.35, true),
            property("Value", label, 0.0, value_y, false),
            property("Footprint", "", 0.0, 0.0, true),
            property("Datasheet", "", 0.0, 0.0, true),
            body_sym,
            pin_sym,
        ],
    )
}

fn unique_parts(board: &Board) -> BTreeMap<String, Part> {
    let mut out = BTreeMap::new();
    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            out.entry(part.id.as_str().to_string())
                .or_insert_with(|| part.clone());
        }
    }
    out
}

fn is_two_pin_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "resistor" | "capacitor" | "inductor" | "diode" | "led" | "crystal" | "switch"
    )
}

/// Classify a pin's schematic-body side for the synthesized symbol
/// fallback. Order matters: ground/power placement and clock/reset/
/// boot/RF clustering take priority over plain input/output
/// direction, since those are the stronger, more universal EDA
/// conventions (rails top/bottom; control/clock pins grouped away
/// from the data bus).
///
/// This classifier is kept in lockstep with
/// `synth_layout::route::classify_ic_pin` and
/// `synth_layout::classify_ic_pin_layout` — those copies drive wire
/// terminal placement and body sizing, so a disagreement would leave
/// wires landing on a body edge with no drawn pin. All three
/// implement the same schematic convention (ProtoExpress "Schematic
/// Design Rules": inputs on the left, outputs on the right). Deduping
/// them into one shared function is tracked as follow-up cleanup.
fn classify_ic_pin(pin: &synth_registry::Pin) -> PinSide {
    use synth_registry::{ElectricalType, PinCapability};
    let lower = pin.name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "gnda" | "vee" | "vneg" | "agnd" | "dgnd"
    ) {
        return PinSide::Bottom;
    }
    if matches!(
        pin.electrical_type,
        ElectricalType::PowerInput | ElectricalType::PowerOutput | ElectricalType::GroundReference
    ) {
        return PinSide::Top;
    }
    if pin.capabilities.iter().any(|c| {
        matches!(
            c,
            PinCapability::Reset
                | PinCapability::BootMode
                | PinCapability::ClockInput
                | PinCapability::ClockOutput
                | PinCapability::RfFeed
        )
    }) {
        return PinSide::Right;
    }
    // Plain data direction: outputs to the right, inputs (and
    // bidirectional/passive/analog/... — anything without a clearer
    // signal) stay left. This is the convention IC datasheets and
    // most hand-drawn schematics already follow.
    if pin.electrical_type == ElectricalType::Output {
        return PinSide::Right;
    }
    PinSide::Left
}

fn build_symbol(part: &Part) -> Sexp {
    let pin_count = part.pins.len();
    let part_id = part.id.as_str();

    let (body_w, body_h, is_two_pin) = if pin_count == 2 && is_two_pin_symbol_kind(&part.kind) {
        (TWOPIN_HALF_W * 2.0, 4.0, true)
    } else {
        let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
        let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
        let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
        let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
        let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();

        let horiz_max = top_n.max(bottom_n).max(2);
        let vert_max = left_n.max(right_n).max(2);
        let corner_pad = if left_n > 0 || right_n > 0 {
            PIN_PITCH * 2.0
        } else {
            BODY_PIN_PADDING
        };
        let w = ((horiz_max as f64) * PIN_PITCH + 2.0 * corner_pad).max(BODY_HALF_WIDTH * 2.0);
        let h = ((vert_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING).max(MIN_BODY_HEIGHT);
        (w, h, false)
    };

    let top = body_h / 2.0;
    let bottom = -body_h / 2.0;
    let left = -body_w / 2.0;
    let right = body_w / 2.0;

    let (ref_x, ref_y, val_x, val_y) = if is_two_pin {
        (right + 2.54, -2.54, right + 2.54, 2.54)
    } else {
        (0.0, top + PIN_PITCH, 0.0, top + (PIN_PITCH * 2.0))
    };

    // Pin numbers stay visible: KiCad draws them outside the symbol
    // graphic at the pin's outer end (the standard convention —
    // ProtoExpress "Schematic Design Rules" requires pin numbers on
    // the outside of the symbol graphic). Power-flag symbols keep
    // theirs hidden in `build_power_symbol_def`.
    let mut children = vec![
        Sexp::list("pin_names", vec![Sexp::list("offset", vec![num(0.508)])]),
        Sexp::list("in_bom", vec![Sexp::atom("yes")]),
        Sexp::list("on_board", vec![Sexp::atom("yes")]),
        property(
            "Reference",
            default_reference_prefix(part),
            ref_x,
            ref_y,
            false,
        ),
        property("Value", part_id, val_x, val_y, false),
        property(
            "Footprint",
            part.kicad_footprint.as_deref().unwrap_or(""),
            0.0,
            0.0,
            true,
        ),
        property(
            "Datasheet",
            part.provenance
                .as_ref()
                .and_then(|p| p.datasheet_url.as_deref())
                .unwrap_or(""),
            0.0,
            0.0,
            true,
        ),
        // Graphic body — units-independent sub-symbol.
        Sexp::list(
            "symbol",
            vec![
                Sexp::str(format!("{part_id}_0_1")),
                Sexp::list(
                    "rectangle",
                    vec![
                        Sexp::list("start", vec![num(left), num(bottom)]),
                        Sexp::list("end", vec![num(right), num(top)]),
                        Sexp::list(
                            "stroke",
                            vec![
                                Sexp::list("width", vec![num(0.254)]),
                                Sexp::list("type", vec![Sexp::atom("default")]),
                            ],
                        ),
                        Sexp::list(
                            "fill",
                            vec![Sexp::list("type", vec![Sexp::atom("background")])],
                        ),
                    ],
                ),
            ],
        ),
        // Pins sub-symbol.
        build_pins_subsymbol(part_id, &part.pins, body_w, body_h, is_two_pin),
    ];

    // KiCad expects the outer (symbol "lib:id" ...) form where the
    // name includes the library nickname — that's the key instances
    // in the schematic reference via `(lib_id "synth:<id>")`. Bare
    // `<id>` here makes KiCad fail to resolve the instance and
    // render `?` placeholders. Sub-symbols (`*_0_1`, `*_1_1`) keep
    // the bare `<id>` prefix — that's KiCad's internal convention.
    let mut outer = vec![Sexp::str(format!("{LIBRARY_NICKNAME}:{part_id}"))];
    outer.append(&mut children);
    Sexp::list("symbol", outer)
}

#[allow(clippy::too_many_lines)]
fn build_pins_subsymbol(
    part_id: &str,
    pins: &[synth_registry::Pin],
    body_w: f64,
    body_h: f64,
    is_two_pin: bool,
) -> Sexp {
    let mut children = vec![Sexp::str(format!("{part_id}_1_1"))];

    if is_two_pin {
        for (i, pin) in pins.iter().enumerate() {
            let (x, y, angle) = if i == 0 {
                (-TWOPIN_HALF_W - PIN_LENGTH, 0.0, 0)
            } else {
                (TWOPIN_HALF_W + PIN_LENGTH, 0.0, 180)
            };
            let kicad_type = map_electrical_type(pin.electrical_type);
            children.push(Sexp::list(
                "pin",
                vec![
                    Sexp::atom(kicad_type),
                    Sexp::atom("line"),
                    Sexp::list("at", vec![num(x), num(y), num(angle as f64)]),
                    Sexp::list("length", vec![num(PIN_LENGTH)]),
                    Sexp::list(
                        "name",
                        vec![
                            Sexp::str(&pin.name),
                            Sexp::list(
                                "effects",
                                vec![Sexp::list(
                                    "font",
                                    vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                                )],
                            ),
                        ],
                    ),
                    Sexp::list(
                        "number",
                        vec![
                            Sexp::str(&pin.number.0),
                            Sexp::list(
                                "effects",
                                vec![Sexp::list(
                                    "font",
                                    vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                                )],
                            ),
                        ],
                    ),
                ],
            ));
        }
    } else {
        let sides: Vec<PinSide> = pins.iter().map(classify_ic_pin).collect();
        let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
        let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();
        let corner_pad = if left_n > 0 || right_n > 0 {
            PIN_PITCH * 2.0
        } else {
            BODY_PIN_PADDING
        };

        let bx = -body_w / 2.0;
        let by_top = body_h / 2.0;
        let by_bottom = -body_h / 2.0;

        let mut top_idx = 0_usize;
        let mut bottom_idx = 0_usize;
        let mut left_idx = 0_usize;
        let mut right_idx = 0_usize;

        for (i, pin) in pins.iter().enumerate() {
            let side = sides[i];
            let (x, y, angle) = match side {
                PinSide::Top => {
                    let px = bx + corner_pad + PIN_PITCH * (top_idx as f64);
                    let py = by_top + PIN_LENGTH;
                    top_idx += 1;
                    (px, py, 270)
                }
                PinSide::Bottom => {
                    let px = bx + corner_pad + PIN_PITCH * (bottom_idx as f64);
                    let py = by_bottom - PIN_LENGTH;
                    bottom_idx += 1;
                    (px, py, 90)
                }
                PinSide::Left => {
                    let px = bx - PIN_LENGTH;
                    let py = by_top - BODY_PIN_PADDING - PIN_PITCH * (left_idx as f64);
                    left_idx += 1;
                    (px, py, 0)
                }
                PinSide::Right => {
                    let px = -bx + PIN_LENGTH;
                    let py = by_top - BODY_PIN_PADDING - PIN_PITCH * (right_idx as f64);
                    right_idx += 1;
                    (px, py, 180)
                }
            };

            let kicad_type = map_electrical_type(pin.electrical_type);
            children.push(Sexp::list(
                "pin",
                vec![
                    Sexp::atom(kicad_type),
                    Sexp::atom("line"),
                    Sexp::list("at", vec![num(x), num(y), num(angle as f64)]),
                    Sexp::list("length", vec![num(PIN_LENGTH)]),
                    Sexp::list(
                        "name",
                        vec![
                            Sexp::str(&pin.name),
                            Sexp::list(
                                "effects",
                                vec![Sexp::list(
                                    "font",
                                    vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                                )],
                            ),
                        ],
                    ),
                    Sexp::list(
                        "number",
                        vec![
                            Sexp::str(&pin.number.0),
                            Sexp::list(
                                "effects",
                                vec![Sexp::list(
                                    "font",
                                    vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
                                )],
                            ),
                        ],
                    ),
                ],
            ));
        }
    }
    Sexp::list("symbol", children)
}

fn property(name: &str, value: &str, x: f64, y: f64, hide: bool) -> Sexp {
    let mut effects = vec![Sexp::list(
        "font",
        vec![Sexp::list("size", vec![num(1.27), num(1.27)])],
    )];
    if hide {
        effects.push(Sexp::list("hide", vec![Sexp::atom("yes")]));
    }
    Sexp::list(
        "property",
        vec![
            Sexp::str(name),
            Sexp::str(value),
            Sexp::list("at", vec![num(x), num(y), num(0.0)]),
            Sexp::list("effects", effects),
        ],
    )
}

/// Maps a registry [`ElectricalType`] to the KiCad pin-type
/// keyword. Used in both the library and the schematic. Several
/// Synth types collapse onto KiCad's smaller vocabulary: analog/RF
/// pins are rendered as `passive` (no electrical-direction in KiCad
/// terms); differential pairs as `bidirectional`; open-drain variants
/// as `open_collector`; three-state as `tri_state`; clock as `clock`.
fn map_electrical_type(t: ElectricalType) -> &'static str {
    match t {
        ElectricalType::PowerOutput => "power_out",
        ElectricalType::PowerInput | ElectricalType::GroundReference => {
            "power_in" // Ground is power input in KiCad
        }
        ElectricalType::Input => "input",
        ElectricalType::Output => "output",
        ElectricalType::ThreeStatable => "tri_state",
        ElectricalType::OpenDrainLow => "open_collector",
        ElectricalType::OpenDrainHigh => "open_emitter",
        ElectricalType::DoNotConnect => "no_connect",
        ElectricalType::Bidirectional
        | ElectricalType::DifferentialPositive
        | ElectricalType::DifferentialNegative => "bidirectional",
        // KiCad has no `clock` pin type (its pin vocabulary is
        // input/output/bidirectional/tri_state/passive/open_*/power_*).
        // Map clock pins to `passive`, matching how analog/RF are
        // handled: KiCad performs no direction-driven ERC on them, so
        // no false positives, while Synth's own ERC still classifies
        // them as clock via the electrical type.
        // ElectricalType::Clock, Passive, Analog, Rf, Unclassified and any
        // future variants (non-exhaustive enum) all map to KiCad's
        // `passive`.
        _ => "passive",
    }
}

/// Default refdes prefix per common EDA convention; the user's
/// declared refdes (e.g. "U1") overrides this in the schematic.
fn default_reference_prefix(part: &Part) -> &'static str {
    match part.kind.as_str() {
        "capacitor" => "C",
        "resistor" => "R",
        "inductor" => "L",
        "diode" => "D",
        "connector" | "header" => "J",
        "crystal" | "oscillator" => "Y",
        // All active IC kinds ("mcu", "secure_element", "modem",
        // "regulator", "charger", and anything unknown) default to "U".
        _ => "U",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_registry::{Pin, PinCapability, PinNumber};

    fn pin(name: &str, electrical_type: ElectricalType, capabilities: Vec<PinCapability>) -> Pin {
        Pin {
            name: name.into(),
            number: PinNumber("1".into()),
            electrical_type,
            capabilities,
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    #[test]
    fn classify_ic_pin_routes_by_direction_then_side_conventions() {
        // Plain data direction: output right, input (and anything
        // else undirected) left.
        assert_eq!(
            classify_ic_pin(&pin("do", ElectricalType::Output, vec![])),
            PinSide::Right
        );
        assert_eq!(
            classify_ic_pin(&pin("di", ElectricalType::Input, vec![])),
            PinSide::Left
        );
        assert_eq!(
            classify_ic_pin(&pin("io", ElectricalType::Bidirectional, vec![])),
            PinSide::Left
        );

        // Ground/power rails always win, regardless of direction.
        assert_eq!(
            classify_ic_pin(&pin("gnd", ElectricalType::Output, vec![])),
            PinSide::Bottom
        );
        assert_eq!(
            classify_ic_pin(&pin("vdd", ElectricalType::PowerInput, vec![])),
            PinSide::Top
        );
        assert_eq!(
            classify_ic_pin(&pin("shield", ElectricalType::GroundReference, vec![])),
            PinSide::Top
        );

        // Clock/reset/boot/RF capability wins over plain direction too
        // (an input-typed reset pin still clusters on the right).
        assert_eq!(
            classify_ic_pin(&pin(
                "reset",
                ElectricalType::Input,
                vec![PinCapability::Reset]
            )),
            PinSide::Right
        );
    }

    fn make_part() -> Part {
        Part {
            id: synth_registry::PartId("test".into()),
            kind: "mcu".into(),
            version: 0,
            lifecycle: synth_registry::Lifecycle::Active,
            signed_by: vec![],
            substitutes: vec![],
            lcsc_pn: None,
            mpn: None,

            description: None,
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            pins: vec![
                Pin {
                    name: "p1".into(),
                    number: PinNumber("1".into()),
                    electrical_type: ElectricalType::Bidirectional,
                    capabilities: vec![],
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                Pin {
                    name: "p2".into(),
                    number: PinNumber("2".into()),
                    electrical_type: ElectricalType::PowerInput,
                    capabilities: vec![],
                    required: true,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: vec![],
            provenance: None,
        }
    }

    #[test]
    fn build_symbol_contains_correct_pin_count() {
        let s = build_symbol(&make_part());
        let rendered = s.to_string_pretty();
        // Count `(pin ` openings rather than full lines, because the
        // s-exp printer may render pins on one or several lines
        // depending on child complexity.
        let pin_openings = rendered.matches("(pin\n").count() + rendered.matches("(pin ").count();
        assert_eq!(pin_openings, 2);
    }

    #[test]
    fn electrical_type_mapping_covers_known_variants() {
        assert_eq!(map_electrical_type(ElectricalType::Passive), "passive");
        assert_eq!(map_electrical_type(ElectricalType::PowerInput), "power_in");
        assert_eq!(
            map_electrical_type(ElectricalType::Bidirectional),
            "bidirectional"
        );
        assert_eq!(
            map_electrical_type(ElectricalType::DoNotConnect),
            "no_connect"
        );
        assert_eq!(
            map_electrical_type(ElectricalType::GroundReference),
            "power_in"
        );
        assert_eq!(
            map_electrical_type(ElectricalType::ThreeStatable),
            "tri_state"
        );
        assert_eq!(
            map_electrical_type(ElectricalType::OpenDrainLow),
            "open_collector"
        );
        assert_eq!(
            map_electrical_type(ElectricalType::OpenDrainHigh),
            "open_emitter"
        );
        assert_eq!(map_electrical_type(ElectricalType::Clock), "passive");
        assert_eq!(
            map_electrical_type(ElectricalType::DoNotConnect),
            "no_connect"
        );
        assert_eq!(map_electrical_type(ElectricalType::Analog), "passive");
        assert_eq!(map_electrical_type(ElectricalType::Rf), "passive");
        assert_eq!(map_electrical_type(ElectricalType::Unclassified), "passive");
    }

    #[test]
    fn reference_prefix_by_kind() {
        let mut p = make_part();
        p.kind = "capacitor".into();
        assert_eq!(default_reference_prefix(&p), "C");
        p.kind = "resistor".into();
        assert_eq!(default_reference_prefix(&p), "R");
    }
}
