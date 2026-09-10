// SPDX-License-Identifier: Apache-2.0

//! Read-only SVG schematic renderer with drag, zoom, and pan.
//!
//! Layout: components are placed on a roughly-square auto grid so the
//! sheet looks more like a real schematic than a long ribbon. Wire
//! routing has two paths that trade correctness for responsiveness:
//!
//! - **Settled view** (no component is actively being dragged): wires
//!   come from the shared `synth_layout::route::route_board` router —
//!   the same A*-based orthogonal router `synth-kicad` uses, so the
//!   browser preview and the KiCad export agree. Component positions
//!   fed into it include any drag offsets left behind by a completed
//!   drag (see `apply_offsets_to_layout`).
//! - **Active component drag**: `route_board` runs full pathfinding
//!   per net and would otherwise be recomputed on every pointer-move
//!   event while dragging — a real, easy-to-miss performance risk.
//!   For just that render pass, wires fall back to a cheap local
//!   per-pair L-shape heuristic (`render_wires_for_net`/
//!   `l_route_points`) that recomputes instantly. The view upgrades
//!   back to the shared router the moment the drag ends.
//!
//! Sheet decoration follows KiCad's convention: a thick page border
//! and a bottom-right title block carrying the board name, source
//! file, and sheet info.
//!
//! ## Symbol rendering
//!
//! Two-pin parts (resistor, capacitor, inductor, diode, crystal,
//! switch) draw a horizontal IEC-style schematic symbol with pins
//! emerging on opposite sides — the way real schematics draw them.
//! Multi-pin parts (MCUs, sensors, regulators, ...) get the
//! rectangular IC body with all pins on the left edge.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    // `centre_x`/`centre_y`, `scale_x`/`scale_y`, etc. are intentional
    // 2D-pair names; renaming for clippy's taste would be worse.
    clippy::similar_names,
    // The Schematic + render_component closures are large because
    // they hold the whole reactive view; splitting prematurely loses
    // co-location of state and the JSX-like view tree.
    clippy::too_many_lines,
    // f64 midpoint compute is fine here — values are mm coordinates,
    // never near i64::MAX.
    clippy::manual_midpoint,
)]

use std::collections::HashMap;
use std::rc::Rc;

use leptos::ev::{PointerEvent, WheelEvent};
use leptos::prelude::*;
use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, PinId};
use synth_layout::route::{route_board, RouteResult};
use synth_layout::{Layout, NetLabel, PowerFlag, PowerFlagKind, Rotation};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{Element, SvgsvgElement};

use crate::state::BoardView;

/// Per-component layout info: centre (in mm) and rotation.
/// Built once per render from the [`Layout`] produced by
/// `synth_layout::layout`. The browser still applies its own
/// interactive drag offsets on top of the centre.
type Centres = HashMap<ComponentId, ((f64, f64), Rotation)>;

fn centres_from_layout(layout: &Layout) -> Centres {
    layout
        .components
        .iter()
        .map(|p| (p.id, (p.center_mm, p.rotation)))
        .collect()
}

/// Returns a copy of `layout` with each component's centre shifted by
/// its live drag offset (if any), leaving every other field
/// untouched.
///
/// This lets the settled (non-dragging) render path feed the *current*
/// on-screen positions — including offsets left behind by a completed
/// drag — into `synth_layout::route::route_board`, so wires route
/// against where components actually are instead of their original
/// `synth_layout::layout` placement. Pure and side-effect free (no
/// Leptos reactivity), so it's unit-testable without a browser; see
/// `tests::apply_offsets_shifts_only_the_offset_component` below.
///
/// Mirrors `synth_layout::sidecar::SidecarLayout::apply_to_layout`'s
/// idea of overlaying position overrides onto a base `Layout`, but
/// keyed by the browser's ephemeral `Offsets` map (`ComponentId.0 ->
/// (dx, dy)` delta) rather than a persisted refdes -> absolute
/// position map.
fn apply_offsets_to_layout(layout: &Layout, offsets: &Offsets) -> Layout {
    let mut adjusted = layout.clone();
    for placement in &mut adjusted.components {
        if let Some(&(dx, dy)) = offsets.get(&placement.id.0) {
            placement.center_mm.0 += dx;
            placement.center_mm.1 += dy;
        }
    }
    adjusted
}

/// Convert a [`Rotation`] to the sidecar schema's `rotation` degrees.
fn rotation_degrees(rotation: Rotation) -> u32 {
    match rotation {
        Rotation::Zero => 0,
        Rotation::Ninety => 90,
        Rotation::OneEighty => 180,
        Rotation::TwoSeventy => 270,
    }
}

/// Owned per-component base position snapshot captured into the
/// `'static` drag-end closure (Leptos view! closures are `'static`, so
/// we cannot move a borrowed `&Board`/`&Centres` into them). Holding
/// just the id + refdes + base centre + base rotation keeps the
/// captured payload small regardless of board size. The id keys the
/// drag-offset map (`ComponentId.0`); the refdes keys the sidecar.
type BasePositions = Vec<(u32, String, (f64, f64), Rotation)>;

/// Build the refdes-keyed sidecar payload from the base positions plus
/// the live drag offsets: `{ "components": { <refdes>: { x, y, rotation
/// } } }`, matching `<design>.synth.layout.toml` (§7.7.6). Positions
/// are *absolute* (base centre + drag delta), so the server can write
/// them straight into the sidecar and a reload reproduces the same
/// on-screen placement.
fn sidecar_save_payload(base: &BasePositions, offsets: &Offsets) -> serde_json::Value {
    let mut components = serde_json::Map::new();
    for (id, refdes, (bx, by), rotation) in base {
        let (dx, dy) = offsets.get(id).copied().unwrap_or((0.0, 0.0));
        components.insert(
            refdes.clone(),
            serde_json::json!({
                "x": bx + dx,
                "y": by + dy,
                "rotation": rotation_degrees(*rotation),
            }),
        );
    }
    serde_json::json!({ "components": components })
}

/// POST the current drag offsets to the preview server's
/// `POST /api/v1/layout/save` endpoint, which writes them to
/// `<design>.synth.layout.toml` on disk (§7.7.6). Fire-and-forget: a
/// failure only logs to the console and never breaks the live view.
/// The browser remains read-only with respect to the `.synth` source.
fn save_layout_to_sidecar(base: &BasePositions, offsets: &Offsets) {
    let payload = sidecar_save_payload(base, offsets);
    let Ok(body) = serde_json::to_string(&payload) else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_mode(web_sys::RequestMode::Cors);
    init.set_body(&JsValue::from_str(&body));
    if let Ok(headers) = web_sys::Headers::new() {
        let _ = headers.set("Content-Type", "application/json");
        init.set_headers(&headers);
    }
    let Ok(request) = web_sys::Request::new_with_str_and_init("/api/v1/layout/save", &init) else {
        return;
    };
    let _ = window.fetch_with_request(&request);
}

/// Lookup table from `(component, pin)` to the power flag that
/// should render at that pin (if any). Empty when no nets were
/// classified as power.
type FlagIndex = HashMap<(ComponentId, PinId), PowerFlag>;

fn flag_index_from_layout(layout: &Layout) -> FlagIndex {
    layout
        .power_flags
        .iter()
        .map(|f| ((f.component, f.pin), f.clone()))
        .collect()
}

/// Lookup table from `(component, pin)` to a net label (if any).
type NetLabelIndex = HashMap<(ComponentId, PinId), NetLabel>;

fn net_label_index_from_layout(layout: &Layout) -> NetLabelIndex {
    layout
        .net_labels
        .iter()
        .map(|l| ((l.component, l.pin), l.clone()))
        .collect()
}

// Layout constants -----------------------------------------------------------
//
// Component centres come from `synth_layout::layout` so the KiCad
// export and the browser preview agree on placement. The remaining
// constants below describe how pins, bodies, and wires render
// *around* a centre that's already been decided.

const PIN_PITCH: f64 = 2.54;
const BODY_HALF_WIDTH: f64 = 7.62;
const PIN_LENGTH: f64 = 2.54;
const MIN_BODY_HEIGHT: f64 = 10.16;
/// Padding above the topmost pin (and below the bottom pin) so pins
/// aren't flush with the body edge — matches KiCad convention.
const BODY_PIN_PADDING: f64 = 2.54;

/// Half-width of a 2-pin symbolic part (resistor/cap/diode/...) —
/// pins live at `cx ± TWOPIN_HALF_W` after the stub.
const TWOPIN_HALF_W: f64 = 3.81;

const MIN_VIEWBOX_W: f64 = 297.0;
const MIN_VIEWBOX_H: f64 = 210.0;

/// KiCad-style dot grid spacing: 0.1" = 2.54mm.
const GRID_PITCH: f64 = 2.54;

/// Number of per-net staggered routing "slots". Different nets get
/// different slots (by `net.id % WIRE_STAGGER_SLOTS`), pushing their
/// L-shape corners apart so two wires sharing a region don't draw
/// on top of each other.
const WIRE_STAGGER_SLOTS: u32 = 4;
/// Distance between adjacent stagger slots in mm.
const WIRE_STAGGER_STEP: f64 = 1.0;

const MIN_ZOOM: f64 = 0.2;
const MAX_ZOOM: f64 = 10.0;

// Sheet decoration ----------------------------------------------------------

/// Outer margin of the sheet border from the page edge.
const FRAME_MARGIN: f64 = 5.0;
/// Title-block dimensions, modelled on KiCad's A4 default.
const TITLE_BLOCK_W: f64 = 95.0;
const TITLE_BLOCK_H: f64 = 32.0;

type Offsets = HashMap<u32, (f64, f64)>;

#[derive(Clone, Copy, Debug)]
struct DragState {
    kind: DragKind,
    start_client_x: f64,
    start_client_y: f64,
    base_offset: (f64, f64),
    scale_x: f64,
    scale_y: f64,
}

#[derive(Clone, Copy, Debug)]
enum DragKind {
    Component(u32),
    Pan,
}

/// Shared viewport state: zoom factor and pan offset, both in
/// viewBox (mm) coordinates.
#[derive(Clone, Copy)]
pub struct ViewTransform {
    pub zoom: RwSignal<f64>,
    pub pan: RwSignal<(f64, f64)>,
}

impl ViewTransform {
    pub fn new() -> Self {
        Self {
            zoom: RwSignal::new(1.0),
            pan: RwSignal::new((0.0, 0.0)),
        }
    }

    pub fn zoom_button(self, factor: f64) {
        let z = self.zoom.get();
        let new_z = (z * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let (px, py) = self.pan.get();
        let centre_x = px + MIN_VIEWBOX_W / z / 2.0;
        let centre_y = py + MIN_VIEWBOX_H / z / 2.0;
        let new_px = centre_x - MIN_VIEWBOX_W / new_z / 2.0;
        let new_py = centre_y - MIN_VIEWBOX_H / new_z / 2.0;
        self.zoom.set(new_z);
        self.pan.set((new_px, new_py));
    }

    pub fn reset(self) {
        self.zoom.set(1.0);
        self.pan.set((0.0, 0.0));
    }

    fn viewbox_attr(self) -> String {
        let z = self.zoom.get();
        let (px, py) = self.pan.get();
        let w = MIN_VIEWBOX_W / z;
        let h = MIN_VIEWBOX_H / z;
        format!("{px} {py} {w} {h}")
    }
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
enum PinLayout {
    /// Multi-pin IC body. Each pin's side is precomputed so VCC pins
    /// sit on top, GND on the bottom, the reset/boot/clock-out pin
    /// on the right, and the rest on the left.
    Ic(IcLayout),
    /// Horizontal 2-pin. `reversed = false`: pin 0 on the left,
    /// pin 1 on the right (`Rotation::Zero`). `reversed = true`:
    /// pin 0 on the right, pin 1 on the left (`Rotation::OneEighty`).
    /// Used by USB+ESD so the diode's anode pin faces back into
    /// the connector.
    TwoPinHorizontal { reversed: bool },
    /// Vertical 2-pin: pin 0 on the bottom, pin 1 on the top.
    /// Set by `synth_layout` for decoupling caps so the VCC flag
    /// attaches naturally above and the GND flag below.
    TwoPinVertical {
        /// Whether the 90° rotation runs clockwise (pin 1 ends up
        /// on top) or counter-clockwise (pin 0 ends up on top).
        clockwise: bool,
    },
}

/// Precomputed pin layout for an IC body. Indexed by pin idx.
#[derive(Clone, Debug)]
struct IcLayout {
    placements: Vec<PinPlacement>,
    /// `(bx, by, bw, bh)` of the IC body rectangle.
    bbox: (f64, f64, f64, f64),
}

#[derive(Clone, Copy, Debug)]
struct PinPlacement {
    tip_x: f64,
    tip_y: f64,
    dir_x: f64,
    dir_y: f64,
    side: synth_layout::PinSide,
}

impl PinLayout {
    fn for_kind(
        kind: &str,
        part: Option<&synth_registry::Part>,
        rotation: Rotation,
        cx: f64,
        cy: f64,
    ) -> Self {
        let pin_count = part.map_or(0, |p| p.pins.len());
        if pin_count == 2 && is_two_pin_symbol_kind(kind) {
            match rotation {
                Rotation::Ninety => Self::TwoPinVertical { clockwise: true },
                Rotation::TwoSeventy => Self::TwoPinVertical { clockwise: false },
                Rotation::OneEighty => Self::TwoPinHorizontal { reversed: true },
                Rotation::Zero => Self::TwoPinHorizontal { reversed: false },
            }
        } else {
            Self::Ic(compute_ic_layout(part, cx, cy))
        }
    }

    /// Pin-tip position and stub direction as a 2D unit vector.
    fn pin_tip(&self, pin_idx: usize, cx: f64, cy: f64) -> (f64, f64, f64, f64) {
        match self {
            Self::Ic(ic) => {
                let p = ic.placements.get(pin_idx).copied().unwrap_or(PinPlacement {
                    tip_x: cx,
                    tip_y: cy,
                    dir_x: -1.0,
                    dir_y: 0.0,
                    side: synth_layout::PinSide::Left,
                });
                (p.tip_x, p.tip_y, p.dir_x, p.dir_y)
            }
            Self::TwoPinHorizontal { reversed } => {
                // reversed=false: pin 0 left, pin 1 right.
                // reversed=true: pin 0 right, pin 1 left.
                let pin0_on_left = !reversed;
                if (pin_idx == 0) == pin0_on_left {
                    (cx - TWOPIN_HALF_W - PIN_LENGTH, cy, -1.0, 0.0)
                } else {
                    (cx + TWOPIN_HALF_W + PIN_LENGTH, cy, 1.0, 0.0)
                }
            }
            Self::TwoPinVertical { clockwise } => {
                let pin0_on_top = !clockwise;
                let top_y = cy - TWOPIN_HALF_W - PIN_LENGTH;
                let bottom_y = cy + TWOPIN_HALF_W + PIN_LENGTH;
                if (pin_idx == 0) == pin0_on_top {
                    (cx, top_y, 0.0, -1.0)
                } else {
                    (cx, bottom_y, 0.0, 1.0)
                }
            }
        }
    }

    /// Which side of the body a pin emerges from. Drives label
    /// positioning. For 2-pin variants the answer doesn't matter
    /// (the label code special-cases them); we return Left.
    fn pin_side(&self, pin_idx: usize) -> synth_layout::PinSide {
        match self {
            Self::Ic(ic) => ic
                .placements
                .get(pin_idx)
                .map_or(synth_layout::PinSide::Left, |p| p.side),
            _ => synth_layout::PinSide::Left,
        }
    }

    /// Axis-aligned bounding box of the symbol body.
    fn body_bbox(&self, cx: f64, cy: f64) -> (f64, f64, f64, f64) {
        match self {
            Self::Ic(ic) => ic.bbox,
            Self::TwoPinHorizontal { .. } => {
                (cx - TWOPIN_HALF_W, cy - 2.0, TWOPIN_HALF_W * 2.0, 4.0)
            }
            Self::TwoPinVertical { .. } => (cx - 2.0, cy - TWOPIN_HALF_W, 4.0, TWOPIN_HALF_W * 2.0),
        }
    }
}

/// Classify which side of the IC body each pin should emerge from.
///
/// - **Top**: `power_input` / `power_output` that isn't a ground
///   (positive supply rails — vcc, vdd, vbus, vin, vout, …).
/// - **Bottom**: `power_input` named `gnd` / `vss` / `vssa` / `gnda`.
/// - **Right**: pins with `reset`, `boot_mode`, `clock_input`,
///   `clock_output`, or `rf_feed` capability — board-edge / control
///   signals that conventionally exit the right side of an IC.
/// - **Left**: everything else (signal pins).
///
/// NoConnect pins fall into the Left bucket and get hidden by the
/// renderer; that's the same convention real schematic editors use.
fn classify_ic_pin(pin: &synth_registry::Pin) -> synth_layout::PinSide {
    use synth_layout::PinSide;
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
        ElectricalType::PowerInput | ElectricalType::PowerOutput
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
    PinSide::Left
}

/// Compute every pin's tip position + side for an IC body centred
/// at `(cx, cy)`. The body dimensions are sized to fit the
/// max(left_count, right_count) along the vertical axis and
/// max(top_count, bottom_count) along the horizontal axis.
fn compute_ic_layout(part: Option<&synth_registry::Part>, cx: f64, cy: f64) -> IcLayout {
    use synth_layout::PinSide;
    let Some(part) = part else {
        return IcLayout {
            placements: Vec::new(),
            bbox: (
                cx - BODY_HALF_WIDTH,
                cy - MIN_BODY_HEIGHT / 2.0,
                BODY_HALF_WIDTH * 2.0,
                MIN_BODY_HEIGHT,
            ),
        };
    };
    let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin).collect();
    let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
    let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
    let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
    let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();

    let horiz_max = top_n.max(bottom_n).max(2);
    let vert_max = left_n.max(right_n).max(2);
    let body_w = (horiz_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING;
    let body_w = body_w.max(BODY_HALF_WIDTH * 2.0);
    let body_h = (vert_max as f64) * PIN_PITCH + 2.0 * BODY_PIN_PADDING;
    let body_h = body_h.max(MIN_BODY_HEIGHT);
    let bx = cx - body_w / 2.0;
    let by = cy - body_h / 2.0;

    let mut top_idx = 0_usize;
    let mut bottom_idx = 0_usize;
    let mut left_idx = 0_usize;
    let mut right_idx = 0_usize;
    let placements: Vec<PinPlacement> = sides
        .iter()
        .map(|side| match side {
            PinSide::Top => {
                let x = bx + BODY_PIN_PADDING + PIN_PITCH * (top_idx as f64);
                let y = by - PIN_LENGTH;
                top_idx += 1;
                PinPlacement {
                    tip_x: x,
                    tip_y: y,
                    dir_x: 0.0,
                    dir_y: -1.0,
                    side: PinSide::Top,
                }
            }
            PinSide::Bottom => {
                let x = bx + BODY_PIN_PADDING + PIN_PITCH * (bottom_idx as f64);
                let y = by + body_h + PIN_LENGTH;
                bottom_idx += 1;
                PinPlacement {
                    tip_x: x,
                    tip_y: y,
                    dir_x: 0.0,
                    dir_y: 1.0,
                    side: PinSide::Bottom,
                }
            }
            PinSide::Left => {
                let x = bx - PIN_LENGTH;
                let y = by + BODY_PIN_PADDING + PIN_PITCH * (left_idx as f64);
                left_idx += 1;
                PinPlacement {
                    tip_x: x,
                    tip_y: y,
                    dir_x: -1.0,
                    dir_y: 0.0,
                    side: PinSide::Left,
                }
            }
            PinSide::Right => {
                let x = bx + body_w + PIN_LENGTH;
                let y = by + BODY_PIN_PADDING + PIN_PITCH * (right_idx as f64);
                right_idx += 1;
                PinPlacement {
                    tip_x: x,
                    tip_y: y,
                    dir_x: 1.0,
                    dir_y: 0.0,
                    side: PinSide::Right,
                }
            }
        })
        .collect();
    IcLayout {
        placements,
        bbox: (bx, by, body_w, body_h),
    }
}

fn is_two_pin_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "resistor" | "capacitor" | "inductor" | "diode" | "led" | "crystal" | "switch"
    )
}

/// Recognise generic "p1", "p2", "p3", … pin names that passive
/// parts use because their physical pins carry no semantic role.
/// Such names tell the reader nothing and clutter rotated 2-pin
/// symbols; the renderer hides them.
fn is_generic_passive_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first = chars.next();
    if !matches!(first, Some('p' | 'P')) {
        return false;
    }
    let rest: String = chars.collect();
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

#[component]
pub fn Schematic(
    state: ReadSignal<BoardView>,
    view: ViewTransform,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> impl IntoView {
    let offsets: RwSignal<Offsets> = RwSignal::new(HashMap::new());
    let dragging: RwSignal<Option<DragState>> = RwSignal::new(None);
    let svg_ref = NodeRef::<leptos::svg::Svg>::new();

    Effect::new(move |prev: Option<Option<String>>| {
        let current = state.get().board.as_ref().map(|b| b.name.clone());
        let prev = prev.flatten();
        if prev != current && current.is_some() {
            offsets.set(HashMap::new());
        }
        current
    });

    let on_wheel = move |ev: WheelEvent| {
        ev.prevent_default();
        let Some(rect) = svg_bounding_rect(svg_ref) else {
            return;
        };
        if rect.0 <= 0.0 || rect.1 <= 0.0 {
            return;
        }
        let cx_px = ev.client_x() as f64 - rect.2;
        let cy_px = ev.client_y() as f64 - rect.3;

        let z = view.zoom.get();
        let (px, py) = view.pan.get();

        let scale_x = MIN_VIEWBOX_W / z / rect.0;
        let scale_y = MIN_VIEWBOX_H / z / rect.1;
        let world_x = px + cx_px * scale_x;
        let world_y = py + cy_px * scale_y;

        let factor = if ev.delta_y() < 0.0 { 1.15 } else { 1.0 / 1.15 };
        let new_z = (z * factor).clamp(MIN_ZOOM, MAX_ZOOM);

        let new_scale_x = MIN_VIEWBOX_W / new_z / rect.0;
        let new_scale_y = MIN_VIEWBOX_H / new_z / rect.1;
        let new_px = world_x - cx_px * new_scale_x;
        let new_py = world_y - cy_px * new_scale_y;
        view.zoom.set(new_z);
        view.pan.set((new_px, new_py));
    };

    let on_bg_pointerdown = move |ev: PointerEvent| {
        let Some(rect) = svg_bounding_rect(svg_ref) else {
            return;
        };
        if rect.0 <= 0.0 || rect.1 <= 0.0 {
            return;
        }
        let z = view.zoom.get();
        let scale_x = MIN_VIEWBOX_W / z / rect.0;
        let scale_y = MIN_VIEWBOX_H / z / rect.1;
        let base_offset = view.pan.get();
        if let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            let _ = target.set_pointer_capture(ev.pointer_id());
        }
        dragging.set(Some(DragState {
            kind: DragKind::Pan,
            start_client_x: ev.client_x() as f64,
            start_client_y: ev.client_y() as f64,
            base_offset,
            scale_x,
            scale_y,
        }));
        ev.prevent_default();
    };

    let on_bg_pointermove = move |ev: PointerEvent| {
        let Some(state) = dragging.get() else { return };
        if !matches!(state.kind, DragKind::Pan) {
            return;
        }
        let dx_px = ev.client_x() as f64 - state.start_client_x;
        let dy_px = ev.client_y() as f64 - state.start_client_y;
        view.pan.set((
            state.base_offset.0 - dx_px * state.scale_x,
            state.base_offset.1 - dy_px * state.scale_y,
        ));
    };

    let on_bg_pointerup = move |ev: PointerEvent| {
        if let Some(state) = dragging.get() {
            if matches!(state.kind, DragKind::Pan) {
                dragging.set(None);
            }
        }
        if let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            let _ = target.release_pointer_capture(ev.pointer_id());
        }
    };

    view! {
        <div class="schematic-wrap">
            {move || {
                let view_state = state.get();
                if view_state.board.is_none() {
                    return view! {
                        <div class="empty">"No board to display — waiting for a successful compile."</div>
                    }.into_any();
                }

                view! {
                    <svg
                        node_ref=svg_ref
                        class="schematic-svg"
                        xmlns="http://www.w3.org/2000/svg"
                        viewBox=move || view.viewbox_attr()
                        preserveAspectRatio="xMidYMid meet"
                        on:wheel=on_wheel
                        on:pointerdown=on_bg_pointerdown
                        on:pointermove=on_bg_pointermove
                        on:pointerup=on_bg_pointerup
                        on:pointercancel=on_bg_pointerup
                    >
                        <defs>
                            <pattern id="kicad-grid" width=GRID_PITCH height=GRID_PITCH
                                patternUnits="userSpaceOnUse">
                                <circle class="grid-dot" cx="0" cy="0" r="0.08" />
                                <circle class="grid-dot" cx=GRID_PITCH cy="0" r="0.08" />
                                <circle class="grid-dot" cx="0" cy=GRID_PITCH r="0.08" />
                                <circle class="grid-dot" cx=GRID_PITCH cy=GRID_PITCH r="0.08" />
                            </pattern>
                        </defs>
                        {move || {
                            let bv = state.get();
                            let offs = offsets.get();
                            let Some(board) = bv.board.as_ref() else {
                                return ().into_any();
                            };
                            let board_rc = Rc::new(board.clone());
                            let total = board_rc.components.len();
                            let layout_data = synth_layout::layout(&board_rc);
                            let centres = centres_from_layout(&layout_data);
                            let power_net_ids = layout_data.power_net_ids();
                            let labeled_net_ids = layout_data.labeled_net_ids();
                            let flag_index = flag_index_from_layout(&layout_data);
                            let net_label_index = net_label_index_from_layout(&layout_data);
                            let (page_x, page_y, page_w, page_h) =
                                page_bounds(&board_rc, &centres, &offs);
                            let frame = render_frame(page_x, page_y, page_w, page_h);
                            let title_block = render_title_block(
                                &board_rc.name,
                                &bv.source_path,
                                total,
                                page_x,
                                page_y,
                                page_w,
                                page_h,
                            );
                            let components: Vec<_> = board_rc.components.iter()
                                .map(|c| render_component(&board_rc, c, &centres, &flag_index, &net_label_index, &offs, svg_ref, dragging, offsets, view, selected))
                                .collect();
                            // Two wire-rendering paths — see the module doc
                            // comment for why. `route_board` runs full A*
                            // pathfinding per net; `offsets` updates on every
                            // pointer-move event during a component drag, so
                            // recomputing full routing on every mouse-move
                            // frame is a real perf risk this environment
                            // can't load-test. While a component is actively
                            // being dragged, fall back to the cheap local
                            // per-pair heuristic for just that render pass;
                            // otherwise (including right after a drag ends,
                            // where `offs` still holds the left-behind delta)
                            // use the shared router against the
                            // offset-adjusted layout so wires match what
                            // `synth-kicad` would export.
                            let dragging_component = matches!(
                                dragging.get(),
                                Some(DragState { kind: DragKind::Component(_), .. })
                            );
                            let wires: Vec<AnyView> = if dragging_component {
                                board_rc.nets.iter()
                                    .filter(|net| !power_net_ids.contains(&net.id) && !labeled_net_ids.contains(&net.id))
                                    .flat_map(|net| render_wires_for_net(&board_rc, net, &centres, &offs, selected))
                                    .collect()
                            } else {
                                let adjusted_layout = apply_offsets_to_layout(&layout_data, &offs);
                                let route = route_board(&board_rc, &adjusted_layout);
                                render_routed_wires(&route, selected)
                            };
                            view! {
                                <rect class="page" x=page_x y=page_y
                                    width=page_w height=page_h />
                                <rect x=page_x y=page_y width=page_w height=page_h
                                    fill="url(#kicad-grid)" pointer-events="none" />
                                {frame}
                                <g class="wires-layer">{wires}</g>
                                <g class="components-layer">{components}</g>
                                {title_block}
                            }.into_any()
                        }}
                    </svg>
                }.into_any()
            }}
        </div>
    }
}

/// Page bounds: at least A4 landscape; expanded to fit a grid layout
/// plus the title block in the bottom-right corner.
fn page_bounds(board: &Board, centres: &Centres, offsets: &Offsets) -> (f64, f64, f64, f64) {
    let mut min_x = 0.0_f64;
    let mut min_y = 0.0_f64;
    let mut max_x = MIN_VIEWBOX_W;
    let mut max_y = MIN_VIEWBOX_H;
    for component in &board.components {
        let (cx, cy) = component_center(component, centres, offsets);
        let kind = component.part.as_ref().map_or("", |p| p.kind.as_str());
        let rotation = component_rotation(component, centres);
        let layout = PinLayout::for_kind(kind, component.part.as_ref(), rotation, cx, cy);
        let (bx, by, bw, bh) = layout.body_bbox(cx, cy);
        let pad_x = PIN_LENGTH + 4.0;
        let pad_y = 8.0;
        min_x = min_x.min(bx - pad_x);
        min_y = min_y.min(by - pad_y);
        max_x = max_x.max(bx + bw + pad_x);
        max_y = max_y.max(by + bh + pad_y);
    }
    // Reserve room for the title block at the bottom-right.
    max_y = max_y.max(min_y + MIN_VIEWBOX_H);
    max_x = max_x.max(min_x + MIN_VIEWBOX_W);
    // Outer margin so frame and title don't collide with content.
    let margin = 10.0;
    (
        min_x - margin,
        min_y - margin,
        (max_x - min_x) + 2.0 * margin,
        (max_y - min_y) + 2.0 * margin + TITLE_BLOCK_H,
    )
}

/// KiCad-style outer frame: a thin border just inside the page edges.
fn render_frame(page_x: f64, page_y: f64, page_w: f64, page_h: f64) -> AnyView {
    let fx = page_x + FRAME_MARGIN;
    let fy = page_y + FRAME_MARGIN;
    let fw = page_w - 2.0 * FRAME_MARGIN;
    let fh = page_h - 2.0 * FRAME_MARGIN;
    view! {
        <g class="sheet-frame" pointer-events="none">
            <rect x=fx y=fy width=fw height=fh fill="none" />
        </g>
    }
    .into_any()
}

/// KiCad-style title block in the bottom-right corner: rows for
/// sheet info, file, title, and a footer with size/rev/id/generator.
#[allow(clippy::too_many_arguments)]
fn render_title_block(
    board_name: &str,
    source_path: &str,
    total: usize,
    page_x: f64,
    page_y: f64,
    page_w: f64,
    page_h: f64,
) -> AnyView {
    let right = page_x + page_w - FRAME_MARGIN;
    let bottom = page_y + page_h - FRAME_MARGIN;
    let left = right - TITLE_BLOCK_W;
    let top = bottom - TITLE_BLOCK_H;
    // Row partitions (top-to-bottom).
    let r1 = top + 7.0; // Sheet / File
    let r2 = top + 17.0; // Title
    let r3 = top + 25.0; // Size | Date | Rev
    let r4 = bottom; // Footer

    // Column partition for the bottom Size/Date/Rev row.
    let c1 = left + TITLE_BLOCK_W * 0.40;
    let c2 = left + TITLE_BLOCK_W * 0.75;

    let board_label = if board_name.is_empty() {
        "(untitled)".to_string()
    } else {
        board_name.to_string()
    };
    let file_label = source_path
        .rsplit('/')
        .next()
        .unwrap_or(source_path)
        .to_string();
    let size_label = format!("Size: {}", classify_sheet_size(page_w, page_h));
    let count_label = format!("{total} parts");
    view! {
        <g class="title-block" pointer-events="none">
            // Outer box
            <rect x=left y=top width=TITLE_BLOCK_W height=TITLE_BLOCK_H
                class="tb-frame" />
            // Inner horizontal dividers
            <line x1=left y1=r1 x2=right y2=r1 class="tb-rule" />
            <line x1=left y1=r2 x2=right y2=r2 class="tb-rule" />
            <line x1=left y1=r3 x2=right y2=r3 class="tb-rule" />
            // Bottom-row column dividers
            <line x1=c1 y1=r3 x2=c1 y2=r4 class="tb-rule" />
            <line x1=c2 y1=r3 x2=c2 y2=r4 class="tb-rule" />

            // Row 1: Sheet + File
            <text class="tb-label" x=left + 1.5 y=top + 2.5>"Sheet: /"</text>
            <text class="tb-label" x=left + 1.5 y=r1 - 1.0>{format!("File: {file_label}")}</text>

            // Row 2: Title
            <text class="tb-label-strong" x=left + 1.5 y=r1 + 4.0>"Title"</text>
            <text class="tb-title" x=left + TITLE_BLOCK_W / 2.0 y=r2 - 1.5
                text-anchor="middle">{board_label}</text>

            // Row 3: Size / Date / Rev cells
            <text class="tb-label" x=left + 1.5 y=r3 - 1.5>{size_label}</text>
            <text class="tb-label" x=c1 + 1.5 y=r3 - 1.5>{count_label}</text>
            <text class="tb-label" x=c2 + 1.5 y=r3 - 1.5>"Rev: —"</text>

            // Footer: generator + sheet id
            <text class="tb-footer" x=left + 1.5 y=r4 - 1.2>"synth · preview"</text>
            <text class="tb-footer" x=right - 1.5 y=r4 - 1.2 text-anchor="end">"Id: 1/1"</text>
        </g>
    }
    .into_any()
}

fn classify_sheet_size(w: f64, h: f64) -> &'static str {
    let area = w * h;
    if area <= 297.0 * 210.0 * 1.1 {
        "A4"
    } else if area <= 420.0 * 297.0 * 1.1 {
        "A3"
    } else if area <= 594.0 * 420.0 * 1.1 {
        "A2"
    } else {
        "custom"
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn render_component(
    board: &Board,
    component: &Component,
    centres: &Centres,
    flag_index: &FlagIndex,
    net_label_index: &NetLabelIndex,
    offsets: &Offsets,
    svg_ref: NodeRef<leptos::svg::Svg>,
    dragging: RwSignal<Option<DragState>>,
    offsets_sig: RwSignal<Offsets>,
    view: ViewTransform,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> impl IntoView + use<> {
    let id = component.id.0;
    let (cx, cy) = component_center(component, centres, offsets);
    let kind = component
        .part
        .as_ref()
        .map_or_else(String::new, |p| p.kind.as_str().to_string());
    let part_id = component
        .part
        .as_ref()
        .map_or_else(String::new, |p| p.id.as_str().to_string());
    let rotation = component_rotation(component, centres);
    let layout = PinLayout::for_kind(&kind, component.part.as_ref(), rotation, cx, cy);
    let component_id_typed = component.id;

    // Owned base-position snapshot captured by the `'static` drag-end
    // closure so it can persist offsets to the sidecar (§7.7.6).
    let base_positions: BasePositions = board
        .components
        .iter()
        .map(|c| {
            let ((bx, by), rot) = centres
                .get(&c.id)
                .copied()
                .unwrap_or(((0.0, 0.0), Rotation::Zero));
            (c.id.0, c.refdes.clone(), (bx, by), rot)
        })
        .collect();

    let pins: Vec<_> = component
        .part
        .as_ref()
        .map(|part| {
            part.pins
                .iter()
                .enumerate()
                .map(|(i, pin)| {
                    let pin_id = PinId(i as u32);
                    let (tip_x, tip_y, dir_x, dir_y) = layout.pin_tip(i, cx, cy);
                    let inner_x = tip_x - dir_x * PIN_LENGTH;
                    let inner_y = tip_y - dir_y * PIN_LENGTH;
                    // Suppress meaningless `p1`/`p2`-style names on
                    // 2-pin passives. Keep `anode`/`cathode` (diodes)
                    // and IC pin names — they actually inform the
                    // reader.
                    let pin_name = if matches!(
                        layout,
                        PinLayout::TwoPinHorizontal { .. } | PinLayout::TwoPinVertical { .. }
                    ) && is_generic_passive_name(&pin.name)
                    {
                        String::new()
                    } else {
                        pin.name.clone()
                    };
                    let pin_number = pin.number.0.clone();
                    let (num_x, num_y, name_x, name_y, name_anchor) = match &layout {
                        PinLayout::Ic(_) => {
                            // For ICs the label sits inside the body
                            // next to the pin stub, mirrored by side.
                            use synth_layout::PinSide;
                            let side = layout.pin_side(i);
                            match side {
                                PinSide::Left => (
                                    (tip_x + inner_x) / 2.0,
                                    tip_y - 0.5,
                                    inner_x + 0.8,
                                    tip_y + 0.5,
                                    "start",
                                ),
                                PinSide::Right => (
                                    (tip_x + inner_x) / 2.0,
                                    tip_y - 0.5,
                                    inner_x - 0.8,
                                    tip_y + 0.5,
                                    "end",
                                ),
                                PinSide::Top => (
                                    tip_x + 0.8,
                                    (tip_y + inner_y) / 2.0 + 0.4,
                                    tip_x,
                                    inner_y + 1.6,
                                    "middle",
                                ),
                                PinSide::Bottom => (
                                    tip_x + 0.8,
                                    (tip_y + inner_y) / 2.0 + 0.4,
                                    tip_x,
                                    inner_y - 0.8,
                                    "middle",
                                ),
                            }
                        }
                        PinLayout::TwoPinHorizontal { .. } => (
                            (tip_x + inner_x) / 2.0,
                            tip_y - 0.5,
                            if dir_x < 0.0 {
                                tip_x - 0.4
                            } else {
                                tip_x + 0.4
                            },
                            tip_y + 2.4,
                            if dir_x < 0.0 { "end" } else { "start" },
                        ),
                        PinLayout::TwoPinVertical { .. } => {
                            // Vertical 2-pin: stub runs along the
                            // y-axis. Number sits to the right of the
                            // stub midpoint; pin name sits to the
                            // left of the pin tip, offset slightly
                            // up or down so it doesn't collide with
                            // the power flag label.
                            let stub_mid_y = (tip_y + inner_y) / 2.0;
                            (
                                tip_x + 0.7,
                                stub_mid_y + 0.4,
                                tip_x - 0.6,
                                tip_y + if dir_y < 0.0 { -0.4 } else { 1.4 },
                                "end",
                            )
                        }
                    };
                    let flag = flag_index
                        .get(&(component_id_typed, pin_id))
                        .map(|f| render_power_flag(tip_x, tip_y, dir_x, dir_y, f));
                    let net_label = net_label_index
                        .get(&(component_id_typed, pin_id))
                        .map(|l| render_net_label(tip_x, tip_y, dir_x, dir_y, l));
                    view! {
                        <g class="pin">
                            <line x1=tip_x y1=tip_y x2=inner_x y2=inner_y class="pin-stub" />
                            <text class="pin-name" x=name_x y=name_y text-anchor=name_anchor>
                                {pin_name}
                            </text>
                            <text class="pin-num" x=num_x y=num_y text-anchor="middle">
                                {pin_number}
                            </text>
                            {flag}
                            {net_label}
                        </g>
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let body = render_body(&layout, &kind, &part_id, cx, cy);

    let refdes = component.refdes.clone();
    let value = part_id.clone();
    let (bx, by, bw, bh) = layout.body_bbox(cx, cy);
    let refdes_y = by - 1.4;
    let value_y = by + bh + 3.2;
    let label_x = bx + bw / 2.0;

    let on_pointerdown = move |ev: PointerEvent| {
        ev.stop_propagation();
        let Some(rect) = svg_bounding_rect(svg_ref) else {
            return;
        };
        if rect.0 <= 0.0 || rect.1 <= 0.0 {
            return;
        }
        let z = view.zoom.get();
        let scale_x = MIN_VIEWBOX_W / z / rect.0;
        let scale_y = MIN_VIEWBOX_H / z / rect.1;
        let base_offset = offsets_sig
            .with(|o| o.get(&id).copied())
            .unwrap_or((0.0, 0.0));
        if let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            let _ = target.set_pointer_capture(ev.pointer_id());
        }
        dragging.set(Some(DragState {
            kind: DragKind::Component(id),
            start_client_x: ev.client_x() as f64,
            start_client_y: ev.client_y() as f64,
            base_offset,
            scale_x,
            scale_y,
        }));
        ev.prevent_default();
    };

    let on_pointermove = move |ev: PointerEvent| {
        ev.stop_propagation();
        let Some(state) = dragging.get() else { return };
        let DragKind::Component(comp_id) = state.kind else {
            return;
        };
        if comp_id != id {
            return;
        }
        let dx_px = ev.client_x() as f64 - state.start_client_x;
        let dy_px = ev.client_y() as f64 - state.start_client_y;
        let new_offset = (
            state.base_offset.0 + dx_px * state.scale_x,
            state.base_offset.1 + dy_px * state.scale_y,
        );
        offsets_sig.update(|o| {
            o.insert(id, new_offset);
        });
    };

    let on_pointerup = move |ev: PointerEvent| {
        ev.stop_propagation();
        let was_dragging_component = matches!(
            dragging.get(),
            Some(DragState {
                kind: DragKind::Component(_),
                ..
            })
        );
        dragging.set(None);
        if let Some(target) = ev.target().and_then(|t| t.dyn_into::<Element>().ok()) {
            let _ = target.release_pointer_capture(ev.pointer_id());
        }
        // A component drag just finished: persist the resulting offsets
        // to the sidecar so they survive a reload (§7.7.6).
        if was_dragging_component {
            let live_offsets = offsets_sig.get_untracked();
            save_layout_to_sidecar(&base_positions, &live_offsets);
        }
    };

    let comp_id = component.id;
    let on_click = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        selected.set(crate::state::SelectedEntity::Component(comp_id));
    };

    let is_selected = move || selected.get() == crate::state::SelectedEntity::Component(comp_id);

    // `on_pointerup` captures owned data (BasePositions), so it is not
    // `Copy`; clone it for the `pointercancel` handler so both event
    // bindings can move their own instance into `view!`.
    let on_pointercancel = on_pointerup.clone();

    view! {
        <g
            class=move || if is_selected() { "component selected" } else { "component" }
            on:pointerdown=on_pointerdown
            on:pointermove=on_pointermove
            on:pointerup=on_pointerup
            on:pointercancel=on_pointercancel
            on:click=on_click
        >
            <rect class="hit-target" x=bx y=by width=bw height=bh fill="transparent" />
            {move || is_selected().then(|| view! {
                <rect class="selection-outline"
                    x=bx - 1.2 y=by - 1.2
                    width=bw + 2.4 height=bh + 2.4
                    fill="none" stroke="#3b82f6" stroke-width="0.8" stroke-dasharray="1.5,1.0" rx="0.8" />
            })}
            {body}
            {pins}
            <text class="refdes" x=label_x y=refdes_y text-anchor="middle">{refdes}</text>
            <text class="value" x=label_x y=value_y text-anchor="middle">{value}</text>
        </g>
    }
}

fn render_body(layout: &PinLayout, kind: &str, part_id: &str, cx: f64, cy: f64) -> AnyView {
    match layout {
        PinLayout::Ic(_) => {
            let (bx, by, bw, bh) = layout.body_bbox(cx, cy);
            view! { <rect class="body" x=bx y=by width=bw height=bh /> }.into_any()
        }
        PinLayout::TwoPinHorizontal { reversed: false } => {
            render_two_pin_symbol(kind, part_id, cx, cy)
        }
        PinLayout::TwoPinHorizontal { reversed: true } => {
            // Mirror the horizontal symbol left-right around the
            // body centre so pin 0 sits on the right side. For a
            // diode the triangle now points left and the cathode
            // bar moves to the left side — matching the
            // anode-on-right pin position.
            let inner = render_two_pin_symbol(kind, part_id, cx, cy);
            view! {
                <g transform={format!("matrix(-1 0 0 1 {} 0)", 2.0 * cx)}>{inner}</g>
            }
            .into_any()
        }
        PinLayout::TwoPinVertical { .. } => {
            // Rotate the horizontal symbol 90° around the centre.
            // SVG's transform takes degrees; we use 90 (clockwise in
            // screen coordinates). Pin positions are computed
            // separately via `pin_tip`, which already returns the
            // rotated coordinates.
            let inner = render_two_pin_symbol(kind, part_id, cx, cy);
            view! {
                <g transform={format!("rotate(90 {cx} {cy})")}>{inner}</g>
            }
            .into_any()
        }
    }
}

/// Two-pin symbols. Pins sit at `(cx ± TWOPIN_HALF_W, cy)` and the
/// symbol body is drawn between them with explicit leads — so the
/// horizontal "wire" of the symbol actually meets the pin stubs.
fn render_two_pin_symbol(kind: &str, part_id: &str, cx: f64, cy: f64) -> AnyView {
    let left = cx - TWOPIN_HALF_W;
    let right = cx + TWOPIN_HALF_W;

    match kind {
        "resistor" => {
            // ANSI zigzag: 3 peaks (up) interleaved with 2 valleys
            // (down). Half-width entry/exit segments give the
            // classic symmetric look. Modelled on netlistsvg's
            // resistor geometry.
            let zigzag_w: f64 = 5.0;
            let amp: f64 = 1.0;
            let z_left = cx - zigzag_w / 2.0;
            let z_right = cx + zigzag_w / 2.0;
            let dx = 1.0; // peak-to-peak horizontal spacing
            let points = format!(
                "{left},{cy} \
                 {zl},{cy} \
                 {p1x},{py_up} \
                 {v1x},{py_dn} \
                 {p2x},{py_up} \
                 {v2x},{py_dn} \
                 {p3x},{py_up} \
                 {zr},{cy} \
                 {right},{cy}",
                left = left,
                cy = cy,
                zl = z_left,
                p1x = z_left + 0.5 * dx,
                v1x = z_left + 1.5 * dx,
                p2x = z_left + 2.5 * dx,
                v2x = z_left + 3.5 * dx,
                p3x = z_left + 4.5 * dx,
                zr = z_right,
                right = right,
                py_up = cy - amp,
                py_dn = cy + amp,
            );
            view! {
                <polyline class="symbol" points=points fill="none" />
            }
            .into_any()
        }

        "capacitor" => {
            // Two parallel plates, non-polarised.
            let plate_gap = 0.6;
            view! {
                <g class="symbol">
                    <line x1=left y1=cy x2={cx - plate_gap} y2=cy />
                    <line x1={cx + plate_gap} y1=cy x2=right y2=cy />
                    <line x1={cx - plate_gap} y1={cy - 2.0}
                          x2={cx - plate_gap} y2={cy + 2.0} />
                    <line x1={cx + plate_gap} y1={cy - 2.0}
                          x2={cx + plate_gap} y2={cy + 2.0} />
                </g>
            }
            .into_any()
        }

        "inductor" => {
            // Three half-circles in a row, leads tucked into the
            // outer hump.
            let r = 1.0;
            let span = r * 2.0;
            let start_x = cx - 1.5 * span;
            let end_x = start_x + 3.0 * span;
            view! {
                <g class="symbol" fill="none">
                    <line x1=left y1=cy x2=start_x y2=cy />
                    <path d={format!(
                        "M {start_x} {cy} a {r} {r} 0 0 1 {span} 0 a {r} {r} 0 0 1 {span} 0 a {r} {r} 0 0 1 {span} 0"
                    )} />
                    <line x1=end_x y1=cy x2=right y2=cy />
                </g>
            }
            .into_any()
        }

        "diode" | "led" => {
            // Triangle pointing right (anode → cathode) with a bar.
            // LEDs get two diagonal arrows of emission.
            let is_led = kind == "led" || part_id.starts_with("led_");
            let tri_left = cx - 1.2;
            let tri_right = cx + 1.2;
            let tri = format!(
                "M {tl} {ay1} L {tl} {ay2} L {tr} {cy} Z",
                tl = tri_left,
                ay1 = cy - 1.5,
                ay2 = cy + 1.5,
                tr = tri_right,
                cy = cy,
            );
            view! {
                <g class="symbol">
                    <line x1=left y1=cy x2=tri_left y2=cy />
                    <path d=tri fill="currentColor" />
                    <line x1=tri_right y1={cy - 1.5} x2=tri_right y2={cy + 1.5} />
                    <line x1=tri_right y1=cy x2=right y2=cy />
                    {is_led.then(|| view! {
                        // Two parallel light-emission arrows at 45°
                        // up-right. Hand-positioned with explicit
                        // endpoints (no transforms) so the layout is
                        // obvious. Each shaft is 1.7mm long; both
                        // start at y = cy - 1.7 (clear of the diode
                        // triangle top at cy - 1.5) and end at
                        // y = cy - 2.9 (clear of the refdes baseline
                        // at cy - 3.4).
                        <g class="led-arrows" stroke="currentColor"
                           stroke-width="0.18" fill="currentColor">
                            <line x1={cx - 0.3} y1={cy - 1.7}
                                  x2={cx + 0.9} y2={cy - 2.9} fill="none" />
                            <polygon points={format!(
                                "{},{} {},{} {},{}",
                                cx + 0.9,   cy - 2.9,
                                cx + 0.334, cy - 2.758,
                                cx + 0.758, cy - 2.334,
                            )} />
                            <line x1={cx + 0.7} y1={cy - 1.7}
                                  x2={cx + 1.9} y2={cy - 2.9} fill="none" />
                            <polygon points={format!(
                                "{},{} {},{} {},{}",
                                cx + 1.9,   cy - 2.9,
                                cx + 1.334, cy - 2.758,
                                cx + 1.758, cy - 2.334,
                            )} />
                        </g>
                    })}
                </g>
            }
            .into_any()
        }

        "crystal" => view! {
            <g class="symbol" fill="none">
                <line x1=left y1=cy x2={cx - 1.2} y2=cy />
                <line x1={cx - 1.2} y1={cy - 1.5} x2={cx - 1.2} y2={cy + 1.5} />
                <rect x={cx - 0.6} y={cy - 1.5} width="1.2" height="3.0" />
                <line x1={cx + 1.2} y1={cy - 1.5} x2={cx + 1.2} y2={cy + 1.5} />
                <line x1={cx + 1.2} y1=cy x2=right y2=cy />
            </g>
        }
        .into_any(),

        "switch" => view! {
            <g class="symbol" fill="currentColor">
                <line x1=left y1=cy x2={cx - 1.5} y2=cy stroke="currentColor" />
                <circle cx={cx - 1.5} cy=cy r="0.4" />
                <circle cx={cx + 1.5} cy=cy r="0.4" />
                <line x1={cx - 1.5} y1=cy x2={cx + 1.3} y2={cy - 1.8}
                    stroke="currentColor" fill="none" />
                <line x1={cx + 1.5} y1=cy x2=right y2=cy stroke="currentColor" />
            </g>
        }
        .into_any(),

        _ => view! {
            <g class="symbol">
                <line x1=left y1=cy x2={cx - 2.0} y2=cy />
                <rect x={cx - 2.0} y={cy - 1.0} width="4.0" height="2.0" fill="none" />
                <line x1={cx + 2.0} y1=cy x2=right y2=cy />
            </g>
        }
        .into_any(),
    }
}

/// Render a small VCC arrow or GND symbol attached to a pin tip.
///
/// VCC: the symbol grows UP from the pin (an upward arrowhead with
/// the label above). GND: the symbol grows DOWN from the pin (the
/// classic three-bar GND with the label below). Both stay close
/// (within ~5mm) so they read as "this pin is on power" without
/// needing to follow any wire.
/// Render a net label attached to a pin's stub end.
///
/// A net label is a small piece of text drawn just past the pin's
/// stub in the stub's direction. KiCad convention: a short stub
/// (we draw the existing pin stub), then a label parallel to the
/// stub axis. Two same-named labels on the same sheet are
/// understood to be electrically connected.
fn render_net_label(tip_x: f64, tip_y: f64, dir_x: f64, dir_y: f64, label: &NetLabel) -> AnyView {
    // Place the label a little past the pin stub's outer end.
    let stub_len: f64 = 2.0;
    let label_x = tip_x + dir_x * (stub_len + 0.3);
    let label_y = tip_y + dir_y * (stub_len + 0.3);
    // Anchor and baseline tweak the text so it sits along the stub
    // axis without overlapping the symbol body.
    let (anchor, dy) = if dir_x > 0.5 {
        // Pin extends right → label to the right, left-aligned.
        ("start", 0.4)
    } else if dir_x < -0.5 {
        ("end", 0.4)
    } else if dir_y < -0.5 {
        // Pin extends up → label sits above the stub tip.
        ("middle", -0.4)
    } else {
        // dir_y > 0: pin extends down → label below.
        ("middle", 1.5)
    };
    let text = label.label.clone();
    view! {
        <g class="net-label" pointer-events="none">
            <text class="net-label-text" x=label_x y=label_y + dy text-anchor=anchor>
                {text}
            </text>
        </g>
    }
    .into_any()
}

/// Render a power-rail symbol (VCC arrow or GND bars) attached to
/// a pin tip, oriented along the pin's stub direction.
///
/// The symbol always grows OUTWARD from the pin in `(dir_x, dir_y)`,
/// so a top pin gets a VCC arrow pointing up, a bottom pin gets a
/// GND symbol pointing down, a right-side pin (e.g., a GPIO
/// classified as Right) gets the symbol oriented horizontally to
/// the right, etc. This eliminates the L-shaped stubs that
/// happened in earlier slices where all symbols pointed up/down
/// regardless of pin orientation.
fn render_power_flag(tip_x: f64, tip_y: f64, dir_x: f64, dir_y: f64, flag: &PowerFlag) -> AnyView {
    let stub_len: f64 = 2.0;
    let se_x = tip_x + dir_x * stub_len;
    let se_y = tip_y + dir_y * stub_len;
    // Perpendicular (rotate dir 90° CCW in screen coords: (-dy, dx)).
    let perp_x = -dir_y;
    let perp_y = dir_x;
    let anchor = pick_flag_label_anchor(dir_x, dir_y);
    let label = flag.label.clone();
    match flag.kind {
        PowerFlagKind::Vcc => {
            let arrow_len: f64 = 1.4;
            let half_w: f64 = 0.9;
            // Arrow tip past the stub end, base at the stub end.
            let at_x = se_x + dir_x * arrow_len;
            let at_y = se_y + dir_y * arrow_len;
            let bl_x = se_x + perp_x * half_w;
            let bl_y = se_y + perp_y * half_w;
            let br_x = se_x - perp_x * half_w;
            let br_y = se_y - perp_y * half_w;
            let label_off: f64 = 0.9;
            let lx = at_x + dir_x * label_off;
            let ly = at_y + dir_y * label_off + 0.4;
            view! {
                <g class="power-flag vcc" pointer-events="none">
                    <line x1=tip_x y1=tip_y x2=se_x y2=se_y class="flag-stub" />
                    <polygon points={format!(
                        "{at_x},{at_y} {bl_x},{bl_y} {br_x},{br_y}",
                    )} />
                    <text class="flag-label" x=lx y=ly text-anchor=anchor>
                        {label}
                    </text>
                </g>
            }
            .into_any()
        }
        PowerFlagKind::Gnd => {
            let bar1_half: f64 = 1.2;
            let bar2_half: f64 = 0.8;
            let bar3_half: f64 = 0.4;
            let bar_spacing: f64 = 0.55;
            // Bar centres along the direction axis.
            let b1_cx = se_x;
            let b1_cy = se_y;
            let b2_cx = se_x + dir_x * bar_spacing;
            let b2_cy = se_y + dir_y * bar_spacing;
            let b3_cx = se_x + dir_x * 2.0 * bar_spacing;
            let b3_cy = se_y + dir_y * 2.0 * bar_spacing;
            // Each bar's left/right ends along the perpendicular axis.
            let bar = |cx: f64, cy: f64, half: f64| {
                (
                    cx + perp_x * half,
                    cy + perp_y * half,
                    cx - perp_x * half,
                    cy - perp_y * half,
                )
            };
            let (b1lx, b1ly, b1rx, b1ry) = bar(b1_cx, b1_cy, bar1_half);
            let (b2lx, b2ly, b2rx, b2ry) = bar(b2_cx, b2_cy, bar2_half);
            let (b3lx, b3ly, b3rx, b3ry) = bar(b3_cx, b3_cy, bar3_half);
            let label_off: f64 = 1.4;
            let lx = se_x + dir_x * (2.0 * bar_spacing + label_off);
            let ly = se_y + dir_y * (2.0 * bar_spacing + label_off) + 0.5;
            view! {
                <g class="power-flag gnd" pointer-events="none">
                    <line x1=tip_x y1=tip_y x2=se_x y2=se_y class="flag-stub" />
                    <line x1=b1lx y1=b1ly x2=b1rx y2=b1ry class="gnd-bar" />
                    <line x1=b2lx y1=b2ly x2=b2rx y2=b2ry class="gnd-bar" />
                    <line x1=b3lx y1=b3ly x2=b3rx y2=b3ry class="gnd-bar" />
                    <text class="flag-label" x=lx y=ly text-anchor=anchor>
                        {label}
                    </text>
                </g>
            }
            .into_any()
        }
    }
}

/// Pick the SVG `text-anchor` for a power-flag label based on the
/// direction the symbol extends.
fn pick_flag_label_anchor(dir_x: f64, dir_y: f64) -> &'static str {
    if dir_x > 0.5 {
        "start"
    } else if dir_x < -0.5 {
        "end"
    } else {
        // dir_y dominates → label sits straight above or below
        let _ = dir_y;
        "middle"
    }
}

/// Render a [`RouteResult`] (from `synth_layout::route::route_board`)
/// as SVG polylines plus junction dots — the settled-view counterpart
/// to `render_wires_for_net`'s drag-fallback rendering. Matches the
/// same `wire`/`wire selected` CSS classes and click-to-select
/// behaviour so the two paths look identical to the user.
///
/// One difference from `render_wires_for_net`'s junction handling:
/// `RouteResult::junctions` is a flat list of `(x, y)` meeting points
/// with no net attribution (unlike the drag-fallback path, which
/// derives a junction from a specific net's root pin), so junction
/// dots here render as plain, non-selectable `junction` markers. This
/// is also the *real* ≥3-way-meeting-point set the router computed,
/// rather than the drag-fallback path's `other_count >= 2` heuristic.
fn render_routed_wires(
    route: &RouteResult,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> Vec<AnyView> {
    let mut out: Vec<AnyView> = Vec::with_capacity(route.wires.len() + route.junctions.len());
    for wire in &route.wires {
        let net_id = wire.net;
        let points = wire
            .points
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(" ");
        let on_click = move |ev: leptos::ev::MouseEvent| {
            ev.stop_propagation();
            selected.set(crate::state::SelectedEntity::Net(net_id));
        };
        let is_selected = move || selected.get() == crate::state::SelectedEntity::Net(net_id);
        out.push(
            view! {
                <polyline
                    class=move || if is_selected() { "wire selected" } else { "wire" }
                    points=points
                    on:click=on_click
                />
            }
            .into_any(),
        );
    }
    for &(jx, jy) in &route.junctions {
        out.push(view! { <circle class="junction" cx=jx cy=jy r="0.6" /> }.into_any());
    }
    out
}

/// Orthogonal wire routing — **per-pair** now, not the old "header
/// band above the page" approach.
///
/// For each endpoint other than the root, we draw an L-shape (or
/// straight line where possible) from the root's pin tip to that
/// endpoint's pin tip:
///
/// 1. Both pins extend their stubs in their own `(dir_x, dir_y)`.
/// 2. If both stubs point along the same axis and the pins are
///    aligned on the perpendicular axis, we collapse to a straight
///    line.
/// 3. Otherwise we pick a 1-corner L-shape (perpendicular stubs)
///    or a 2-corner Z-shape (parallel stubs at different
///    perpendicular positions).
///
/// This removes the "header band" detour that made every short
/// vertical wire go up to a horizontal band above the page and
/// back down — including the R→LED stubs inside an LED-indicator
/// cluster that the user flagged.
fn render_wires_for_net(
    board: &Board,
    net: &Net,
    centres: &Centres,
    offsets: &Offsets,
    selected: RwSignal<crate::state::SelectedEntity>,
) -> Vec<AnyView> {
    if net.endpoints.len() < 2 {
        return Vec::new();
    }
    let Some(root) = endpoint_xyd(board, &net.endpoints[0], centres, offsets) else {
        return Vec::new();
    };
    let stagger_idx = (net.id.0 % WIRE_STAGGER_SLOTS) as f64;
    let stub_len: f64 = 2.0 + stagger_idx * WIRE_STAGGER_STEP;
    let mut out: Vec<AnyView> = Vec::new();
    let net_id = net.id;
    let on_click = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        selected.set(crate::state::SelectedEntity::Net(net_id));
    };
    let is_selected = move || selected.get() == crate::state::SelectedEntity::Net(net_id);

    let mut other_count = 0_usize;
    for ep in net.endpoints.iter().skip(1) {
        let Some(other) = endpoint_xyd(board, ep, centres, offsets) else {
            continue;
        };
        let points = l_route_points(root, other, stub_len, board, centres, offsets);
        out.push(
            view! {
                <polyline
                    class=move || if is_selected() { "wire selected" } else { "wire" }
                    points=points
                    on:click=on_click
                />
            }
            .into_any(),
        );
        other_count += 1;
    }
    if other_count >= 2 {
        let (rx, ry, rdx, rdy) = root;
        let cx = rx + rdx * stub_len;
        let cy = ry + rdy * stub_len;
        out.push(
            view! {
                <circle
                    class=move || if is_selected() { "junction selected" } else { "junction" }
                    cx=cx cy=cy r="0.6"
                    on:click=on_click
                />
            }
            .into_any(),
        );
    }
    out
}

fn segment_crosses_component(
    p1: (f64, f64),
    p2: (f64, f64),
    board: &Board,
    centres: &Centres,
    offsets: &Offsets,
) -> bool {
    let x_min = p1.0.min(p2.0);
    let x_max = p1.0.max(p2.0);
    let y_min = p1.1.min(p2.1);
    let y_max = p1.1.max(p2.1);

    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let (cx, cy) = component_center(component, centres, offsets);
        let rotation = component_rotation(component, centres);
        let layout = PinLayout::for_kind(part.kind.as_str(), Some(part), rotation, cx, cy);
        let (bx, by, bw, bh) = layout.body_bbox(cx, cy);

        // Add a small safety margin of 0.5 mm
        let margin = 0.5;
        let lo_x = bx - margin;
        let hi_x = bx + bw + margin;
        let lo_y = by - margin;
        let hi_y = by + bh + margin;

        if (p1.1 - p2.1).abs() < 0.01 {
            let y = p1.1;
            if y > lo_y && y < hi_y && x_max > lo_x && x_min < hi_x {
                return true;
            }
        } else if (p1.0 - p2.0).abs() < 0.01 {
            let x = p1.0;
            if x > lo_x && x < hi_x && y_max > lo_y && y_min < hi_y {
                return true;
            }
        }
    }
    false
}

fn path_crosses_component(
    pts: &[(f64, f64)],
    board: &Board,
    centres: &Centres,
    offsets: &Offsets,
) -> bool {
    for i in 0..pts.len().saturating_sub(1) {
        let p1 = pts[i];
        let p2 = pts[i + 1];
        if segment_crosses_component(p1, p2, board, centres, offsets) {
            return true;
        }
    }
    false
}

/// Build an orthogonal polyline path from `a` to `b` using the
/// shortest L (or straight line, or Z) that respects each pin's
/// stub direction and avoids crossing component bodies.
fn l_route_points(
    a: (f64, f64, f64, f64),
    b: (f64, f64, f64, f64),
    stub_len: f64,
    board: &Board,
    centres: &Centres,
    offsets: &Offsets,
) -> String {
    let (ax, ay, adx, ady) = a;
    let (bx, by, bdx, bdy) = b;
    let a_stub = (ax + adx * stub_len, ay + ady * stub_len);
    let b_stub = (bx + bdx * stub_len, by + bdy * stub_len);
    let a_horiz = adx.abs() > 0.5;
    let b_horiz = bdx.abs() > 0.5;

    let mut candidate_paths = Vec::new();

    if a_horiz && b_horiz {
        let opposite = adx * bdx < 0.0;
        if (a_stub.1 - b_stub.1).abs() < 0.5 {
            // Aligned y — straight horizontal stretch.
            candidate_paths.push(vec![(ax, ay), (bx, by)]);
        } else if opposite {
            // Option 1: Bend at a_stub.0
            candidate_paths.push(vec![(ax, ay), (a_stub.0, ay), (a_stub.0, by), (bx, by)]);
            // Option 2: Bend at b_stub.0
            candidate_paths.push(vec![(ax, ay), (b_stub.0, ay), (b_stub.0, by), (bx, by)]);
            // Option 3: Bend at mid_x (default)
            let mid_x = (ax + bx) / 2.0;
            candidate_paths.push(vec![(ax, ay), (mid_x, ay), (mid_x, by), (bx, by)]);
        } else {
            // Option 1: Bend at a_stub.0
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (a_stub.0, b_stub.1),
                b_stub,
                (bx, by),
            ]);
            // Option 2: Bend at b_stub.0
            candidate_paths.push(vec![(ax, ay), (b_stub.0, a_stub.1), b_stub, (bx, by)]);
            // Option 3: Bend at mid_x
            let mid_x = if adx > 0.0 {
                a_stub.0.max(b_stub.0)
            } else {
                a_stub.0.min(b_stub.0)
            };
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (mid_x, a_stub.1),
                (mid_x, b_stub.1),
                b_stub,
                (bx, by),
            ]);
        }
    } else if !a_horiz && !b_horiz {
        let opposite = ady * bdy < 0.0;
        if (a_stub.0 - b_stub.0).abs() < 0.5 {
            // Aligned x — straight vertical wire.
            candidate_paths.push(vec![(ax, ay), (bx, by)]);
        } else if opposite {
            // Option 1: Bend at a_stub.1
            candidate_paths.push(vec![(ax, ay), (ax, a_stub.1), (bx, a_stub.1), (bx, by)]);
            // Option 2: Bend at b_stub.1
            candidate_paths.push(vec![(ax, ay), (ax, b_stub.1), (bx, b_stub.1), (bx, by)]);
            // Option 3: Bend at mid_y (default)
            let mid_y = (ay + by) / 2.0;
            candidate_paths.push(vec![(ax, ay), (ax, mid_y), (bx, mid_y), (bx, by)]);
        } else {
            // Option 1: Bend at a_stub.1
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (b_stub.0, a_stub.1),
                b_stub,
                (bx, by),
            ]);
            // Option 2: Bend at b_stub.1
            candidate_paths.push(vec![(ax, ay), (a_stub.0, b_stub.1), b_stub, (bx, by)]);
            // Option 3: Bend at mid_y
            let mid_y = if ady > 0.0 {
                a_stub.1.max(b_stub.1)
            } else {
                a_stub.1.min(b_stub.1)
            };
            candidate_paths.push(vec![
                (ax, ay),
                a_stub,
                (a_stub.0, mid_y),
                (b_stub.0, mid_y),
                b_stub,
                (bx, by),
            ]);
        }
    } else if a_horiz {
        // a horizontal, b vertical
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, b_stub.1),
            b_stub,
            (bx, by),
        ]);
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (b_stub.0, a_stub.1),
            b_stub,
            (bx, by),
        ]);
    } else {
        // a vertical, b horizontal
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (b_stub.0, a_stub.1),
            b_stub,
            (bx, by),
        ]);
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, b_stub.1),
            b_stub,
            (bx, by),
        ]);
    }

    // Detours! If any standard candidate has a collision, detours will be evaluated.
    for offset in &[15.0, 25.0] {
        // Up detour
        let detour_y = a_stub.1.min(b_stub.1) - offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, detour_y),
            (b_stub.0, detour_y),
            b_stub,
            (bx, by),
        ]);
        // Down detour
        let detour_y2 = a_stub.1.max(b_stub.1) + offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (a_stub.0, detour_y2),
            (b_stub.0, detour_y2),
            b_stub,
            (bx, by),
        ]);
        // Left detour
        let detour_x = a_stub.0.min(b_stub.0) - offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (detour_x, a_stub.1),
            (detour_x, b_stub.1),
            b_stub,
            (bx, by),
        ]);
        // Right detour
        let detour_x2 = a_stub.0.max(b_stub.0) + offset;
        candidate_paths.push(vec![
            (ax, ay),
            a_stub,
            (detour_x2, a_stub.1),
            (detour_x2, b_stub.1),
            b_stub,
            (bx, by),
        ]);
    }

    // Deduplicate points and pick the first candidate that is collision-free.
    let mut selected_pts = None;
    for path in &candidate_paths {
        let mut clean_path: Vec<(f64, f64)> = Vec::new();
        for &p in path {
            if let Some(last) = clean_path.last() {
                if (p.0 - last.0).abs() < 0.01 && (p.1 - last.1).abs() < 0.01 {
                    continue;
                }
            }
            clean_path.push(p);
        }
        if !path_crosses_component(&clean_path, board, centres, offsets) {
            selected_pts = Some(clean_path);
            break;
        }
    }

    // Fallback: if all candidates have collisions, use the first clean path from candidate 3 (or the last option)
    let pts = selected_pts.unwrap_or_else(|| {
        let mut clean_path: Vec<(f64, f64)> = Vec::new();
        let default_path = vec![(ax, ay), (bx, by)];
        let path = candidate_paths.last().unwrap_or(&default_path);
        for &p in path {
            if let Some(last) = clean_path.last() {
                if (p.0 - last.0).abs() < 0.01 && (p.1 - last.1).abs() < 0.01 {
                    continue;
                }
            }
            clean_path.push(p);
        }
        clean_path
    });

    pts.iter()
        .map(|(x, y)| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Component centre in viewBox coordinates. The base position comes
/// from `synth_layout::layout` (cached in `centres`); the browser's
/// interactive drag offsets are applied on top.
fn component_center(component: &Component, centres: &Centres, offsets: &Offsets) -> (f64, f64) {
    let ((bx, by), _rot) = centres
        .get(&component.id)
        .copied()
        .unwrap_or(((0.0, 0.0), Rotation::Zero));
    let (dx, dy) = offsets.get(&component.id.0).copied().unwrap_or((0.0, 0.0));
    (bx + dx, by + dy)
}

fn component_rotation(component: &Component, centres: &Centres) -> Rotation {
    centres
        .get(&component.id)
        .map_or(Rotation::Zero, |(_, r)| *r)
}

fn endpoint_xyd(
    board: &Board,
    endpoint: &NetEndpoint,
    centres: &Centres,
    offsets: &Offsets,
) -> Option<(f64, f64, f64, f64)> {
    let component = board.component(endpoint.component)?;
    let part = component.part.as_ref()?;
    let (cx, cy) = component_center(component, centres, offsets);
    let rotation = component_rotation(component, centres);
    let layout = PinLayout::for_kind(part.kind.as_str(), Some(part), rotation, cx, cy);
    Some(layout.pin_tip(endpoint.pin.0 as usize, cx, cy))
}

fn svg_bounding_rect(svg_ref: NodeRef<leptos::svg::Svg>) -> Option<(f64, f64, f64, f64)> {
    let el = svg_ref.get()?;
    let svg = (*el).clone().dyn_into::<SvgsvgElement>().ok()?;
    let rect = svg.get_bounding_client_rect();
    Some((rect.width(), rect.height(), rect.left(), rect.top()))
}

#[cfg(test)]
mod tests {
    use synth_ir::ComponentId;
    use synth_layout::{ComponentPlacement, Layout, Rotation, SheetSize};

    use super::*;

    fn empty_layout(components: Vec<ComponentPlacement>) -> Layout {
        Layout {
            components,
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    #[test]
    fn apply_offsets_shifts_only_the_offset_component() {
        let layout = empty_layout(vec![
            ComponentPlacement {
                id: ComponentId(0),
                center_mm: (10.0, 10.0),
                rotation: Rotation::Zero,
            },
            ComponentPlacement {
                id: ComponentId(1),
                center_mm: (50.0, 20.0),
                rotation: Rotation::Ninety,
            },
        ]);
        let mut offsets: Offsets = Offsets::new();
        offsets.insert(0, (5.0, -3.0));

        let adjusted = apply_offsets_to_layout(&layout, &offsets);

        assert_eq!(adjusted.components.len(), 2);
        let moved = adjusted
            .components
            .iter()
            .find(|p| p.id == ComponentId(0))
            .unwrap();
        assert_eq!(moved.center_mm, (15.0, 7.0));
        assert_eq!(moved.rotation, Rotation::Zero);

        let untouched = adjusted
            .components
            .iter()
            .find(|p| p.id == ComponentId(1))
            .unwrap();
        assert_eq!(untouched.center_mm, (50.0, 20.0));
        assert_eq!(untouched.rotation, Rotation::Ninety);

        // Non-position fields carry through unchanged.
        assert_eq!(adjusted.sheet_size, layout.sheet_size);
        assert!(adjusted.wires.is_empty());
        assert!(adjusted.junctions.is_empty());
    }

    #[test]
    fn apply_offsets_with_empty_map_is_a_no_op() {
        let layout = empty_layout(vec![ComponentPlacement {
            id: ComponentId(0),
            center_mm: (1.0, 2.0),
            rotation: Rotation::OneEighty,
        }]);

        let adjusted = apply_offsets_to_layout(&layout, &Offsets::new());

        assert_eq!(adjusted, layout);
    }
}
