// SPDX-License-Identifier: Apache-2.0

//! Schematic placement and wire routing for the Synth EDA compiler.
//!
//! Both consumers — the browser preview (`synth-web`) and the
//! KiCad export (`synth-kicad`) — call [`layout`] on the same
//! [`Board`] to get component positions and (eventually) wire
//! routes. Keeping the layouter in one crate is the only way to
//! ensure the two views agree.
//!
//! ## Layered roadmap (§7.5 of the implementation plan)
//!
//! - **Slice 1A — foundation.** Deterministic auto grid, no
//!   semantic information.
//! - **Slice 1B — layered placement.** Power-flow heuristic
//!   assigns layers (rows); within-row order is IR-declaration.
//!   Replaced by Slice 2.
//! - **Slice 2 — cluster recognition (this file).** Sub-circuits
//!   like "MCU + its decoupling caps" are recognised and laid out
//!   as units; clusters are placed in a roughly-square 2D grid
//!   (not a long horizontal chain). This is the first slice that
//!   visibly approaches how a human draws a schematic.
//! - **Slice 3 — wire routing.** Channel routing on inter-cluster
//!   gaps; the `wires` field on [`Layout`] becomes populated.
//! - **Slice 4 — barycenter ordering + Brandes-Köpf coords.**
//!   Wire-crossing minimisation between cluster layers. Layer
//!   assignment + barycenter crossing reduction are implemented
//!   (`place_clusters`); coordinate assignment uses a full
//!   Brandes–Köpf pass (`bk_y_coordinates`) to position the
//!   barycenter-ordered columns/clusters with straight vertical
//!   alignment of aligned nodes, then snaps to the 2.54 mm grid.
//! - **Slice 5 — sidecar drag offsets.** Per-component overrides
//!   from `<source>.synth.layout.toml`.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    // ceil-of-sqrt-of-positive-usize is bounded, never negative.
    clippy::cast_sign_loss,
    // `build_clusters` is a sequence of independent recognition
    // passes; collapsing them into smaller fns would obscure the
    // overall order (LEDs → USB → ICs → singletons) which is the
    // whole point.
    clippy::too_many_lines,
)]

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use synth_ir::{Board, ComponentId, NetId, PinId};

pub mod kicad_footprint_loader;
pub mod kicad_lib_loader;
pub mod kicad_zip;
pub mod netclass;
pub mod ops;
mod patterns;
pub mod placer;
pub mod route;
pub mod score;
pub mod sheets;

pub use placer::{default_placer, NativeSemanticPlacer, Placer};

/// 90° rotations are the only orientations a schematic symbol may
/// take. V1 always returns [`Rotation::Zero`]; later slices may
/// rotate connectors so their pins face the board edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rotation {
    Zero,
    Ninety,
    OneEighty,
    TwoSeventy,
}

/// Standard schematic sheet sizes. `Custom` is for designs whose
/// content bounding box doesn't fit any of A4/A3/A2.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SheetSize {
    A4,
    A3,
    A2,
    Custom { width_mm: f64, height_mm: f64 },
}

impl SheetSize {
    pub fn dims_mm(self) -> (f64, f64) {
        match self {
            Self::A4 => (297.0, 210.0),
            Self::A3 => (420.0, 297.0),
            Self::A2 => (594.0, 420.0),
            Self::Custom {
                width_mm,
                height_mm,
            } => (width_mm, height_mm),
        }
    }
}

/// Final position + orientation of a single component on the sheet.
///
/// Coordinates are millimetres in the sheet's local coordinate
/// system; `(0, 0)` is the top-left of the page rect.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComponentPlacement {
    pub id: ComponentId,
    pub center_mm: (f64, f64),
    pub rotation: Rotation,
}

/// Orthogonal wire route between endpoints on the same net.
///
/// Populated by [`route::route_board`] (via [`layout`]) for every
/// signal net that routed successfully — one `WirePath` per
/// root-to-endpoint route. Consumers render each polyline directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WirePath {
    pub net: NetId,
    /// Connected sequence of `(x, y)` points in mm. Adjacent points
    /// always differ in exactly one axis (orthogonal routing).
    pub points: Vec<(f64, f64)>,
    /// T-connection junction dots, in mm. Only emitted for nets
    /// with ≥3 endpoints.
    pub junctions: Vec<(f64, f64)>,
}

/// Whether a power-flag symbol points up (positive rail) or down
/// (ground). The renderer draws the symbol attached *at* the pin
/// tip, growing in the indicated direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PowerFlagKind {
    /// Positive supply rail. Drawn as an upward arrow above the pin.
    Vcc,
    /// Ground reference. Drawn as a downward GND symbol below the pin.
    Gnd,
}

/// A power-rail flag attached to a single pin.
///
/// Power nets (GND, VCC, +3V3, …) would, drawn as wires, cross the
/// entire schematic and produce an unreadable tangle. Real
/// schematic editors render each endpoint of a power net as a tiny
/// standalone power symbol (arrow up for VCC, triangle down for
/// GND) instead of a wire. Consumers should:
///
/// - Skip rendering wires for any net that has flags on it (use
///   `Layout::power_net_ids` to discover those nets).
/// - At each `(component, pin)` in `power_flags`, render the
///   appropriate symbol with `label` text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PowerFlag {
    pub net: NetId,
    pub component: ComponentId,
    pub pin: PinId,
    pub kind: PowerFlagKind,
    /// Short label drawn next to the symbol ("GND", "VCC", …).
    pub label: String,
}

/// A text label attached to a single pin's stub naming a signal
/// net.
///
/// Used for signal nets that span too far to draw as a wire without
/// crossing other components. Both endpoints of the net carry a
/// label with the same name; the convention is that same-named
/// labels are electrically connected. This is the same primitive
/// real schematic editors use to break up long signal routes
/// without filling the page with crossing wires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetLabel {
    pub net: NetId,
    pub component: ComponentId,
    pub pin: PinId,
    /// Text drawn next to the pin's stub end. Same for every
    /// endpoint of the same net.
    pub label: String,
}

/// A cross-sheet signal net stub (§P26 multi-sheet).
///
/// Same identity as a [`NetLabel`] — one per in-sheet endpoint of a
/// net that continues on another sheet — but rendered as a KiCad
/// hierarchical label (paired with a pin on the parent sheet
/// instance) instead of a local label. Power nets never need one:
/// power symbols already connect globally by value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HierarchicalLabel {
    pub net: NetId,
    pub component: ComponentId,
    pub pin: PinId,
    /// Net name, identical on every sheet the net touches.
    pub label: String,
}

/// The complete layout output. Stable shape across slices — later
/// slices fill in `wires`, may reshape `sheet_size`, but never
/// rename or remove fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub components: Vec<ComponentPlacement>,
    pub wires: Vec<WirePath>,
    /// Junction dots where ≥3 wire segments meet, in mm. Populated by
    /// the canonical router (`crate::route::route_board`) alongside
    /// `wires`; consumers emit one dot symbol per entry.
    pub junctions: Vec<(f64, f64)>,
    pub power_flags: Vec<PowerFlag>,
    pub net_labels: Vec<NetLabel>,
    /// Cross-sheet signal stubs, one per in-sheet endpoint of a net
    /// that continues on another sheet. Empty on single-sheet
    /// layouts; populated by the §P26 split.
    #[serde(default)]
    pub hierarchical_labels: Vec<HierarchicalLabel>,
    /// Free text drawn on the sheet — sub-circuit captions, design
    /// notes (§21.1 `notes` blocks), and generated connector pin
    /// legends. Placement owns where prose lands; consumers render
    /// it verbatim.
    #[serde(default)]
    pub annotations: Vec<TextAnnotation>,
    /// Titled outline boxes, one per declared `group` (§21.1).
    #[serde(default)]
    pub group_boxes: Vec<GroupBox>,
    pub sheet_size: SheetSize,
}

/// A run of text placed on the sheet, in mm page coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextAnnotation {
    pub text: String,
    /// Left edge / baseline anchor of the text, in mm.
    pub at_mm: (f64, f64),
    /// Glyph height in mm. KiCad's schematic default is 1.27.
    pub size_mm: f64,
    /// What the run is. The exporter renders captions bold (the top
    /// of the Phase B typography hierarchy); every other kind renders
    /// plain at its authored size.
    #[serde(default)]
    pub kind: TextKind,
}

/// The role of a [`TextAnnotation`] run. Determines exporter styling
/// (bold captions) and documents the resolve pass's priority order
/// (caption > note > legend, larger glyphs first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    /// A declared `group`'s title: bold, group-hued box, inside top-left.
    Caption,
    /// A `notes` block title.
    NoteTitle,
    /// One line of a `notes` block.
    #[default]
    NoteLine,
    /// A generated `{refdes} pinout` legend title.
    LegendTitle,
    /// One `pin: net` legend line (or its `…` truncation marker).
    LegendLine,
}

/// A titled outline box drawn around one declared `group` (§21.1).
///
/// The caption names the sub-circuit; the box shows where it is.
/// Coordinates are mm page space, top-left `min_mm` to bottom-right
/// `max_mm` (KiCad y grows downward).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupBox {
    pub group: String,
    pub min_mm: (f64, f64),
    pub max_mm: (f64, f64),
    /// Outline + tint hue, assigned deterministically from the group
    /// name ([`group_color`]). The schematic exporter strokes the box
    /// dashed in this hue with a translucent fill of the same hue.
    #[serde(default = "default_group_color")]
    pub color: [u8; 3],
    /// The caption sits inside the box at the top-left (Phase B2).
    /// Always true; carried so other consumers (preview renderer)
    /// place captions without re-deriving the convention.
    #[serde(default = "default_caption_inside")]
    pub caption_inside: bool,
}

impl Layout {
    /// Set of net ids that the layouter classified as power nets.
    /// Consumers should skip rendering wires for any net id in this
    /// set — each endpoint already has a flag.
    pub fn power_net_ids(&self) -> HashSet<NetId> {
        self.power_flags.iter().map(|f| f.net).collect()
    }

    /// Set of net ids that carry net labels (long signals that
    /// would cross other components if drawn as wires).
    pub fn labeled_net_ids(&self) -> HashSet<NetId> {
        self.net_labels.iter().map(|l| l.net).collect()
    }
}

impl Layout {
    /// Find the placement of a component by id. `O(n)` linear scan;
    /// consumers that need many lookups should build their own
    /// `HashMap` once.
    pub fn placement(&self, id: ComponentId) -> Option<&ComponentPlacement> {
        self.components.iter().find(|p| p.id == id)
    }
}

// ----- Pin orientation -----------------------------------------------------

/// Which side of a component body a pin sits on. Drives both
/// rendering (where the stub emerges) and routing (which direction
/// the wire leaves the pin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PinSide {
    Left,
    Right,
    Top,
    Bottom,
}

// ----- Layout constants ----------------------------------------------------

/// Top-left of the cluster grid in viewBox / mm coordinates. Centered
/// vertically on an A4 sheet (210 mm height).
const GRID_ORIGIN_X: f64 = 55.0;
const GRID_ORIGIN_Y: f64 = 78.74;
/// Horizontal spacing between cluster anchors when clusters are
/// packed into a 2D grid. Each cell holds one anchor plus up to
/// `MEMBERS_PER_ROW` members in the row beneath it.
const BASE_CLUSTER_DX: f64 = 60.0;
/// Horizontal pitch floor between two columns of the *same* declared
/// group. Lower than `BASE_CLUSTER_DX`, which buys routing room
/// between unrelated blocks — columns of one sub-circuit are not
/// unrelated. Only boards that declare `group`s ever see this: an
/// ungrouped board bands by layer, and consecutive columns of one
/// layer are exactly the case `BASE_CLUSTER_DX` was tuned for.
const INTRA_GROUP_CLUSTER_DX: f64 = 40.0;
/// Vertical spacing between cluster rows. Must be large enough to
/// clear the anchor body, the power-flag stub above/below the
/// anchor, the member row below the anchor, and the member power
/// flags below that — call it ~70 mm with the current 2-pin
/// vertical cap symbol.
const BASE_CLUSTER_DY: f64 = 70.0;
/// Extra margin between body edges to leave room for wires +
/// reference/value labels + power flags. Added to the largest body
/// extent to size the cluster grid cell.
const WIRE_MARGIN: f64 = 30.0;
/// Snap a millimetre coordinate to the KiCad standard 2.54 mm
/// grid. Applied to every component centre and placement constant
/// so terminals, stubs and labels land on grid.
fn snap_grid(v: f64) -> f64 {
    (v / 2.54).round() * 2.54
}

/// Clearance between the anchor body edge and a cluster member's
/// centre. Leaves room for wire stubs + anchor labels + power flags.
const MEMBER_CLEARANCE: f64 = 17.78;
/// Spare margin between the page-frame border and any placed
/// component. KiCad's stock title-block leaves ~10–15 mm on each
/// side; 20 mm covers most variants.
const PAGE_MARGIN: f64 = 20.0;
/// Height of a group caption's glyphs. Captions render bold at this
/// size (Phase B typography: captions 2.0 bold, refdes/value 1.27,
/// note/legend lines 1.0) so a sub-circuit name reads as a heading
/// rather than as another component label.
const GROUP_CAPTION_SIZE: f64 = 2.0;
/// Gap between the box's top edge and the caption baseline, in mm:
/// one glyph height plus this gap keeps the caption fully inside the
/// box while clearing the edge stroke.
const GROUP_CAPTION_INSET: f64 = 1.0;
/// Padding between a group's outline box (§21.1) and its contents —
/// caption included, so the box top clears the caption baseline.
const GROUP_BOX_PAD: f64 = 5.0;
/// Title and line sizes for design notes (§21.1 `notes` blocks) and
/// generated connector pin legends. Phase B typography: titles stay
/// at 2.0 mm, lines drop to 1.0 mm so prose reads subordinate to
/// component labels (1.27 mm) and captions (2.0 mm bold).
const NOTE_TITLE_SIZE: f64 = 2.0;
const NOTE_LINE_SIZE: f64 = 1.0;
/// Baseline step between note/legend lines, and the gap between a
/// title baseline and its first line.
const NOTE_LINE_PITCH: f64 = 2.54;
const NOTE_TITLE_GAP: f64 = 4.0;
/// Gap below a group box, connector body, or content bottom before a
/// notes block or pin legend starts.
const BELOW_GAP: f64 = 8.0;
/// Gap between a group box's bottom edge and its first in-box note
/// title baseline's top (Phase B3): the title clears the edge stroke
/// before the box grows to enclose the strip.
const NOTE_STRIP_GAP: f64 = 1.0;
/// A connector earns a generated pin legend only with at least this
/// many pins joining *named* nets (Phase A3): fewer means an internal
/// header, not a board-edge interface.
const LEGEND_MIN_NAMED_PINS: usize = 4;
/// Maximum `pin: net` lines in one connector legend before an
/// ellipsis (Phase A3).
const LEGEND_MAX_LINES: usize = 8;

/// Fallback body extents when a component has no part info.
const BODY_FALLBACK_W: f64 = 15.0;
const BODY_FALLBACK_H: f64 = 10.0;
/// Horizontal spacing between adjacent members in the row below
/// an anchor. A vertical cap with two power flags occupies ~10 mm
/// horizontally; 14 mm keeps neighbouring caps readable.
const MEMBER_DX: f64 = 14.0;
/// Most shelves the fitting loop will fold a column band onto
/// (schematic-quality plan Phase A5/C3). Four is enough to turn any
/// band that fits A2 at all into a roughly page-shaped block; more
/// only costs fitting attempts.
const MAX_SHELVES: usize = 4;
/// Gap (mm) between two packed regions' *body* bounds.
///
/// Each region already draws its own padded, captioned box, so the
/// full inter-cluster pitch on top of that is dead space: at
/// `BASE_CLUSTER_DX` a three-region board spent ~80 mm on gaps and
/// tipped from A3 onto A2 with two thirds of the page blank. Eight
/// grid steps still clears both boxes' padding and captions — the
/// margin `E-SYNTH-SCHEM-013` checks — while reading as a deliberate
/// separation rather than a void.
const REGION_GAP: f64 = 20.32;
/// Candidate shelf widths (mm) for region packing, one per sheet the
/// exporter can declare (A4/A3/A2 content width). The fitting loop
/// tries each and keeps whichever lands on the smallest page.
const REGION_SHELF_WIDTHS: [f64; 3] = [
    297.0 - 2.0 * PAGE_MARGIN,
    420.0 - 2.0 * PAGE_MARGIN,
    594.0 - 2.0 * PAGE_MARGIN,
];
/// Target sheet aspect ratio (width/height) for cluster packing.
/// 1.4 ≈ A4 landscape.
const TARGET_ASPECT: f64 = 1.4;
/// Height (mm, measured up from the page's bottom edge) of the band
/// KiCad's default title block occupies. KiCad draws this itself
/// (fixed 108×32 mm rect anchored to the page's bottom-right corner —
/// see `drawing_sheet_default_description.cpp`'s `(rect (start 110 34)
/// (end 2 2))`) regardless of what we export, so placed content must
/// stay above `sheet_height - TITLE_BLOCK_H` to avoid visually
/// colliding with the title block. The band is sheet-relative: it is
/// the same 34 mm on A4, A3 and A2.
const TITLE_BLOCK_H: f64 = 34.0;

/// Clearance from the anchor's body edge to a member's *centre*,
/// sized to the member instead of to the largest part on the board
/// (schematic-quality plan Phase A2).
///
/// [`MEMBER_CLEARANCE`] is a single constant tuned for a large member
/// — enough room for its body, its Reference/Value text and a routing
/// channel. Applying it to an 0603 decoupling cap put the cap's
/// centre 17.78 mm below the IC's edge, a ~15 mm body gap, which read
/// as "floating near the IC" rather than "decoupling it" and tripped
/// `E-SYNTH-SCHEM-003` no matter how well the cluster was formed.
///
/// A member only needs its own half-extent, its text margin and one
/// grid step of channel. Clamped to [`MEMBER_CLEARANCE`] at the top
/// so nothing ever moves *further* out than before, and to five grid
/// steps at the bottom so the router keeps a usable channel.
fn member_clearance(board: &Board, id: ComponentId, vertical: bool) -> f64 {
    const MIN_CLEARANCE: f64 = 12.7;
    let (w, h) = board
        .component(id)
        .and_then(|c| c.part.as_ref())
        .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
    let half = if vertical { h / 2.0 } else { w / 2.0 };
    snap_grid((half + TEXT_MARGIN_Y + 2.54).clamp(MIN_CLEARANCE, MEMBER_CLEARANCE))
}

/// Clearance for the row below an anchor: the largest its members
/// need, so one oversized member never overlaps a tightly-placed
/// neighbour.
///
/// Only the *Below* row uses this. The Above row (bus pull-ups) and
/// the side columns (reset networks) keep the fixed
/// [`MEMBER_CLEARANCE`]: pulling those in crowds the anchor's own pin
/// stubs, and the pin-aware router responds by abandoning the route
/// and degrading the net to a label — a strictly worse drawing than
/// the few millimetres it saved.
fn row_clearance(board: &Board, members: &[ComponentId], vertical: bool) -> f64 {
    members
        .iter()
        .map(|&id| member_clearance(board, id, vertical))
        .fold(0.0_f64, f64::max)
        .max(12.7)
}

fn compute_dynamic_member_dx(board: &Board, members: &[ComponentId]) -> f64 {
    let mut max_label_len = 0_usize;
    for &id in members {
        if let Some(comp) = board.component(id) {
            if let Some(part) = comp.part.as_ref() {
                max_label_len = max_label_len.max(part.id.as_str().len().max(comp.refdes.len()));
            }
        }
    }
    let text_w = (max_label_len as f64) * 0.762 + 5.08;
    snap_grid(20.32_f64.max(text_w))
}

/// Refdes text sits above the body, value text below (§7.6.8's
/// angle-0 field pinning) — matches the obstacle-rect margin
/// `crate::route::route_board` uses when marking component+text
/// extents for the router, so placement and routing agree on how
/// much vertical room a component actually needs.
const TEXT_MARGIN_Y: f64 = 6.35;

/// A component's half-height including its Reference/Value text
/// labels, not just its body — the router already accounts for this
/// when marking obstacles (`route::route_board` step 1); placement
/// needs the same number wherever it decides vertical clearance
/// between components, or refdes/value text can visually overlap a
/// neighbour even though the bodies themselves don't.
fn text_inclusive_half_height(board: &Board, id: ComponentId) -> f64 {
    let (_, body_h) = board
        .component(id)
        .and_then(|c| c.part.as_ref())
        .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
    body_h / 2.0 + TEXT_MARGIN_Y
}

/// Minimum centre-to-centre vertical spacing between two vertically
/// stacked members (a Right/Left/Above column) so their text-
/// inclusive extents don't overlap, with a small readability margin.
fn min_member_dy(board: &Board, a: ComponentId, b: ComponentId) -> f64 {
    text_inclusive_half_height(board, a) + text_inclusive_half_height(board, b) + 2.0
}

/// A component's half-width including its Reference/Value text
/// labels, mirroring [`text_inclusive_half_height`] — used for
/// column spacing in [`place_clusters`]'s layered left-to-right
/// arrangement, the same way the height variant sizes row spacing.
/// Matches the obstacle-rect margin `route::route_board` marks for
/// the router.
///
/// Falls back to body-only extents when the component or its part is
/// missing; the resolved-part computation lives in
/// [`text_inclusive_half_width`], shared verbatim with the router.
fn component_text_inclusive_half_width(board: &Board, id: ComponentId) -> f64 {
    let Some(component) = board.component(id) else {
        return BODY_FALLBACK_W / 2.0;
    };
    let Some(part) = component.part.as_ref() else {
        // Without a part there is no Value text to resolve; only the
        // fallback body and the Reference label contribute.
        let refdes_w = (component.refdes.len() as f64) * 1.27 * 0.85 + 2.54;
        return (BODY_FALLBACK_W / 2.0).max(refdes_w / 2.0);
    };
    text_inclusive_half_width(component, part)
}

/// A component's half-width including its Reference/Value text
/// labels, for a component whose part has resolved.
///
/// This is the single source of truth shared between placement
/// ([`place_clusters`] column spacing, via
/// [`component_text_inclusive_half_width`]) and the router's
/// obstacle-marking step (`route::route_board` step 1): both must
/// reserve the same horizontal room for a placed part, or A*/L routes
/// can run straight through rendered text. Sizing the Value label off
/// `part.id` alone underestimates the rendered width whenever the
/// component carries a longer custom `value`, so the precedence below
/// mirrors `synth_kicad::schematic`'s `display_value` exactly
/// (`component.value` → `part.mpn` → `(no value)` sentinel).
pub(crate) fn text_inclusive_half_width(
    component: &synth_ir::Component,
    part: &synth_registry::Part,
) -> f64 {
    let (body_w, _) = body_size_for_part(part);
    let refdes_w = (component.refdes.len() as f64) * 1.27 * 0.85 + 2.54;
    let display_value = component
        .value
        .as_deref()
        .or(part.mpn.as_deref())
        .unwrap_or("(no value)");
    let val_w = (display_value.len() as f64) * 1.27 * 0.85 + 2.54;
    (body_w / 2.0).max(refdes_w / 2.0).max(val_w / 2.0)
}

pub mod sidecar;

/// After grid-snapping component centres, some stock KiCad symbols
/// (e.g. `Device:R`, `Device:C`) have pin origins that are half a
/// grid (1.27 mm) off relative to the symbol origin. Shift those
/// centres by ±1.27 mm so the pin terminals land cleanly on the
/// 2.54 mm grid.
fn align_to_pin_grid(_board: &Board, layout: &mut Layout) {
    for placement in &mut layout.components {
        placement.center_mm.0 = (placement.center_mm.0 / 1.27).round() * 1.27;
        placement.center_mm.1 = (placement.center_mm.1 / 1.27).round() * 1.27;
    }
}

// ----- Soft pin swapping (§7.7.2) ----------------------------------------

/// A soft pin-reassignment emitted during layout lowering (§7.7.2).
///
/// `pin_a` and `pin_b` are two *interchangeable* pins on `component`
/// (e.g. two GPIOs with matching capability sets, resistor-network
/// pins, or the gates of a quad-gate IC). The two signal nets currently
/// attached to them are swapped *for rendering only*: the `Board` IR
/// remains the connectivity source of truth, but a consumer that
/// routes/draws the schematic now attaches each net at the swapped
/// terminal, which removes a wire crossing without changing electrical
/// connectivity — because the pins are interchangeable, the net
/// assignment is equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SoftPinSwap {
    /// The IC whose two pins are being swapped.
    pub component: ComponentId,
    pub pin_a: PinId,
    pub pin_b: PinId,
}

/// Optional soft pin-swapping pass (§7.7.2): a post-placement
/// optimization over the already-placed [`Layout`] that swaps the two
/// interchangeable pins of an IC when doing so reduces the visual
/// crossing count of the signal nets attached to them, without
/// violating net constraints.
///
/// The pass models each signal net as the straight segment from its IC
/// pin terminal to the centroid of the net's *other* endpoints, counts
/// the intersections of the two candidate nets in their current vs
/// swapped assignments, and emits a [`SoftPinSwap`] when the swapped
/// assignment removes crossings.
///
/// Connectivity is never touched — `board` is read-only here. The
/// returned swaps are metadata a consumer applies when drawing/routing
/// so the visual route attaches each net at the swapped terminal. The
/// pass is deterministic: identical `(board, layout)` input yields
/// identical swaps.
pub fn soft_pin_swap_pass(board: &Board, layout: &Layout) -> Vec<SoftPinSwap> {
    let power_nets = layout.power_net_ids();
    let positions: std::collections::HashMap<ComponentId, (f64, f64)> = layout
        .components
        .iter()
        .map(|p| (p.id, p.center_mm))
        .collect();
    let mut swaps: Vec<SoftPinSwap> = Vec::new();
    let mut used: std::collections::HashSet<(ComponentId, PinId)> =
        std::collections::HashSet::new();

    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(&(cx, cy)) = positions.get(&component.id) else {
            continue;
        };
        for group in interchangeable_pin_groups(part) {
            for i in 0..group.len() {
                for j in (i + 1)..group.len() {
                    let (a, b) = (group[i], group[j]);
                    if used.contains(&(component.id, a)) || used.contains(&(component.id, b)) {
                        continue;
                    }
                    let Some((far_a, net_a)) =
                        signal_far_centroid(board, &positions, &power_nets, component.id, a)
                    else {
                        continue;
                    };
                    let Some((far_b, net_b)) =
                        signal_far_centroid(board, &positions, &power_nets, component.id, b)
                    else {
                        continue;
                    };
                    if net_a == net_b {
                        continue;
                    }
                    // Pin terminal positions (relative to the centre).
                    let (a_x, a_y, _) = compute_anchor_pin_offset(part, a.0 as usize);
                    let (b_x, b_y, _) = compute_anchor_pin_offset(part, b.0 as usize);
                    let pa = (cx + a_x, cy + a_y);
                    let pb = (cx + b_x, cy + b_y);
                    let current = segments_cross(pa, far_a, pb, far_b) as usize;
                    let swapped = segments_cross(pa, far_b, pb, far_a) as usize;
                    if swapped < current {
                        swaps.push(SoftPinSwap {
                            component: component.id,
                            pin_a: a,
                            pin_b: b,
                        });
                        used.insert((component.id, a));
                        used.insert((component.id, b));
                    }
                }
            }
        }
    }
    swaps
}

/// Compute the layout and, as an optional post-placement pass, run
/// [`soft_pin_swap_pass`] over it (§7.7.2). Returns the layout plus the
/// soft pin swaps that reduce net crossings.
///
/// This is the "optional pass wired into the pipeline" entry point;
/// [`layout`] itself is unchanged and does not run the swap pass, so
/// callers that do not care about pin swaps keep the exact same output.
pub fn layout_with_pin_swaps(board: &Board) -> (Layout, Vec<SoftPinSwap>) {
    let layout = layout(board);
    let swaps = soft_pin_swap_pass(board, &layout);
    (layout, swaps)
}

/// Partition an IC's pins into groups of *interchangeable* pins — same
/// electrical side, same capability set — that are safe to swap without
/// changing connectivity. Only lateral (left/right) pins qualify:
/// power pins and top/bottom pins are never freely reassignable.
fn interchangeable_pin_groups(part: &synth_registry::Part) -> Vec<Vec<PinId>> {
    use synth_registry::ElectricalType;
    let mut by_key: std::collections::BTreeMap<(PinSide, Vec<String>), Vec<PinId>> =
        std::collections::BTreeMap::new();
    for (idx, pin) in part.pins.iter().enumerate() {
        if matches!(
            pin.electrical_type,
            ElectricalType::PowerInput | ElectricalType::PowerOutput
        ) {
            continue;
        }
        let side = classify_ic_pin_layout(pin);
        if side == PinSide::Top || side == PinSide::Bottom {
            continue;
        }
        let mut caps: Vec<String> = pin.capabilities.iter().map(|c| format!("{c:?}")).collect();
        caps.sort_unstable();
        by_key
            .entry((side, caps))
            .or_default()
            .push(PinId(idx as u32));
    }
    by_key.into_values().filter(|g| g.len() >= 2).collect()
}

/// For a given IC pin, return the centroid of the signal net's *other*
/// endpoints and that net's id, skipping power nets and nets with no
/// other endpoint to measure against.
fn signal_far_centroid(
    board: &Board,
    positions: &std::collections::HashMap<ComponentId, (f64, f64)>,
    power_nets: &HashSet<NetId>,
    component: ComponentId,
    pin: PinId,
) -> Option<((f64, f64), NetId)> {
    for (net_id, net) in board.nets_containing(component, pin) {
        if power_nets.contains(&net_id) || net.endpoints.len() < 2 {
            continue;
        }
        let far: Vec<(f64, f64)> = net
            .endpoints
            .iter()
            .filter(|ep| ep.component != component)
            .filter_map(|ep| positions.get(&ep.component).copied())
            .collect();
        if far.is_empty() {
            continue;
        }
        let (mut sx, mut sy) = (0.0_f64, 0.0_f64);
        for &(x, y) in &far {
            sx += x;
            sy += y;
        }
        let centroid = (sx / far.len() as f64, sy / far.len() as f64);
        return Some((centroid, net_id));
    }
    None
}

/// True when two open segments properly intersect (strictly cross; a
/// shared endpoint or collinear touch does not count). Uses the
/// classic orientation test.
fn segments_cross(a1: (f64, f64), a2: (f64, f64), b1: (f64, f64), b2: (f64, f64)) -> bool {
    let orient = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| -> f64 {
        (q.1 - p.1) * (r.0 - q.0) - (q.0 - p.0) * (r.1 - q.1)
    };
    let d1 = orient(a1, a2, b1);
    let d2 = orient(a1, a2, b2);
    let d3 = orient(b1, b2, a1);
    let d4 = orient(b1, b2, a2);
    d1 != 0.0
        && d2 != 0.0
        && d3 != 0.0
        && d4 != 0.0
        && ((d1 > 0.0) != (d2 > 0.0))
        && ((d3 > 0.0) != (d4 > 0.0))
}

/// Compute the layout for `board`.
///
/// Output is deterministic.
/// Run the full layout pipeline with the default placer
/// ([`NativeSemanticPlacer`]).
pub fn layout(board: &Board) -> Layout {
    layout_with_placer(board, &default_placer())
}

/// Run the full layout pipeline with a caller-supplied Stage B
/// [`Placer`] (§7.8.3 / §7.8.5). Stage A recognition, power flags,
/// two-pin rotations and Stage C routing/labeling are identical for
/// every placer; only cluster placement is delegated.
pub fn layout_with_placer(board: &Board, placer: &dyn Placer) -> Layout {
    layout_with_overrides(board, placer, &|_| {})
}

/// Run the full pipeline with a caller-supplied [`Placer`] *and* a
/// user-override callback applied between placement and routing.
///
/// The override lands AFTER grid alignment / power-flag classification /
/// rotation passes but BEFORE `route_and_label`, so wires, junctions
/// and net labels are always computed against the final component
/// positions — dragging a component can never leave stale wires behind
/// (the preview reload bug), because there are no pre-existing wires
/// when the router runs. Overrides keep whatever center/rotation they
/// set; the router snaps pin terminals itself.
pub fn layout_with_overrides(
    board: &Board,
    placer: &dyn Placer,
    overlay: &dyn Fn(&mut Layout),
) -> Layout {
    let clusters = build_clusters(board);
    let mut layout = placer.place(board, &clusters);
    align_to_pin_grid(board, &mut layout);
    layout.power_flags = classify_power_flags(board);
    rotate_two_pin_with_power_flags(board, &mut layout);
    rotate_led_chains(board, &clusters, &mut layout);
    rotate_usb_esd_diodes(board, &clusters, &mut layout);
    lock_connector_rotations(board, &mut layout);
    overlay(&mut layout);
    route_and_label(board, &mut layout);
    annotate_groups(board, &mut layout);
    place_connector_legends(board, &mut layout);
    place_design_notes(board, &mut layout);
    resolve_text_overlaps(board, &mut layout);
    grow_sheet_to_fit(board, &mut layout);
    compact_sheet_to_fit(board, &mut layout);
    clamp_annotations_to_sheet(&mut layout);
    layout
}

/// Pull any caption that overhangs the right edge back onto the page.
///
/// A caption is anchored at its group's left edge, so a group near the
/// right margin can start on-page and still run off it — the text is
/// wider than the parts it names. `grow_sheet_to_fit` grows the page
/// for that where it can, but sheets stop at A2, and past there the
/// only remaining move is to slide the text left. Captions are
/// annotation, so shifting one costs nothing electrically; letting it
/// print half a name costs the reader the sub-circuit's identity.
fn clamp_annotations_to_sheet(layout: &mut Layout) {
    let (sheet_w, _) = layout.sheet_size.dims_mm();
    for text in &mut layout.annotations {
        let width = text.text.chars().count() as f64 * text.size_mm * 0.72;
        let max_x = sheet_w - PAGE_MARGIN - width;
        if text.at_mm.0 > max_x {
            text.at_mm.0 = max_x.max(PAGE_MARGIN);
        }
    }
}

/// Implicit region for mechanical and test parts that declare no
/// `group` (Phase D2): mounting holes, test points, and fiducials get
/// their own titled block, matching the reference sheet's `MOUNTING`
/// region, instead of trailing loose in the ungrouped region.
pub const MOUNTING_REGION: &str = "MOUNTING";

/// The region a component belongs to: its declared `group`, else the
/// implicit [`MOUNTING_REGION`] for a mechanical/test part, else
/// `None` (the trailing ungrouped region).
///
/// Public so consumers that reason about regions (the
/// `E-SYNTH-SCHEM-013` contiguity check) resolve a component to its
/// region exactly as placement does.
pub fn effective_group(board: &Board, id: ComponentId) -> Option<&str> {
    let component = board.component(id)?;
    if let Some(group) = component.group.as_deref() {
        return Some(group);
    }
    let kind = component.part.as_ref().map(|p| p.kind.as_str())?;
    matches!(kind, "mounting_hole" | "testpoint" | "fiducial").then_some(MOUNTING_REGION)
}

/// Caption every declared `group` on the sheet, and draw its titled
/// outline box (§21.1).
///
/// One text run per group, sitting inside the box at the top-left —
/// the device the SIM7080G reference schematic uses ("VBAT
/// DECOUPLING + ESD", "NANO SIM (1.8V only) + ESD"): a sub-circuit
/// is named where it is drawn, so a reader can see what a cluster of
/// parts is *for* without tracing nets. The box pads the component
/// bounds so the eye groups the parts even before reading the title.
/// Groups are declaration-order, and a board that declares none gets
/// no captions and no boxes.
///
/// The box carries a deterministic hue ([`group_color`], dark band
/// distinct from the net-class brights); the exporter strokes it
/// dashed in that hue with a translucent fill. Per-text color is not
/// expressible in KiCad's schematic grammar (verified against
/// `kicad-cli`: `(color …)` inside text effects fails to load), so
/// the caption itself renders bold monochrome — containment in the
/// hued box is what ties it to the sub-circuit.
///
/// Runs after routing so captions sit above the final positions, and
/// before `grow_sheet_to_fit` so a caption pushed near an edge grows
/// the page like any other content.
fn annotate_groups(board: &Board, layout: &mut Layout) {
    for (group, min_x, max_x, min_y, max_y) in group_bounds(board, layout) {
        let caption_y = (min_y - GROUP_BOX_PAD + GROUP_CAPTION_SIZE + GROUP_CAPTION_INSET)
            .max(GROUP_CAPTION_SIZE);
        // Header attributes (Phase D1): the display title when set,
        // and an explicit hue in place of the deterministic palette one.
        let declared = board.group(&group);
        let caption = declared.map_or_else(|| group.clone(), |g| g.display_title().to_string());
        let color = declared
            .and_then(|g| g.color)
            .unwrap_or_else(|| group_color(&group));
        layout.annotations.push(TextAnnotation {
            text: caption,
            // Inside the box at the top-left, one glyph plus an
            // inset below the top edge stroke.
            at_mm: (min_x, caption_y),
            size_mm: GROUP_CAPTION_SIZE,
            kind: TextKind::Caption,
        });
        layout.group_boxes.push(GroupBox {
            group,
            min_mm: (min_x - GROUP_BOX_PAD, min_y - GROUP_BOX_PAD),
            max_mm: (max_x + GROUP_BOX_PAD, max_y + GROUP_BOX_PAD),
            color,
            caption_inside: true,
        });
    }
}
/// Height (mm) a group's box adds above and below its members'
/// bodies: `(above, below)`.
///
/// Above is the box padding — the caption is drawn *inside* the box,
/// so it costs nothing extra. Below is the padding plus the note
/// block, which `place_design_notes` puts inside the box and grows
/// the box to hold. Region packing needs both or boxes overlap.
fn group_box_overhang(board: &Board, group: Option<&str>) -> (f64, f64) {
    let Some(group) = group else {
        return (0.0, 0.0);
    };
    let notes: f64 = board
        .notes
        .iter()
        .filter(|n| n.group.as_deref() == Some(group))
        .map(|n| NOTE_TITLE_GAP + n.lines.len() as f64 * NOTE_LINE_PITCH)
        .sum();
    (GROUP_BOX_PAD, GROUP_BOX_PAD + notes)
}

/// Dark-band hues for group boxes, distinct from the net-class
/// brights (`synth_kicad::netclass_colors`): boxes are large areas,
/// so they take muted tones with a translucent fill while nets take
/// saturated signal colors.
const GROUP_PALETTE: [[u8; 3]; 8] = [
    [0x8B, 0x00, 0x00], // dark red
    [0x00, 0x64, 0x00], // dark green
    [0x00, 0x00, 0x8B], // dark blue
    [0x8B, 0x45, 0x13], // saddle brown
    [0x8B, 0x00, 0x8B], // dark magenta
    [0x00, 0x80, 0x80], // teal
    [0x80, 0x80, 0x00], // olive
    [0x4B, 0x00, 0x82], // indigo
];

/// Neutral packing key for a region with no `region` hint: unhinted
/// groups sit in the middle of the shelf order, keeping declaration
/// order among themselves.
const QUADRANT_NEUTRAL: u8 = 4;

/// Packing key for a pinned page quadrant (Phase D1). Lower sorts
/// earlier in the shelf flow, so `top_left` packs first and
/// `bottom_right` last.
fn quadrant_key(region: &synth_ir::PlacementRegion) -> u8 {
    use synth_ir::PlacementRegion as R;
    match region {
        R::TopLeft => 0,
        R::TopEdge => 1,
        R::TopRight => 2,
        R::LeftEdge => 3,
        R::Centre => QUADRANT_NEUTRAL,
        R::RightEdge => 5,
        R::BottomLeft => 6,
        R::BottomEdge => 7,
        R::BottomRight => 8,
    }
}

/// Deterministic hue for a group name: FNV-1a into
/// [`GROUP_PALETTE`]. Same name, same hue, every export —
/// `DefaultHasher` would not promise that across processes.
pub fn group_color(name: &str) -> [u8; 3] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    GROUP_PALETTE[(hash % GROUP_PALETTE.len() as u64) as usize]
}

/// Serde default for [`GroupBox::color`]: the hue of the empty name,
/// so hand-built boxes without a color still land in the palette.
fn default_group_color() -> [u8; 3] {
    group_color("")
}

/// Serde default for [`GroupBox::caption_inside`]: captions live
/// inside their box (Phase B2).
fn default_caption_inside() -> bool {
    true
}

/// Bounding box of each declared `group`'s component bodies, in
/// first-seen placement order: `(group, min_x, max_x, min_y, max_y)`.
/// Shared by captions, boxes, and group-placed design notes so the
/// three can never disagree about where a group is.
fn group_bounds(board: &Board, layout: &Layout) -> Vec<(String, f64, f64, f64, f64)> {
    let mut order: Vec<String> = Vec::new();
    let mut bounds: std::collections::HashMap<String, (f64, f64, f64, f64)> =
        std::collections::HashMap::new();
    for placement in &layout.components {
        let Some(group) = effective_group(board, placement.id).map(str::to_string) else {
            continue;
        };
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
        let entry = bounds.entry(group.clone()).or_insert_with(|| {
            order.push(group);
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            )
        });
        entry.0 = entry.0.min(cx - bw / 2.0);
        entry.1 = entry.1.max(cx + bw / 2.0);
        entry.2 = entry.2.min(cy - bh / 2.0);
        entry.3 = entry.3.max(cy + bh / 2.0);
    }
    order
        .into_iter()
        .map(|group| {
            let (min_x, max_x, min_y, max_y) = bounds[&group];
            (group, min_x, max_x, min_y, max_y)
        })
        .collect()
}

/// Pin legend for opt-in board-edge connectors (schematic-quality
/// plan Phase A3, §21.1).
///
/// One titled block per qualifying connector — `{refdes} pinout` plus
/// one `pin: net` line per pin on a *named* net, capped at
/// [`LEGEND_MAX_LINES`] lines with an ellipsis. A reader checking a
/// harness against the schematic reads the mating list where the
/// connector is drawn instead of tracing each stub.
///
/// Three filters keep the legend from becoming the sheet's largest
/// text block (defect D4):
///
/// - Opt-in: nothing is emitted unless the board declares
///   `legends on` (default off). The reference sheet carries a
///   one-line prose note (*"Silk order: VIN 3Vo GND SCL SDA"*) instead
///   — that is what `notes` is for.
/// - Board-edge only: the connector must have at least
///   [`LEGEND_MIN_NAMED_PINS`] pins joining *named* nets. An internal
///   header on auto-named nets gets no legend.
/// - Compact: `NC` lines are dropped entirely (a no-connect cross on
///   the pin already says it), and long pinouts truncate with `…`.
///
/// Runs after routing (positions are final) and before
/// `grow_sheet_to_fit` so a legend grows the page like any other
/// content.
fn place_connector_legends(board: &Board, layout: &mut Layout) {
    if !board.legends {
        return;
    }
    for placement in &layout.components {
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if part.kind != "connector" {
            continue;
        }
        let mut lines: Vec<(String, String)> = Vec::new();
        for (index, pin) in part.pins.iter().enumerate() {
            // u32 cast is bounded: the index comes from the part's own pin list.
            let pid = PinId(index as u32);
            let Some((_, net)) = board.nets_containing(component.id, pid).next() else {
                continue;
            };
            if is_auto_net_name(&net.name) {
                continue;
            }
            lines.push((pin.name.clone(), net.name.clone()));
        }
        if lines.len() < LEGEND_MIN_NAMED_PINS {
            continue;
        }
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = body_size_for_part(part);
        let x = cx - bw / 2.0;
        let mut y = cy + bh / 2.0 + BELOW_GAP;
        layout.annotations.push(TextAnnotation {
            text: format!("{} pinout", component.refdes),
            at_mm: (x, y),
            size_mm: NOTE_TITLE_SIZE,
            kind: TextKind::LegendTitle,
        });
        y += NOTE_TITLE_GAP;
        for (pin_name, net_name) in lines.iter().take(LEGEND_MAX_LINES) {
            layout.annotations.push(TextAnnotation {
                text: format!("{pin_name}: {net_name}"),
                at_mm: (x, y),
                size_mm: NOTE_LINE_SIZE,
                kind: TextKind::LegendLine,
            });
            y += NOTE_LINE_PITCH;
        }
        if lines.len() > LEGEND_MAX_LINES {
            layout.annotations.push(TextAnnotation {
                text: "…".to_string(),
                at_mm: (x, y),
                size_mm: NOTE_LINE_SIZE,
                kind: TextKind::LegendLine,
            });
        }
    }
}

/// Whether a net name is an auto-generated `net_<idx>` placeholder
/// rather than a human name. Legends only list named nets.
fn is_auto_net_name(name: &str) -> bool {
    name.strip_prefix("net_")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Render `notes` blocks as titled text (§21.1).
///
/// A note inside a `group` sits in a strip at the bottom *inside*
/// that group's outline box (reference mechanism 3: intent lives
/// where the sub-circuit is drawn); the box grows to fit. Top-level
/// notes stack at the bottom-left below all content. Each block is
/// its title (caption size) plus one run per line. Runs before
/// `grow_sheet_to_fit` so notes near an edge grow the page, and
/// before `clamp_annotations_to_sheet` so wide lines slide on-page
/// like captions.
fn place_design_notes(board: &Board, layout: &mut Layout) {
    if board.notes.is_empty() {
        return;
    }
    let mut board_cursor = content_bottom(board, layout) + BELOW_GAP;
    for note in &board.notes {
        if let Some(group) = note.group.as_deref() {
            // Index — not a reference: the box grows as notes land.
            let Some(box_idx) = layout.group_boxes.iter().position(|b| b.group == group) else {
                let y = board_cursor;
                board_cursor += NOTE_TITLE_GAP + note.lines.len() as f64 * NOTE_LINE_PITCH;
                push_note_runs(layout, note, PAGE_MARGIN, y);
                continue;
            };
            // Strip below the current box bottom edge, inside the
            // grown box: title first, lines beneath it.
            let title_y = layout.group_boxes[box_idx].max_mm.1 + NOTE_STRIP_GAP + NOTE_TITLE_SIZE;
            let x = layout.group_boxes[box_idx].min_mm.0 + GROUP_BOX_PAD;
            push_note_runs(layout, note, x, title_y);
            let last_y = title_y
                + NOTE_TITLE_GAP
                + note.lines.len().saturating_sub(1) as f64 * NOTE_LINE_PITCH;
            layout.group_boxes[box_idx].max_mm.1 = last_y + GROUP_BOX_PAD;
        } else {
            let y = board_cursor;
            board_cursor += NOTE_TITLE_GAP + note.lines.len() as f64 * NOTE_LINE_PITCH;
            push_note_runs(layout, note, PAGE_MARGIN, y);
        }
    }
}

/// Push one titled note block's runs: the title at `(x, y)`, one run
/// per line beneath it.
fn push_note_runs(layout: &mut Layout, note: &synth_ir::Note, x: f64, y: f64) {
    layout.annotations.push(TextAnnotation {
        text: note.title.clone(),
        at_mm: (x, y),
        size_mm: NOTE_TITLE_SIZE,
        kind: TextKind::NoteTitle,
    });
    let mut line_y = y + NOTE_TITLE_GAP;
    for line in &note.lines {
        layout.annotations.push(TextAnnotation {
            text: line.clone(),
            at_mm: (x, line_y),
            size_mm: NOTE_LINE_SIZE,
            kind: TextKind::NoteLine,
        });
        line_y += NOTE_LINE_PITCH;
    }
}

/// Advance width of a text run in mm. KiCad's stroke font is about
/// 0.72 em — shared by [`grow_sheet_to_fit`] and the overlap pass so
/// placement and page sizing agree on how wide a run is.
fn text_run_width(text: &str, size_mm: f64) -> f64 {
    text.chars().count() as f64 * size_mm * 0.72
}

/// Axis-aligned box of a text run: `(x0, y0, x1, y1)` with `y` the
/// baseline anchor (the box spans one glyph height above it).
fn text_run_rect(text: &TextAnnotation) -> (f64, f64, f64, f64) {
    let (x, y) = text.at_mm;
    (
        x,
        y - text.size_mm,
        x + text_run_width(&text.text, text.size_mm),
        y,
    )
}

/// True when two rects overlap with more than float noise in common.
/// Touching edges do not count — adjacent note lines at
/// [`NOTE_LINE_PITCH`] must never flag.
fn rects_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    const EPS: f64 = 1e-6;
    a.0 < b.2 - EPS && b.0 < a.2 - EPS && a.1 < b.3 - EPS && b.1 < a.3 - EPS
}

/// Vertical nudge step for overlap resolution: half the schematic
/// grid so runs land back on grid after an even number of steps.
const TEXT_NUDGE_STEP: f64 = 1.27;
/// How far one run moves before the pass gives up on nudging it
/// (then shrinks, then drops — see below).
const TEXT_MAX_NUDGES: usize = 16;
/// Shrunk glyph height for line runs that cannot be nudged clear.
/// Titles and captions (2.0 mm) are never shrunk — only nudged or
/// left for `E-SYNTH-SCHEM-011` to report.
const TEXT_SHRUNK_SIZE: f64 = 0.8;

/// Resolve overlapping free-text runs (schematic-quality plan Phase
/// A4, defect D5).
///
/// Builds axis-aligned boxes for every annotation (captions, note
/// lines, legend lines) plus component bodies as fixed obstacles,
/// then places runs largest-first (captions and titles before body
/// text — the plan's priority with refdes/value/net-label kinds owned
/// by the exporter, which places those after layout): each run keeps
/// its authored position when free, otherwise nudges down along the
/// free axis; a line run that still overlaps after
/// [`TEXT_MAX_NUDGES`] steps shrinks one glyph step and retries; a
/// line run that still overlaps is dropped. Title-size runs are never
/// shrunk or dropped — if one cannot be placed clear it stays where
/// it was authored and `E-SYNTH-SCHEM-011` reports the residue.
///
/// Runs after `place_design_notes` (every run exists) and before
/// `grow_sheet_to_fit` (nudged runs grow the page like any content).
/// Deterministic: input order breaks all ties.
fn resolve_text_overlaps(board: &Board, layout: &mut Layout) {
    if layout.annotations.is_empty() {
        return;
    }
    // Fixed obstacles: component bodies including their Reference /
    // Value text margin, so a nudged run never lands on a part.
    let mut placed: Vec<(f64, f64, f64, f64)> = layout
        .components
        .iter()
        .map(|p| {
            let (cx, cy) = p.center_mm;
            let (bw, bh) = board
                .component(p.id)
                .and_then(|c| c.part.as_ref())
                .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
            (
                cx - bw / 2.0,
                cy - bh / 2.0 - TEXT_MARGIN_Y,
                cx + bw / 2.0,
                cy + bh / 2.0 + TEXT_MARGIN_Y,
            )
        })
        .collect();
    // Largest glyphs first; authored order breaks ties (stable sort).
    let mut order: Vec<usize> = (0..layout.annotations.len()).collect();
    order.sort_by(|&a, &b| {
        layout.annotations[b]
            .size_mm
            .total_cmp(&layout.annotations[a].size_mm)
    });
    let mut drop: Vec<bool> = vec![false; layout.annotations.len()];
    for &idx in &order {
        let origin = layout.annotations[idx].clone();
        let mut size = origin.size_mm;
        let mut shrunk = false;
        let droppable = size < NOTE_TITLE_SIZE;
        loop {
            let mut y = origin.at_mm.1;
            let mut nudges = 0_usize;
            while nudges < TEXT_MAX_NUDGES
                && placed.iter().any(|&r| {
                    rects_overlap(
                        (
                            origin.at_mm.0,
                            y - size,
                            origin.at_mm.0 + text_run_width(&origin.text, size),
                            y,
                        ),
                        r,
                    )
                })
            {
                y += TEXT_NUDGE_STEP;
                nudges += 1;
            }
            let rect = (
                origin.at_mm.0,
                y - size,
                origin.at_mm.0 + text_run_width(&origin.text, size),
                y,
            );
            if !placed.iter().any(|&r| rects_overlap(rect, r)) {
                layout.annotations[idx].at_mm.1 = y;
                layout.annotations[idx].size_mm = size;
                placed.push(rect);
                break;
            }
            if !shrunk && droppable {
                size = TEXT_SHRUNK_SIZE;
                shrunk = true;
                continue;
            }
            if droppable {
                drop[idx] = true;
            } else {
                // Title-size runs are never shrunk or dropped: keep
                // the authored position so the caption still names its
                // group, but reserve it so smaller runs stay clear.
                placed.push(text_run_rect(&origin));
            }
            break;
        }
    }
    if drop.iter().any(|&d| d) {
        let mut kept = Vec::with_capacity(layout.annotations.len());
        for (i, run) in layout.annotations.drain(..).enumerate() {
            if !drop[i] {
                kept.push(run);
            }
        }
        layout.annotations = kept;
    }
}

/// Lowest content baseline on the sheet: component bodies, wire
/// points, annotations, and group boxes. Board-level design notes
/// start below this.
fn content_bottom(board: &Board, layout: &Layout) -> f64 {
    let mut bottom = f64::NEG_INFINITY;
    for placement in &layout.components {
        let bh = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or(BODY_FALLBACK_H, |part| body_size_for_part(part).1);
        bottom = bottom.max(placement.center_mm.1 + bh / 2.0);
    }
    for wire in &layout.wires {
        for &(_, y) in &wire.points {
            bottom = bottom.max(y);
        }
    }
    for text in &layout.annotations {
        bottom = bottom.max(text.at_mm.1);
    }
    for box_ in &layout.group_boxes {
        bottom = bottom.max(box_.max_mm.1);
    }
    if bottom.is_finite() {
        bottom
    } else {
        PAGE_MARGIN
    }
}

/// Bounding box of everything drawn on the sheet — component
/// bodies, wire points, text runs, and group boxes — as
/// `(min_x, max_x, min_y, max_y)` in mm page coordinates, or `None`
/// for an empty layout. Shared by [`grow_sheet_to_fit`],
/// [`compact_sheet_to_fit`], and the `E-SYNTH-SCHEM-012` fill rule so
/// page sizing and the fill measurement can never disagree about
/// where the content is.
pub fn content_bounds(board: &Board, layout: &Layout) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for placement in &layout.components {
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
        min_x = min_x.min(cx - bw / 2.0);
        max_x = max_x.max(cx + bw / 2.0);
        min_y = min_y.min(cy - bh / 2.0);
        max_y = max_y.max(cy + bh / 2.0);
    }
    for wire in &layout.wires {
        for &(x, y) in &wire.points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    for text in &layout.annotations {
        let (x, y) = text.at_mm;
        // Rough advance width: KiCad's stroke font is about 0.72 em.
        let width = text_run_width(&text.text, text.size_mm);
        min_x = min_x.min(x);
        max_x = max_x.max(x + width);
        min_y = min_y.min(y - text.size_mm);
        max_y = max_y.max(y);
    }
    for box_ in &layout.group_boxes {
        min_x = min_x.min(box_.min_mm.0);
        max_x = max_x.max(box_.max_mm.0);
        min_y = min_y.min(box_.min_mm.1);
        max_y = max_y.max(box_.max_mm.1);
    }
    if !min_x.is_finite() {
        return None;
    }
    Some((min_x, max_x, min_y, max_y))
}

/// Grow `layout.sheet_size` if anything ended up past the edge of the
/// page the placer chose.
///
/// The placer sizes the sheet from the positions *it* assigns, but
/// three later stages can move content: grid alignment and the
/// rotation passes nudge components, `overlay` can drop one anywhere
/// the user dragged it, and routing adds wire points of its own. A
/// sidecar override in particular is unbounded — nothing stops a
/// dragged component from landing past the right edge — and without
/// this pass the page stayed whatever the placer picked, so the
/// component simply rendered off-sheet (`E-SYNTH-SCHEM-007`).
///
/// Only ever grows, never shrinks: a page that shrank under a manual
/// arrangement would move everything the user had just positioned by
/// hand relative to the frame. A layout that is merely roomier than it
/// needs to be is fine; one whose content hangs off the page is not.
/// (The Phase A5 companion [`compact_sheet_to_fit`] shrinks only to a
/// sheet the content provably fits, so the two never fight.)
fn grow_sheet_to_fit(board: &Board, layout: &mut Layout) {
    let Some((min_x, max_x, min_y, max_y)) = content_bounds(board, layout) else {
        return;
    };
    let (need_w, need_h) = sheet_needs(min_x, max_x, min_y, max_y);
    let required = sheet_size_for(need_w, need_h);
    let (have_w, have_h) = layout.sheet_size.dims_mm();
    let (want_w, want_h) = required.dims_mm();
    if want_w > have_w || want_h > have_h {
        layout.sheet_size = required;
    }
}

/// Shrink `layout.sheet_size` to the smallest standard sheet the
/// content provably fits (schematic-quality plan Phase A5, defect
/// D6: content occupying the top 40 % of an A3 page while the bottom
/// half sits empty).
///
/// Runs after [`grow_sheet_to_fit`], so the sheet first covers the
/// content and then compacts onto it. Shrinking only re-declares the
/// frame — component positions are absolute from the top-left, so
/// nothing moves — and only happens when [`sheet_needs`] (which
/// already reserves the page margin and the title-block band) fits
/// the smaller sheet, so compacted content can neither overflow the
/// page nor slide under the title block. Sidecar-dragged components
/// are content like any other: if they fit a smaller sheet the sheet
/// shrinks around them; if not, it stays.
fn compact_sheet_to_fit(board: &Board, layout: &mut Layout) {
    let Some((min_x, max_x, min_y, max_y)) = content_bounds(board, layout) else {
        return;
    };
    let (need_w, need_h) = sheet_needs(min_x, max_x, min_y, max_y);
    let smallest = sheet_size_for(need_w, need_h);
    // `sheet_size_for` ceilings at A2: beyond that the "smallest"
    // does not actually contain the content, so check the fit
    // explicitly instead of trusting the name.
    let (want_w, want_h) = smallest.dims_mm();
    if need_w > want_w || need_h > want_h {
        return;
    }
    let (have_w, have_h) = layout.sheet_size.dims_mm();
    if want_w < have_w && want_h < have_h {
        layout.sheet_size = smallest;
    }
}

/// Classify net labels and route every signal net over `layout`'s
/// current component positions, overwriting `net_labels`, `wires`
/// and `junctions` in place.
///
/// Factored out of [`layout`] so [`apply_op`] can re-run just this
/// step after a Stage E mutation (§7.8.8) changes where a component
/// sits, without re-running cluster recognition or placement
/// (Stages A/B) — connectivity (`board`) is never touched here, only
/// the visual routing derived from it.
pub(crate) fn route_and_label(board: &Board, layout: &mut Layout) {
    // Labels are classified AFTER placement so we know each
    // endpoint's coordinates and can measure span.
    layout.net_labels = classify_net_labels(board, layout);
    // Route every signal net over the placed sheet. Both consumers —
    // the browser preview and the KiCad export — share this router.
    let route = route::route_board(board, layout);
    layout.wires = route.wires;
    layout.junctions = route.junctions;
    // Nets truncated to per-endpoint net labels instead of wires —
    // either unroutable (every L-route candidate crossed a body, A*
    // found nothing) or routed but crossing too many other nets
    // (§7.7.3) — get the same escape hatch a human uses when a wire
    // would tangle. Same-named labels connect them electrically.
    for net_id in route.nets_truncated_to_labels {
        let Some(net) = board.net(net_id) else {
            continue;
        };
        let text = pick_net_label(board, net).unwrap_or_else(|| format!("NET_{}", net_id.0));
        for ep in &net.endpoints {
            layout.net_labels.push(NetLabel {
                net: net_id,
                component: ep.component,
                pin: ep.pin,
                label: text.clone(),
            });
        }
    }
    // Distinct nets must never render the same label string — KiCad
    // merges same-named local labels into one net. The uniquify pass
    // is a no-op on collision-free boards.
    uniquify_net_labels(board, &mut layout.net_labels);
}

/// Connectors must preserve ascending pin numbers (1 → 2 → 3 → 4) top-to-bottom.
/// Locking connector placement to Rotation::Zero prevents pin-order flipping.
fn lock_connector_rotations(board: &Board, layout: &mut Layout) {
    for placement in &mut layout.components {
        if let Some(comp) = board.component(placement.id) {
            if let Some(part) = comp.part.as_ref() {
                if part.kind == "connector" {
                    placement.rotation = Rotation::Zero;
                }
            }
        }
    }
}

/// Compute layout and overlay overrides from `<design>.synth.layout.toml` if present.
///
/// This is the canonical entry point for every consumer that must
/// honour manual tuning — preview reload AND KiCad export alike.
/// Overrides move components between placement and routing, so the
/// returned layout is fully re-routed against the dragged positions.
pub fn layout_with_sidecar(board: &Board, sidecar_path: Option<&std::path::Path>) -> Layout {
    let Some(path) = sidecar_path else {
        return layout(board);
    };
    let Some(sidecar) = sidecar::SidecarLayout::load_from_file(path) else {
        return layout(board);
    };
    let refdes_to_id: std::collections::HashMap<String, ComponentId> = board
        .components
        .iter()
        .map(|c| (c.refdes.clone(), c.id))
        .collect();
    let mut l = layout_with_overrides(board, &default_placer(), &|l| {
        sidecar.apply_to_layout(board, l, &refdes_to_id);
    });
    // Forced net labels are applied *after* routing, because
    // `route_and_label` recomputes wires and labels for the whole board. Doing
    // it here is what lets a net an agent pinned to a label survive a later
    // structural op (see `ops` module docs).
    sidecar.apply_forced_labels(board, &mut l);
    sidecar.apply_sheet_fit(board, &mut l);
    l
}

/// Force `net` to render as per-endpoint net-label stubs instead of a wire,
/// regardless of span or crossing count.
///
/// Shared by [`ops::LayoutOp::ReplaceWireWithLabel`] and sidecar persistence,
/// so a manually forced net cannot silently revert to the automatic
/// distance/crossing heuristic on the next structural edit.
pub fn force_net_label(board: &Board, layout: &mut Layout, net: NetId) {
    let Some(net_ir) = board.net(net) else {
        return;
    };
    layout.wires.retain(|w| w.net != net);
    layout.net_labels.retain(|l| l.net != net);
    let text = pick_net_label(board, net_ir).unwrap_or_else(|| format!("NET_{}", net.0));
    for ep in &net_ir.endpoints {
        layout.net_labels.push(NetLabel {
            net,
            component: ep.component,
            pin: ep.pin,
            label: text.clone(),
        });
    }
    // A hand-forced label can collide with labels the last routing pass
    // produced; re-run the uniqueness pass so the sheet-wide guarantee holds.
    uniquify_net_labels(board, &mut layout.net_labels);
}

/// USB ESD diodes sit in a column to the LEFT of the USB
/// connector. Rotate each to `OneEighty` so the anode (pin 0) ends
/// up on the right-hand side of the diode, facing the connector.
/// The cathode (pin 1) ends up on the left, where its GND symbol
/// can extend horizontally without crossing anything.
///
/// Overrides the generic 2-pin power-flag rotation, which would
/// otherwise rotate these diodes to `TwoSeventy` (cathode-down).
/// USB-side placement wants horizontal, not vertical.
fn rotate_usb_esd_diodes(board: &Board, clusters: &[Cluster], layout: &mut Layout) {
    use std::collections::HashMap;
    let mut rot: HashMap<ComponentId, Rotation> = HashMap::new();
    for cluster in clusters {
        let Some(anchor) = board.component(cluster.anchor) else {
            continue;
        };
        let Some(part) = anchor.part.as_ref() else {
            continue;
        };
        if part.kind != "connector" {
            continue;
        }
        for member in &cluster.members {
            if member.side != MemberSide::Left {
                continue;
            }
            let Some(member_component) = board.component(member.id) else {
                continue;
            };
            if member_component
                .part
                .as_ref()
                .is_some_and(|p| p.kind == "diode")
            {
                rot.insert(member.id, Rotation::OneEighty);
            }
        }
    }
    for placement in &mut layout.components {
        if let Some(&r) = rot.get(&placement.id) {
            placement.rotation = r;
        }
    }
}

/// LED-indicator clusters get rendered as a vertical chain
/// (current-limit resistor on top, LED below). Rotate both
/// components 270° (CCW) so pin 0 ends up on top — for the LED
/// that's the anode, for the resistor it's the signal-side pin.
fn rotate_led_chains(board: &Board, clusters: &[Cluster], layout: &mut Layout) {
    use std::collections::HashMap;
    let mut rot: HashMap<ComponentId, Rotation> = HashMap::new();
    for cluster in clusters {
        if !cluster.anchor_vertical {
            continue;
        }
        // Anchor (the LED): pin 0 is the anode, which should point
        // up toward the signal source.
        rot.insert(cluster.anchor, Rotation::TwoSeventy);
        // Resistor members of an LED-indicator cluster: pin 0 is
        // signal-side, pin 1 connects down to the LED's anode.
        // Rotate the same direction so the resistor's body sits
        // between signal and LED.
        for member in &cluster.members {
            if member.side != MemberSide::Above {
                continue;
            }
            let Some(component) = board.component(member.id) else {
                continue;
            };
            if component
                .part
                .as_ref()
                .is_some_and(|p| p.kind == "resistor")
            {
                rot.insert(member.id, Rotation::TwoSeventy);
            }
        }
    }
    for placement in &mut layout.components {
        if let Some(&r) = rot.get(&placement.id) {
            placement.rotation = r;
        }
    }
}

/// Rotate every 2-pin part so its power-flagged pin faces the
/// schematic-convention direction: VCC up, GND down.
///
/// Applies to **any** 2-pin part (capacitor, resistor, diode,
/// inductor, crystal, switch) that has at least one power flag.
/// Three cases:
///
/// - Both VCC and GND flags (e.g., decoupling cap): VCC up, GND
///   down. Rotation depends on which pin (0 vs 1) holds the VCC.
/// - VCC only (e.g., a pull-up resistor between a rail and a
///   signal): the VCC pin goes up.
/// - GND only (e.g., an ESD diode's cathode to ground): the GND
///   pin goes down.
///
/// Skipped when the part has no power flag, or when it carries
/// flags on both pins but neither is GND nor VCC (impossible
/// today; defensive `continue`).
fn rotate_two_pin_with_power_flags(board: &Board, layout: &mut Layout) {
    use std::collections::HashMap;
    let mut flags_by_component: HashMap<ComponentId, Vec<&PowerFlag>> = HashMap::new();
    for flag in &layout.power_flags {
        flags_by_component
            .entry(flag.component)
            .or_default()
            .push(flag);
    }
    for placement in &mut layout.components {
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if part.pins.len() != 2 {
            continue;
        }
        let Some(flags) = flags_by_component.get(&placement.id) else {
            continue;
        };
        // Identify which pin (0 or 1) carries VCC and/or GND.
        let pin0_kind = flags.iter().find(|f| f.pin == PinId(0)).map(|f| f.kind);
        let pin1_kind = flags.iter().find(|f| f.pin == PinId(1)).map(|f| f.kind);

        // Determine the desired rotation. At rotation Zero pin 0 is
        // on the left and pin 1 is on the right. A clockwise 90°
        // (`Ninety`) moves pin 0 to the bottom and pin 1 to the top.
        // A counter-clockwise 90° (`TwoSeventy`) moves pin 0 to the
        // top and pin 1 to the bottom.
        let rotation = match (pin0_kind, pin1_kind) {
            // Pin 0 on top (VCC) → use TwoSeventy (CCW).
            (Some(PowerFlagKind::Vcc), Some(PowerFlagKind::Gnd) | None)
            | (None, Some(PowerFlagKind::Gnd)) => Rotation::TwoSeventy,
            // Pin 1 on top (VCC) → use Ninety (CW).
            (Some(PowerFlagKind::Gnd) | None, Some(PowerFlagKind::Vcc))
            | (Some(PowerFlagKind::Gnd), None) => Rotation::Ninety,
            // No power flag, or both pins on the same rail (a
            // 2-endpoint short — undefined orientation): leave at
            // whatever the placer chose.
            _ => continue,
        };
        placement.rotation = rotation;
    }
}

// ----- Cluster recognition -------------------------------------------------

/// A laid-out cluster of related components.
///
/// `anchor` is the component that defines the cluster (an IC,
/// regulator, MCU); `stacked_below` is the list of supporting
/// A recognized sub-circuit: one anchor component plus the
/// components placed around the anchor; each member carries a
/// hint of which side of the anchor it should sit on. Singletons
/// are clusters with no members.
///
/// Public (with public fields) so alternative [`crate::Placer`]
/// implementations (§7.8.5 Stage B) can inspect and reorder the
/// clusters Stage A recognition produced.
#[derive(Debug, Clone)]
pub struct Cluster {
    pub anchor: ComponentId,
    /// Which recognition pass produced this cluster — the motif the
    /// board author would call this sub-circuit.
    pub kind: ClusterKind,
    /// Whether this cluster's anchor itself should be drawn rotated
    /// vertically (e.g., LED indicator with the resistor stacked
    /// vertically above it).
    pub anchor_vertical: bool,
    pub members: Vec<ClusterMember>,
}

/// The motif a [`Cluster`] was recognized as — one variant per
/// `patterns::Pattern` impl.
///
/// Recognition already knows which motif matched; without this the
/// answer was discarded the moment `build_clusters` merged the passes'
/// output. Keeping it lets every consumer name a sub-circuit the way
/// an engineer would ("U1 LDO block") instead of by anchor refdes
/// alone: sheet and group titles, preview tooltips, and diagnostics
/// that today can only say which *net* is at fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterKind {
    LedIndicator,
    UsbEsd,
    LdoBlock,
    I2cBus,
    Crystal,
    IcBlock,
    Divider,
    /// A component no motif claimed — its own one-part cluster.
    Singleton,
}

impl Cluster {
    /// Name for this sub-circuit the way an engineer would write it
    /// on a sheet — `"U1 LDO block"` — falling back to the anchor's
    /// refdes alone for a part no motif claimed.
    pub fn display_name(&self, board: &Board) -> String {
        let refdes = board
            .component(self.anchor)
            .map_or_else(|| format!("#{}", self.anchor.0), |c| c.refdes.clone());
        match self.kind.display_name() {
            Some(motif) => format!("{refdes} {motif}"),
            None => refdes,
        }
    }
}

impl ClusterKind {
    /// Human-readable motif name, for titles and diagnostics.
    ///
    /// `Singleton` has no motif name of its own — a lone part is
    /// named after the part, not after a pattern — so callers that
    /// need a label for one should fall back to its refdes.
    pub fn display_name(self) -> Option<&'static str> {
        match self {
            Self::LedIndicator => Some("LED indicator"),
            Self::UsbEsd => Some("USB ESD protection"),
            Self::LdoBlock => Some("LDO block"),
            Self::I2cBus => Some("I2C bus"),
            Self::Crystal => Some("crystal"),
            Self::IcBlock => Some("IC block"),
            Self::Divider => Some("divider"),
            Self::Singleton => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClusterMember {
    pub id: ComponentId,
    pub side: MemberSide,
}

/// Where to place a cluster member relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberSide {
    /// Members sitting on a "shelf" below the anchor. Default for
    /// decoupling caps.
    Below,
    /// Members sitting in a vertical column above the anchor —
    /// used when the anchor is a vertical part (LED) and the member
    /// (current-limit resistor) feeds into its anode.
    Above,
    /// Members in a vertical column to the right of the anchor.
    /// Used for reset-network passives (the anchor's reset pin is
    /// on the right side of the IC body).
    Right,
    /// Members in a vertical column to the left of the anchor.
    /// Used by USB+ESD: each diode sits in a left-side column,
    /// rotated so its anode pin faces right toward the USB
    /// connector. Wires from connector to diode become short
    /// horizontal stubs instead of long L-shapes.
    Left,
}

/// Find every cluster in the board — Stage A motif recognition
/// (§7.8.3 / §7.8.4).
///
/// Multiple recognition passes, ordered by specificity. Each pass
/// walks `board.components` in declaration order; the first matching
/// component claims its members so passes later in the list never
/// steal already-claimed components.
///
/// Order matters (§7.5.4's greedy matcher, priority order):
///
/// 1. **LED indicators** — anchor on the LED, claim the
///    current-limit resistor on its anode net.
/// 2. **USB+ESD** — anchor on a connector with usb_dp/usb_dn caps,
///    claim ESD diodes on those nets.
/// 3. **LDO block** — anchor on a `regulator`; claim input caps on
///    `vin` and output caps on `vout`.
/// 4. **I2C bus** — anchor on a part with `i2c_sda` + `i2c_scl` pins;
///    claim the SDA/SCL pull-up resistors (tied to a common rail).
/// 5. **Crystal** — anchor on a `crystal`; claim its two load caps.
/// 6. **IC + decoupling + reset network** — anchor on a component
///    that has `required_decoupling` and/or a `reset` capability
///    pin. Claim caps on the decoupling nets (capped per
///    `required_decoupling.count`) plus any resistor/cap/switch
///    on the reset pin's net.
/// 7. **Divider** — anchor on the rail-side resistor of a
///    two-resistor rail→mid→gnd divider; claim the mid-to-gnd
///    resistor.
/// 8. **Singletons** — everything else.
pub fn build_clusters(board: &Board) -> Vec<Cluster> {
    use patterns::Pattern as _;
    let mut claimed: HashSet<ComponentId> = HashSet::new();
    let mut clusters = Vec::new();
    clusters.extend(patterns::led_indicator::LedIndicator::recognize(
        board,
        &mut claimed,
    ));
    clusters.extend(patterns::usb_esd::UsbEsd::recognize(board, &mut claimed));
    clusters.extend(patterns::ldo_block::LdoBlock::recognize(
        board,
        &mut claimed,
    ));
    clusters.extend(patterns::crystal::Crystal::recognize(board, &mut claimed));
    // IcBlock runs *before* I2cBus: an I2C host is usually a full IC
    // that must claim its own decoupling caps and reset network. If
    // I2cBus ran first it would `claim` the MCU and starve IcBlock of
    // the anchor, dropping the MCU's decoupling/reset from the sheet.
    clusters.extend(patterns::ic_block::IcBlock::recognize(board, &mut claimed));
    clusters.extend(patterns::i2c_bus::I2cBus::recognize(board, &mut claimed));
    clusters.extend(patterns::divider::Divider::recognize(board, &mut claimed));
    // Second sweep before `Singleton` mops up (schematic-quality plan
    // Phase A2): a decoupling cap on a *shared* rail is reachable
    // from no single anchor's `required_decoupling`, so it used to
    // fall through to `Singleton` and get placed by power-flow layer
    // — 150+ mm from the part it decouples. Running here, after every
    // structural pass, lets it attach to whichever cluster actually
    // draws from its rail, `LdoBlock` and `Crystal` included.
    patterns::ic_block::attach_orphan_rail_caps(board, &mut claimed, &mut clusters);
    evict_cross_group_members(board, &mut clusters);
    clusters.extend(patterns::singleton::Singleton::recognize(
        board,
        &mut claimed,
    ));
    clusters
}

/// A declared `group` bounds cluster membership: drop any member whose
/// group differs from its anchor's, leaving it to `Singleton`.
///
/// The pattern passes match on topology alone, so an I²C pull-up
/// declared inside the sensor's group can be claimed by the MCU's
/// `IcBlock` two groups away. Placement then puts it in the *anchor's*
/// region while `group_bounds` still measures it as part of its own —
/// stretching that group's box across the whole sheet, overlapping
/// every other box (`E-SYNTH-SCHEM-013`) and pushing the page from A4
/// to A2. Ungrouped boards are unaffected: every component's group is
/// `None`, so nothing is ever evicted.
fn evict_cross_group_members(board: &Board, clusters: &mut Vec<Cluster>) {
    let group_of =
        |id: ComponentId| -> Option<String> { board.component(id).and_then(|c| c.group.clone()) };
    let mut evicted: Vec<ComponentId> = Vec::new();
    for cluster in clusters.iter_mut() {
        let anchor_group = group_of(cluster.anchor);
        cluster.members.retain(|m| {
            if group_of(m.id) == anchor_group {
                true
            } else {
                evicted.push(m.id);
                false
            }
        });
    }
    // Evicted members become their own single-component clusters, in
    // id order so the result stays deterministic.
    evicted.sort_by_key(|id| id.0);
    evicted.dedup();
    for id in evicted {
        clusters.push(Cluster {
            kind: ClusterKind::Singleton,
            anchor: id,
            anchor_vertical: false,
            members: Vec::new(),
        });
    }
}

/// Largest body width and height across every part in the board.
///
/// Queries the KiCad symbol library for parts that have a
/// `kicad_symbol` mapping (so the cluster grid leaves enough room
/// for real rendered footprints). For parts without a mapping, falls back to the synthesized
/// rectangle's nominal size.
///
/// Returns `(0.0, 0.0)` when the board has no parts. Callers
/// combine the result with `BASE_CLUSTER_DX/DY` via `max(...)` so
/// designs that only use small bodies keep the tight default
/// spacing.
fn max_body_extent(board: &Board) -> (f64, f64) {
    let mut max_w: f64 = 0.0;
    let mut max_h: f64 = 0.0;
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let (w, h) = body_size_for_part(part);
        max_w = max_w.max(w);
        max_h = max_h.max(h);
    }
    (max_w, max_h)
}

/// Body width and height for a single part.
///
/// Prefers KiCad's actual rendered geometry when the part has a
/// `kicad_symbol` mapping that loads cleanly. Otherwise falls back
/// to a synthesis matching the rectangle we emit in
/// `synth-kicad::symbol_lib::build_symbol`.
pub fn body_size_for_part(part: &synth_registry::Part) -> (f64, f64) {
    if let Some(lib_id) = part.kicad_symbol.as_deref() {
        if let Some(bbox) = kicad_lib_loader::body_bbox(lib_id) {
            // A multi-unit symbol is drawn as a stack anchored at one
            // placement, so its reserved box is taller than one unit.
            let units = kicad_lib_loader::symbol_units(lib_id).map_or(1, |(_, count)| count);
            let h = bbox.1 + f64::from(units.saturating_sub(1)) * kicad_lib_loader::UNIT_PITCH_MM;
            return (bbox.0, h);
        }
    }
    // Fallback synthesis (mirrors synth-kicad's rectangle sizing).
    let pin_count = part.pins.len();
    if pin_count == 2 && is_two_pin_symbol_kind(&part.kind) {
        return (7.62, 4.0);
    }
    let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin_layout).collect();
    let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
    let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
    let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
    let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();
    let horiz_max = top_n.max(bottom_n).max(2);
    let vert_max = left_n.max(right_n).max(2);
    let w = ((horiz_max as f64) * 2.54 + 2.0 * 2.54).max(15.24);
    let h = ((vert_max as f64) * 2.54 + 2.0 * 2.54).max(10.16);
    (w, h)
}

/// PCB footprint courtyard size for a part, in millimetres.
///
/// Distinct from [`body_size_for_part`], which returns the
/// *schematic-symbol* bbox. The PCB placer needs the *footprint
/// courtyard* — the physical keep-out rectangle the IPC standard
/// requires around the component body. ATmega328P-P DIP-28's
/// schematic symbol is ~25 × 70 mm; its DIP-28 footprint
/// courtyard is ~10 × 36 mm. Mixing the two on the PCB side
/// produces wildly oversized board outlines and impossible
/// constraint problems.
///
/// Looks up the registry's `kicad_footprint` and queries
/// [`kicad_footprint_loader::courtyard_bbox`]. Falls back to a
/// conservative 10 × 10 mm rectangle when the footprint isn't
/// mapped or KiCad isn't installed.
pub fn pcb_courtyard_for_part(part: &synth_registry::Part) -> (f64, f64) {
    let (_offset, size) = pcb_courtyard_geometry_for_part(part);
    size
}

/// PCB footprint courtyard offset and size for a part, in millimetres.
/// Returns `((center_offset_x_mm, center_offset_y_mm), (width_mm, height_mm))`.
pub fn pcb_courtyard_geometry_for_part(part: &synth_registry::Part) -> ((f64, f64), (f64, f64)) {
    if let Some(lib_id) = part.kicad_footprint.as_deref() {
        if let Some((cx, cy, w, h)) = kicad_footprint_loader::courtyard_rect(lib_id) {
            return ((cx, cy), (w, h));
        }
    }
    if let Some(dim) = part.footprint_dimensions.as_ref() {
        return ((0.0, 0.0), (dim.width_mm, dim.height_mm));
    }
    ((0.0, 0.0), (10.0, 10.0))
}

/// LED detection: kind = "led" OR part id starts with "led_". The
/// registry is inconsistent about which it uses; recognise both.
fn is_led(component: &synth_ir::Component) -> bool {
    component
        .part
        .as_ref()
        .is_some_and(|p| p.kind == "led" || p.id.as_str().starts_with("led_"))
}

// ----- Cluster placement ---------------------------------------------------

/// Build a map from every component that belongs to a cluster (its
/// anchor or any member) to that cluster's index in the `clusters`
/// slice it was built from. Used by [`build_cluster_adjacency`] to
/// resolve net endpoints back to the cluster that owns them.
fn cluster_index_by_component(
    clusters: &[Cluster],
) -> std::collections::HashMap<ComponentId, usize> {
    let mut map = std::collections::HashMap::new();
    for (idx, cluster) in clusters.iter().enumerate() {
        map.insert(cluster.anchor, idx);
        for member in &cluster.members {
            map.insert(member.id, idx);
        }
    }
    map
}

/// Strong-vs-weak inter-cluster edge weights (§7.8.1 finding #1,
/// matchpack-style semantic placement).
///
/// A **strong** connection is a functional signal that locally ties
/// two clusters together (a decoupling cap to its IC, a crystal load
/// cap to the crystal, a resistor feeding an IC pin, an LED's series
/// resistor). A **weak** connection is a power/ground rail that many
/// components share but that never decides which component sits next
/// to which — it only says which way is "down".
///
/// `STRONG_NET_WEIGHT` is an order of magnitude above
/// `WEAK_NET_WEIGHT`, so a single strong edge outweighs many weak
/// rails in the barycenter ordering: strong edges dominate proximity
/// (pull clusters into the same/nearby columns), weak edges contribute
/// only a small orientation nudge and never drag unrelated clusters
/// together.
const STRONG_NET_WEIGHT: u32 = 16;
/// See [`STRONG_NET_WEIGHT`].
const WEAK_NET_WEIGHT: u32 = 1;

/// Whether a shared net is a *weak*, orientation-only connection
/// between clusters (a power or ground rail) rather than a *strong*,
/// proximity-determining functional signal (§7.8.1 finding #1).
///
/// A net is weak when any of its endpoints lands on a pin whose
/// electrical type is `PowerInput`/`PowerOutput`, or whose name marks
/// a ground rail (`gnd`/`vss`/`vssa`/`gnda`/`vee`/`agnd`/`dgnd`).
/// This mirrors the power-rail classification in
/// [`classify_power_flags`]: VCC/GND connect many components without
/// pulling them together. Every other net — a resistor feeding an IC
/// pin, a crystal's load caps, an LED's series resistor — is strong.
///
/// Components without a resolved part (`board.pin` returns `None`)
/// carry no power pins, so their nets default to strong — the
/// conservative choice for unrecognised components.
fn net_is_weak(board: &Board, net: &synth_ir::Net) -> bool {
    use synth_registry::ElectricalType;
    for endpoint in &net.endpoints {
        let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
            continue;
        };
        if matches!(
            pin.electrical_type,
            ElectricalType::PowerInput | ElectricalType::PowerOutput
        ) {
            return true;
        }
        let lower = pin.name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "gnd" | "vss" | "vssa" | "gnda" | "vee" | "vneg" | "agnd" | "dgnd"
        ) {
            return true;
        }
    }
    false
}

/// Inter-cluster adjacency for barycenter ordering (§7.5.5 step 2).
///
/// Two clusters are adjacent when any component belonging to one
/// (its anchor or any member) shares a net with any component
/// belonging to the other. The weight is the sum of each distinct
/// shared net's semantic connection strength — [`STRONG_NET_WEIGHT`]
/// for functional signals, [`WEAK_NET_WEIGHT`] for power/ground rails
/// (§7.8.1 finding #1). Strong edges therefore dominate the barycenter
/// (and thus proximity), while weak edges only nudge orientation and
/// never pull unrelated clusters together.
///
/// Returns one `HashMap` per cluster, indexed the same as
/// `clusters`, mapping neighbour cluster index -> strength-weighted
/// edge sum.
fn build_cluster_adjacency(
    board: &Board,
    clusters: &[Cluster],
) -> Vec<std::collections::HashMap<usize, u32>> {
    let owner = cluster_index_by_component(clusters);
    let mut adjacency: Vec<std::collections::HashMap<usize, u32>> =
        vec![std::collections::HashMap::new(); clusters.len()];
    for net in &board.nets {
        let mut touched: Vec<usize> = net
            .endpoints
            .iter()
            .filter_map(|ep| owner.get(&ep.component).copied())
            .collect();
        touched.sort_unstable();
        touched.dedup();
        let weight = if net_is_weak(board, net) {
            WEAK_NET_WEIGHT
        } else {
            STRONG_NET_WEIGHT
        };
        for i in 0..touched.len() {
            for j in (i + 1)..touched.len() {
                let (a, b) = (touched[i], touched[j]);
                *adjacency[a].entry(b).or_insert(0) += weight;
                *adjacency[b].entry(a).or_insert(0) += weight;
            }
        }
    }
    adjacency
}

/// Reorder `rows[target]` by the weighted mean column position (the
/// "barycenter") of each cluster's neighbours in `rows[reference]`
/// — one Sugiyama-style crossing-reduction pass (§7.5.5 step 2).
/// Clusters with no neighbour in the reference row sort after every
/// cluster that has one, falling back to `fallback_key` ((group, layer,
/// anchor id) — declaration order within the layer) for a stable,
/// non-random tiebreak.
///
/// Returns whether the row's order actually changed, so the caller
/// can stop sweeping once a full pass is a no-op.
fn reorder_row_by_barycenter(
    rows: &mut [Vec<usize>],
    target: usize,
    reference: usize,
    adjacency: &[std::collections::HashMap<usize, u32>],
    fallback_key: &[(u32, u32, u32)],
) -> bool {
    let ref_positions: std::collections::HashMap<usize, usize> = rows[reference]
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    let mut scored: Vec<(usize, Option<f64>)> = rows[target]
        .iter()
        .map(|&cluster_idx| {
            let mut weighted_sum = 0.0_f64;
            let mut weight_total = 0.0_f64;
            for (&neighbour, &weight) in &adjacency[cluster_idx] {
                if let Some(&pos) = ref_positions.get(&neighbour) {
                    weighted_sum += pos as f64 * weight as f64;
                    weight_total += weight as f64;
                }
            }
            let barycenter = (weight_total > 0.0).then_some(weighted_sum / weight_total);
            (cluster_idx, barycenter)
        })
        .collect();

    let before: Vec<usize> = scored.iter().map(|&(idx, _)| idx).collect();
    scored.sort_by(|&(a_idx, a_bc), &(b_idx, b_bc)| match (a_bc, b_bc) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => fallback_key[a_idx].cmp(&fallback_key[b_idx]),
    });
    let after: Vec<usize> = scored.iter().map(|&(idx, _)| idx).collect();

    let changed = before != after;
    rows[target] = after;
    changed
}

/// Run barycenter crossing-reduction sweeps over `rows` (§7.5.5 step
/// 2) until stable, capped at 4 sweeps (2 downward + 2 upward) —
/// this codebase never has more than 4 layers, so the full "≤8
/// sweeps" budget from §7.5.5 would be overkill. The first row has
/// no row above it, so it only moves on the first *upward* sweep
/// (relative to the row below); ditto symmetrically for the last
/// row on the first downward sweep.
/// One `(cluster-key slice, order slice)` banding candidate fed to
/// the sheet-fitting search: grouped-by-caption and flat.
type Bandings<'a> = (&'a [(u32, u32, u32)], &'a [usize]);

fn barycenter_order_rows(
    rows: &mut [Vec<usize>],
    adjacency: &[std::collections::HashMap<usize, u32>],
    fallback_key: &[(u32, u32, u32)],
) {
    const MAX_SWEEPS: usize = 4;
    if rows.len() <= 1 {
        return;
    }
    for sweep in 0..MAX_SWEEPS {
        let mut changed = false;
        if sweep % 2 == 0 {
            // Downward: order row r by (already-placed) row r-1.
            for r in 1..rows.len() {
                changed |= reorder_row_by_barycenter(rows, r, r - 1, adjacency, fallback_key);
            }
        } else {
            // Upward: order row r by row r+1.
            for r in (0..rows.len() - 1).rev() {
                changed |= reorder_row_by_barycenter(rows, r, r + 1, adjacency, fallback_key);
            }
        }
        if !changed {
            break;
        }
    }
}

// ----- Brandes–Köpf coordinate assignment --------------------------------

/// One directional sweep of the Brandes–Köpf vertical-alignment phase
/// (§7.5.5 step 3 / §7.7.4 step 3). The algorithm runs a sweep for
/// each combination of a column-scan direction (Up vs Down) and a
/// within-column median tie-break (Left vs Right), then balances the
/// left- and right-weighted results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BkAlignment {
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

/// Minimum vertical separation between two clusters in the same column,
/// expressed in Brandes–Köpf grid units. Multiplied by `grid_h` (mm)
/// before the 2.54 mm snap in [`place_clusters`].
const BK_UNIT: f64 = 1.0;

/// Brandes–Köpf coordinate assignment for a layered cluster graph
/// (§7.5.5 step 3 / §7.7.4 step 3).
///
/// `layers` are the columns, each an ordered list of cluster indices
/// (barycenter order, top→bottom). `adjacency` maps each cluster to
/// `{neighbour cluster -> shared-net count}` (from
/// [`build_cluster_adjacency`]). Returns a per-cluster *unit* y-offset
/// (0.0 = the topmost node); callers scale by `grid_h` and snap to the
/// 2.54 mm grid.
///
/// The layered graph is built from adjacent columns only — a cluster's
/// neighbours in the immediately-left/right column — mirroring the
/// adjacent-column model the barycenter sweep already uses. A net that
/// skips a column is projected onto the columns it actually links; it
/// never aligns a pair of nodes across a skipped column.
///
/// Runs the four vertical-alignment + horizontal-compaction sweeps of
/// Brandes & Köpf (2002, "Fast and simple horizontal coordinate
/// assignment"), averaging the two left-alignments and the two
/// right-alignments, then taking the mid-point — the paper's balancing
/// step. The compaction uses the paper's `sink`/`shift` block-merging
/// trick, so the whole assignment is O(V + E). Each sweep enforces that
/// a node's coordinate is ≥ its aligned predecessor's + [`BK_UNIT`];
/// a final per-column clamp guarantees the barycenter order (and hence
/// distinct, non-inverted positions) even when a column has no
/// cross-column edges to constrain it.
#[allow(clippy::manual_midpoint)]
fn bk_y_coordinates(
    layers: &[Vec<usize>],
    adjacency: &[std::collections::HashMap<usize, u32>],
) -> Vec<f64> {
    let n: usize = layers.iter().map(Vec::len).sum();
    if n == 0 {
        return Vec::new();
    }

    let mut layer_of = vec![0usize; n];
    let mut pos_in_layer = vec![0usize; n];
    for (l, layer) in layers.iter().enumerate() {
        for (p, &c) in layer.iter().enumerate() {
            layer_of[c] = l;
            pos_in_layer[c] = p;
        }
    }

    // Neighbours restricted to the immediately-previous / next column,
    // each ordered by its position in that column (deterministic
    // median).
    let mut upper = vec![Vec::<usize>::new(); n];
    let mut lower = vec![Vec::<usize>::new(); n];
    for v in 0..n {
        let l = layer_of[v];
        for &w in adjacency[v].keys() {
            if layer_of[w] + 1 == l {
                upper[v].push(w);
            } else if layer_of[w] == l + 1 {
                lower[v].push(w);
            }
        }
        upper[v].sort_by_key(|&w| pos_in_layer[w]);
        lower[v].sort_by_key(|&w| pos_in_layer[w]);
    }

    let up_l = align_and_compact(layers, &upper, &lower, BkAlignment::UpLeft);
    let up_r = align_and_compact(layers, &upper, &lower, BkAlignment::UpRight);
    let dn_l = align_and_compact(layers, &upper, &lower, BkAlignment::DownLeft);
    let dn_r = align_and_compact(layers, &upper, &lower, BkAlignment::DownRight);

    // Balance (§7.7.4 step 3): average the two left-sweeps and the two
    // right-sweeps, then take the mid-point so a node sits at the
    // centre of its legal interval.
    let mut y = vec![0.0_f64; n];
    for v in 0..n {
        let left = (up_l[v] + dn_l[v]) / 2.0;
        let right = (up_r[v] + dn_r[v]) / 2.0;
        y[v] = (left + right) / 2.0;
    }

    // Safety clamp: enforce the barycenter order within each column so
    // no two clusters collide or invert, regardless of how sparse the
    // cross-column edges are. Straight vertical alignment of aligned
    // nodes (across columns) is preserved because this only nudges
    // nodes downward within their own column.
    let mut prev = vec![f64::NEG_INFINITY; layers.len()];
    for (l, layer) in layers.iter().enumerate() {
        for &c in layer {
            let want = prev[l] + BK_UNIT;
            if y[c] < want {
                y[c] = want;
            }
            prev[l] = y[c];
        }
    }
    y
}

/// Run the vertical-alignment sweep for `dir`, then the matching
/// horizontal compaction, and return the resulting unit coordinates.
fn align_and_compact(
    layers: &[Vec<usize>],
    upper: &[Vec<usize>],
    lower: &[Vec<usize>],
    dir: BkAlignment,
) -> Vec<f64> {
    let (_align, root) = vertical_alignment(layers, upper, lower, dir);
    horizontal_compaction(layers, upper, lower, &root, dir)
}

/// Vertical-alignment phase (Algorithm 1 of Brandes & Köpf 2002).
///
/// Returns `(align, root)`: `align[v]` is the neighbour `v` is aligned
/// with (a vertical block), and `root[v]` is the root of `v`'s block.
/// All nodes in a block share a y-coordinate (straight vertical
/// alignment of aligned nodes).
fn vertical_alignment(
    layers: &[Vec<usize>],
    upper: &[Vec<usize>],
    lower: &[Vec<usize>],
    dir: BkAlignment,
) -> (Vec<usize>, Vec<usize>) {
    let n: usize = layers.iter().map(Vec::len).sum();
    let mut align: Vec<usize> = (0..n).collect();
    let mut root: Vec<usize> = (0..n).collect();
    let mut visited = vec![false; n];

    // The "previous" column relative to the sweep direction, and the
    // order in which to scan within a column.
    let layer_order: Vec<usize> = match dir {
        BkAlignment::UpLeft | BkAlignment::UpRight => (0..layers.len()).collect(),
        BkAlignment::DownLeft | BkAlignment::DownRight => (0..layers.len()).rev().collect(),
    };
    let neighbors: &[Vec<usize>] = match dir {
        BkAlignment::UpLeft | BkAlignment::UpRight => upper,
        BkAlignment::DownLeft | BkAlignment::DownRight => lower,
    };

    for &l in &layer_order {
        let layer: &[usize] = &layers[l];
        let node_order: Vec<usize> = match dir {
            BkAlignment::UpLeft | BkAlignment::DownLeft => layer.to_vec(),
            BkAlignment::UpRight | BkAlignment::DownRight => layer.iter().rev().copied().collect(),
        };
        for v in node_order {
            let nbrs = &neighbors[v];
            if nbrs.is_empty() {
                continue;
            }
            // Align with the median neighbour (upper median for Right
            // sweeps, lower for Left — the paper's tie-break).
            let w = match dir {
                BkAlignment::UpLeft | BkAlignment::DownLeft => nbrs[(nbrs.len() - 1) / 2],
                BkAlignment::UpRight | BkAlignment::DownRight => nbrs[nbrs.len() / 2],
            };
            if !visited[w] {
                visited[w] = true;
                root[v] = root[w];
                align[v] = w;
            }
        }
    }
    (align, root)
}

/// Horizontal-compaction phase (Algorithms 2 & 3 of Brandes & Köpf
/// 2002). Computes a unit coordinate per node such that consecutive
/// nodes in the sweep order keep ≥ [`BK_UNIT`] separation, using the
/// `sink`/`shift` block-merging trick for O(V + E) running time.
fn horizontal_compaction(
    layers: &[Vec<usize>],
    upper: &[Vec<usize>],
    lower: &[Vec<usize>],
    root: &[usize],
    dir: BkAlignment,
) -> Vec<f64> {
    fn place_block(
        v: usize,
        pred: &[Vec<usize>],
        x: &mut [f64],
        sink: &mut [usize],
        shift: &mut [f64],
    ) {
        if x[v].is_finite() {
            return;
        }
        x[v] = 0.0;
        for &w in &pred[v] {
            place_block(w, pred, x, sink, shift);
            if sink[v] == v {
                sink[v] = sink[w];
            } else if sink[v] != sink[w] {
                let cand = shift[sink[w]] + x[w] + BK_UNIT - x[v];
                if cand > shift[sink[v]] {
                    shift[sink[v]] = cand;
                }
            } else if x[w] + BK_UNIT > x[v] {
                x[v] = x[w] + BK_UNIT;
            }
        }
    }

    let n: usize = layers.iter().map(Vec::len).sum();
    let pred: &[Vec<usize>] = match dir {
        BkAlignment::UpLeft | BkAlignment::UpRight => upper,
        BkAlignment::DownLeft | BkAlignment::DownRight => lower,
    };

    let mut x = vec![f64::NAN; n];
    let mut sink: Vec<usize> = (0..n).collect();
    let mut shift = vec![0.0_f64; n];

    for (v, &r) in root.iter().enumerate() {
        if v == r {
            place_block(v, pred, &mut x, &mut sink, &mut shift);
        }
    }

    let mut out = vec![0.0_f64; n];
    for v in 0..n {
        out[v] = x[root[v]] + shift[sink[root[v]]];
    }
    out
}

/// Lay out clusters on the sheet.
///
/// **Algorithm** (§7.5.5 "Cluster placement", scoped down from the
/// full Sugiyama pipeline as described in step 3 below; transposed
/// to a left-to-right layout per the 2026-08-17 team review — see
/// the "Layered left-to-right" comment inside this function for why):
///
/// 1. **Layer assignment** (§7.5.5 step 1): each cluster's anchor is
///    assigned a layer via [`layer_for`] (0 = board edges/power
///    sources … 3 = passives/everything else). Layers become real
///    columns here — clusters from different `layer_for` values
///    never share a column. A layer with more clusters than fit in
///    one column wraps into multiple sub-columns, all still within
///    that one layer.
/// 2. **Crossing reduction** (§7.5.5 step 2): within each column,
///    clusters are ordered by the barycenter heuristic — the mean
///    row position of neighbouring clusters (sharing a net, via
///    [`build_cluster_adjacency`]) in the column immediately
///    left/right — via [`barycenter_order_rows`] (the function name
///    predates the transpose; it orders "groups by adjacency to a
///    neighbouring group" regardless of which axis a group
///    represents). Clusters with no such neighbour keep (layer,
///    anchor id) declaration order as a deterministic fallback.
/// 3. **Coordinate assignment** (§7.5.5 step 3 / §7.7.4 step 3):
///    Brandes–Köpf layered coordinate assignment via
///    [`bk_y_coordinates`] positions each column's barycenter-ordered
///    clusters with straight vertical alignment of aligned (connected)
///    nodes. The per-cluster unit offsets are scaled by `grid_h`,
///    vertically centred on the sheet, and snapped to the 2.54 mm
///    grid (see the "Brandes–Köpf coordinate assignment" comment
///    inside this function).
///
/// Within each cluster, the anchor sits at its grid cell's centre
/// and members fan out into a single horizontal row beneath the
/// anchor — so a cluster with 4 caps looks like an IC sitting on top
/// of a 4-cap "shelf", not a 5-component column.
///
/// **Region packing (Phase C1).** A board that declares `group`s does
/// not lay out as one long horizontal strip. Instead each group's
/// columns are laid out independently in a local frame, their true
/// content rectangles measured, and the rectangles shelf-packed onto
/// the sheet (ordered by declaration, which follows power/signal
/// flow). A group's parts therefore stay inside one rectangle, the
/// drawn boxes never overlap, and the page reads as the reference
/// sheet's grid of titled regions. A board with no groups (or all
/// components in one group) takes the historic single-region path
/// unchanged, so ungrouped output is byte-identical.
fn place_clusters(board: &Board, clusters: &[Cluster]) -> Layout {
    // Fallback order — also the initial, pre-barycenter row order:
    // by declared sub-circuit first, then power-flow layer (sources
    // higher up), then anchor id for determinism within a layer.
    //
    // A declared `group` outranks `layer_for` because it is the
    // author's own statement about what belongs together, and because
    // the sheet captions drawn by `annotate_groups` are only honest if
    // a group's parts are actually contiguous: a caption sits above
    // its group's bounding box, so a group scattered across the sheet
    // would title a region full of other groups' parts. Boards that
    // declare no groups get one band and the order they always had.
    let mut group_bands: Vec<&str> = Vec::new();
    for component in &board.components {
        if let Some(group) = effective_group(board, component.id) {
            if !group_bands.contains(&group) {
                group_bands.push(group);
            }
        }
    }
    // Ungrouped clusters trail the named ones rather than interleaving:
    // they have no caption, so they cannot break one.
    let band_of = |anchor: ComponentId| -> u32 {
        effective_group(board, anchor)
            .and_then(|g| group_bands.iter().position(|b| *b == g))
            .map_or(u32::MAX, |idx| idx as u32)
    };
    let cluster_layer: Vec<u32> = clusters
        .iter()
        .map(|c| board.component(c.anchor).map_or(3, layer_for))
        .collect();
    let grouped_key: Vec<(u32, u32, u32)> = clusters
        .iter()
        .enumerate()
        .map(|(idx, c)| (band_of(c.anchor), cluster_layer[idx], c.anchor.0))
        .collect();
    // The same ordering with every cluster in one band: what a board
    // that declares no groups gets, and the fallback for a grouped
    // board whose banded placement will not fit any page (see the
    // fitting loop below).
    let flat_key: Vec<(u32, u32, u32)> = clusters
        .iter()
        .enumerate()
        .map(|(idx, c)| (0, cluster_layer[idx], c.anchor.0))
        .collect();
    let order_for = |key: &[(u32, u32, u32)]| -> Vec<usize> {
        let mut order: Vec<usize> = (0..clusters.len()).collect();
        order.sort_by_key(|&idx| key[idx]);
        order
    };
    let grouped_order = order_for(&grouped_key);
    let flat_order = order_for(&flat_key);

    let cluster_count = clusters.len().max(1);

    // Scale the cluster grid to the largest body that will land in
    // it. KiCad's stock symbols (ATmega328P-P DIP-28 → ~25×40 mm,
    // USB-C receptacle → ~15×50 mm, AMS1117 → ~15×12 mm) are
    // significantly larger than the synthesized rectangles we used
    // before. Without this scaling, clusters collide when stock
    // symbols replace fallbacks.
    let (max_body_w, max_body_h) = max_body_extent(board);
    let grid_w = snap_grid(BASE_CLUSTER_DX.max(max_body_w + WIRE_MARGIN));
    let grid_h = snap_grid(BASE_CLUSTER_DY.max(max_body_h + WIRE_MARGIN));

    // Layered left-to-right, not top-to-bottom (team review,
    // 2026-08-17): power/signal flow reads left-to-right on a real
    // schematic — power input at the left edge, the main IC toward
    // the right, everything roughly vertically centred — not power
    // at the top of a tall stack. Layers (§7.5.5 step 1) become
    // COLUMNS instead of rows; barycenter ordering (§7.5.5 step 2)
    // orders each column's clusters top-to-bottom instead of a
    // row's left-to-right. This is also a better fit for the fixed
    // A4-landscape sheet (§7.8's "use A4 layout" decision): a wide,
    // short content box uses the page better than a tall, narrow one.
    //
    // This doubles as the column-wrap height within a single layer
    // (see `col_groups` below): a layer with more clusters than
    // `rows` wraps into multiple sub-columns.
    //
    // Sizing this is the mirror image of the pre-transpose row-based
    // algorithm's `cols` (see git history): that picked `cols ≈
    // sqrt(n · aspect · grid_h/grid_w)` so a flat pile of `n`
    // clusters packs into a roughly `TARGET_ASPECT`-shaped grid.
    // Naively porting that (just swapping which axis `n`/aspect
    // apply to) undershoots badly: `grid_h` is typically much bigger
    // than `grid_w` (bodies are usually taller than wide), so
    // dividing by it instead of multiplying — as the direct row→
    // column swap would — makes `rows` collapse to ~1-2, forcing
    // every populous layer (a common case: "every unclaimed
    // passive" singleton layer) to wrap into many extra columns and
    // spread the sheet far wider than A4. The correct mirror keeps
    // the same total-area balance the original formula solved for,
    // just solved for `rows` instead of `cols`:
    //   total_width  ≈ (n / rows) · grid_w   (n items, `rows`-tall columns)
    //   total_height ≈ rows · grid_h
    //   want total_width ≈ TARGET_ASPECT · total_height
    //   ⇒ rows ≈ sqrt(n · grid_w / (TARGET_ASPECT · grid_h))
    // A hard floor of `1` still stops a tiny board (n=1) from
    // dividing to zero.
    let rows_raw = ((cluster_count as f64) * grid_w / (TARGET_ASPECT * grid_h)).sqrt();
    let rows_aspect = (rows_raw.ceil() as usize).clamp(1, cluster_count);

    // Origin needs to leave room for top/left members of the very
    // first cluster column (anchors sit at the origin).
    let origin_x = snap_grid(GRID_ORIGIN_X.max(max_body_w / 2.0 + MEMBER_CLEARANCE + PAGE_MARGIN));
    let origin_y = snap_grid(GRID_ORIGIN_Y.max(max_body_h / 2.0 + MEMBER_CLEARANCE + PAGE_MARGIN));

    // How many cluster rows fit on a given sheet without running into
    // KiCad's stock title block. KiCad draws that block itself — we
    // don't emit a custom `title_block`/`drawing_sheet`, so the stock
    // one always applies — as a fixed 108×32mm rectangle anchored to
    // the page's bottom-right corner
    // (`drawing_sheet_default_description.cpp`: `(rect (start 110 34)
    // (end 2 2))`, coordinates relative to `rbcorner`); on A4's 210mm
    // height that is y ∈ [176, 208] regardless of how far our content
    // actually extends. A tall column stack can walk straight into it
    // (observed: U2/C3 in `sensor_logger.synth` landing at y≈180-212),
    // so each placement attempt caps its column height to keep content
    // above that band on the sheet being fitted — TEXT_MARGIN_Y gives
    // Reference/Value label overhang some room too.
    let max_rows_for_sheet = |sheet: SheetSize| -> usize {
        let (_, sheet_h) = sheet.dims_mm();
        let safe_max_y = sheet_h - TITLE_BLOCK_H - TEXT_MARGIN_Y;
        (((safe_max_y - origin_y) / grid_h).floor() as usize).max(1)
    };

    // Cluster adjacency depends only on the board and the cluster set,
    // never on how many rows a placement attempt uses — build it once
    // and share it across the attempts below.
    let adjacency = build_cluster_adjacency(board, clusters);

    // One placement attempt at a given column height, returning the
    // placements plus their content bounding box (body extents, text
    // excluded — the same box `sheet_size_for` consumes). Kept as a
    // closure so the fitting loop below can retry with a taller column
    // when the content runs off the page.
    let place_for_rows = |rows: usize,
                          shelves_hint: usize,
                          pitch_floor: f64,
                          region_target_w: f64,
                          fallback_key: &[(u32, u32, u32)],
                          fallback_order: &[usize]|
     -> (Vec<ComponentPlacement>, (f64, f64, f64, f64)) {
        // Group into real layer columns (§7.5.5 step 1): walk
        // `fallback_order` (already sorted by group, then layer) and
        // cut a new column group at every boundary in EITHER, then wrap
        // any group taller than `rows` into multiple sub-columns.
        // Two different `layer_for` values never land in the same
        // column group — the left-to-right analogue of the fix §7.5.5
        // describes ("layering is a sort key, not a row [here: column]
        // boundary") — and neither do two different declared groups,
        // which is what keeps a sub-circuit contiguous under its caption.
        let mut col_groups: Vec<Vec<usize>> = Vec::new();
        let mut i = 0;
        while i < fallback_order.len() {
            let (band, layer, _) = fallback_key[fallback_order[i]];
            let mut j = i;
            while j < fallback_order.len() && {
                let (other_band, other_layer, _) = fallback_key[fallback_order[j]];
                (other_band, other_layer) == (band, layer)
            } {
                j += 1;
            }
            for chunk in fallback_order[i..j].chunks(rows) {
                col_groups.push(chunk.to_vec());
            }
            i = j;
        }

        // Order within each column by barycenter (§7.5.5 step 2), then
        // assign sequential rows per column (§7.5.5 step 3, simplified —
        // see the doc comment above). `barycenter_order_rows` is generic
        // on "groups ordered by adjacency to the neighbouring group" —
        // it doesn't care whether a group is a row or a column.
        barycenter_order_rows(&mut col_groups, &adjacency, fallback_key);

        // Per-column max text-inclusive half-width. `grid_w` is sized to
        // the widest body on the *whole board*; charging every column
        // that spacing regardless of what it actually contains wastes
        // horizontal space once layers are real columns (§7.5.5 step 1)
        // — a board with one large MCU and several columns of small
        // passives was landing on A3/A2 instead of A4 even though only
        // the MCU's own column needs the large pitch.
        let col_max_half_w: Vec<f64> = col_groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|&cluster_idx| {
                        component_text_inclusive_half_width(board, clusters[cluster_idx].anchor)
                    })
                    .fold(0.0_f64, f64::max)
            })
            .collect();
        // Gap BETWEEN consecutive columns: must clear column i's own
        // rightward extent (its anchor's half-width, symmetric around
        // its centre) *and* column i+1's anchor extending back leftward
        // toward it — a narrow column (small anchor) immediately before
        // a column with an unusually wide anchor (e.g. a USB connector)
        // needs a bigger gap than either column's own width alone would
        // suggest, or the wide anchor's Reference/Value text reaches
        // left into the column before it.
        // Which declared group (or the implicit trailing region,
        // `u32::MAX`) each column belongs to.
        let col_band: Vec<u32> = col_groups
            .iter()
            .map(|group| group.first().map_or(u32::MAX, |&idx| fallback_key[idx].0))
            .collect();

        // Brandes–Köpf y coordinates across the whole cluster set. Each
        // region below takes its own slice relative to its own minimum,
        // so regions keep their internal vertical ordering without
        // inheriting a global offset.
        let bk_units = bk_y_coordinates(&col_groups, &adjacency);

        // Contiguous runs of columns sharing one band = one placement
        // region (Phase C1). `place_for_rows` already refuses to put two
        // bands in one column, so a run is exactly one group's columns.
        let mut region_runs: Vec<(u32, usize, usize)> = Vec::new(); // (band, start, end)
        for (col, &band) in col_band.iter().enumerate() {
            match region_runs.last_mut() {
                Some((b, _, end)) if *b == band => *end = col + 1,
                _ => region_runs.push((band, col, col + 1)),
            }
        }
        // A declared `region` hint (Phase D1) reorders the regions so a
        // `top_left` group packs first and a `bottom_right` one last;
        // unhinted groups keep declaration order in the neutral middle
        // band. A stable sort means hints can never reorder two
        // unhinted groups relative to each other, and packing (which
        // follows this order) can never produce overlapping boxes.
        region_runs.sort_by_key(|&(band, _, _)| {
            let hinted = usize::try_from(band)
                .ok()
                .and_then(|b| group_bands.get(b))
                .and_then(|name| board.group(name))
                .and_then(|g| g.region.as_ref())
                .map(quadrant_key);
            hinted.unwrap_or(QUADRANT_NEUTRAL)
        });

        // Local x offsets *within* a region, reset at each region
        // boundary. Inside a declared group columns may sit closer
        // (`INTRA_GROUP_CLUSTER_DX`) than between unrelated regions; the
        // ungrouped region keeps the historic full pitch so ungrouped
        // boards stay byte-identical.
        let mut col_x_local: Vec<f64> = vec![0.0; col_groups.len()];
        for &(band, start, end) in &region_runs {
            let floor = if band == u32::MAX {
                pitch_floor
            } else {
                INTRA_GROUP_CLUSTER_DX
            };
            let mut cum = 0.0;
            for col in start..end {
                if col > start {
                    let gap = snap_grid(
                        floor.max(col_max_half_w[col - 1] + col_max_half_w[col] + WIRE_MARGIN),
                    );
                    cum += gap;
                }
                col_x_local[col] = cum;
            }
        }

        // Lowest BK unit, subtracted so each region's vertical
        // ordering starts at zero in its own local frame.
        let bk_min = if bk_units.is_empty() {
            0.0
        } else {
            bk_units.iter().copied().fold(f64::INFINITY, f64::min)
        };

        let mut placements: Vec<ComponentPlacement> = Vec::with_capacity(board.components.len());
        if region_runs.len() <= 1 {
            // Single region (an ungrouped board, or every component in
            // one group): the pre-region path exactly — absolute
            // anchors, one `place_cluster_into` per cluster. No packing,
            // no translation, so the output is unchanged for every
            // design that declares no groups.
            let mut occupied: std::collections::HashSet<(i64, i64)> =
                std::collections::HashSet::new();
            // Shelf-wrap a band that is too wide for the page shape
            // (schematic-quality plan Phase A5/C3).
            //
            // Layers are columns, so a board with many distinct layers
            // lays every column side by side in one band. Wrapping only
            // ever kicked in *within* a populous layer (`chunks(rows)`),
            // never across layers, so a board with a dozen one-cluster
            // layers spread 290 mm wide and 118 mm tall across a
            // 420 x 297 mm page: 27 % fill, the bottom two thirds empty,
            // and every inter-layer net long enough to degrade into a
            // label. Folding the column sequence onto successive shelves
            // trades width for height until the content is roughly
            // page-shaped. A band already within `TARGET_ASPECT` is left
            // exactly as it was, so balanced boards do not move.
            let band_w = col_x_local.iter().copied().fold(0.0_f64, f64::max) + grid_w;
            // The shelf count is swept by the fitting loop, not guessed
            // here: how many shelves land on the smallest page depends
            // on the sheet's usable aspect (which the title block eats
            // into asymmetrically), so it can only be judged against a
            // concrete candidate sheet. One shelf reproduces the
            // historic single band exactly.
            let shelves = shelves_hint.max(1).min(col_groups.len().max(1));
            {
                // Cut the column sequence into shelves of roughly equal
                // width, keeping reading order: a shelf break never
                // reorders columns, so power still flows left-to-right
                // along each shelf and top-to-bottom between them.
                let shelf_target = band_w / shelves as f64;
                let mut shelf_of: Vec<usize> = Vec::with_capacity(col_groups.len());
                let mut shelf = 0_usize;
                let mut shelf_start_x = 0.0_f64;
                for (col, &x) in col_x_local.iter().enumerate().take(col_groups.len()) {
                    if col > 0 && x - shelf_start_x > shelf_target {
                        shelf += 1;
                        shelf_start_x = x;
                    }
                    shelf_of.push(shelf);
                }
                // Place each shelf in its own local frame, measure what
                // it actually occupies, then stack the shelves. Measuring
                // beats predicting: members hang above and below their
                // anchors by amounts only `place_cluster_into` knows.
                let mut shelf_y = PAGE_MARGIN;
                for shelf_idx in 0..=shelf {
                    let mut local: Vec<ComponentPlacement> = Vec::new();
                    let mut local_occupied: std::collections::HashSet<(i64, i64)> =
                        std::collections::HashSet::new();
                    let base_x = col_groups
                        .iter()
                        .enumerate()
                        .filter(|(col, _)| shelf_of[*col] == shelf_idx)
                        .map(|(col, _)| col_x_local[col])
                        .fold(f64::INFINITY, f64::min);
                    for (col, group) in col_groups.iter().enumerate() {
                        if shelf_of[col] != shelf_idx {
                            continue;
                        }
                        for &cluster_idx in group {
                            let anchor_x = snap_grid(col_x_local[col] - base_x);
                            let anchor_y = snap_grid((bk_units[cluster_idx] - bk_min) * grid_h);
                            place_cluster_into(
                                board,
                                &clusters[cluster_idx],
                                anchor_x,
                                anchor_y,
                                grid_w,
                                &mut local,
                                &mut local_occupied,
                            );
                        }
                    }
                    let (local_min_x, _, local_min_y, local_max_y) = body_bbox_of(board, &local)
                        .unwrap_or((origin_x, origin_x + grid_w, 0.0, grid_h));
                    let dy = snap_grid(shelf_y - local_min_y);
                    let dx = snap_grid(PAGE_MARGIN - local_min_x);
                    for mut place in local {
                        place.center_mm.0 += dx;
                        place.center_mm.1 += dy;
                        let key = (
                            (place.center_mm.0 * 10.0).round() as i64,
                            (place.center_mm.1 * 10.0).round() as i64,
                        );
                        occupied.insert(key);
                        placements.push(place);
                    }
                    shelf_y += (local_max_y - local_min_y) + WIRE_MARGIN;
                }
            }
        } else {
            // Phase A (C1): lay out each region independently in its own
            // local frame — origin at (0,0) — so its *true* content
            // rectangle is known before packing. Using the grid cell
            // instead over-counts badly (one tall USB-C symbol inflates
            // every region's cell height), which is what tipped a
            // grouped board off A2 in the first cut of this pass.
            let mut region_layouts: Vec<RegionLayout> = Vec::with_capacity(region_runs.len());
            for &(band, start, end) in &region_runs {
                let mut local: Vec<ComponentPlacement> = Vec::new();
                let mut occupied: std::collections::HashSet<(i64, i64)> =
                    std::collections::HashSet::new();
                for col in start..end {
                    for &cluster_idx in &col_groups[col] {
                        let anchor_x = snap_grid(col_x_local[col]);
                        let anchor_y = snap_grid((bk_units[cluster_idx] - bk_min) * grid_h);
                        place_cluster_into(
                            board,
                            &clusters[cluster_idx],
                            anchor_x,
                            anchor_y,
                            grid_w,
                            &mut local,
                            &mut occupied,
                        );
                    }
                }
                local.sort_by_key(|p| p.id.0);
                let bbox = body_bbox_of(board, &local).unwrap_or((0.0, grid_w, 0.0, grid_h));
                let group_name = usize::try_from(band)
                    .ok()
                    .and_then(|b| group_bands.get(b))
                    .copied();
                region_layouts.push(RegionLayout {
                    placements: local,
                    bbox,
                    box_overhang: group_box_overhang(board, group_name),
                });
            }

            // Phase B: shelf-pack the *measured* region rectangles
            // left-to-right, wrapping to a new shelf when the running
            // width passes the target, so regions tile the page in two
            // dimensions instead of one long horizontal strip (reference
            // mechanism 1: the page is a grid of titled regions). Target
            // width balances total region area to the sheet aspect,
            // floored at the widest region so one wide region never
            // overflows its own shelf.
            let region_gap = REGION_GAP;
            let total_area: f64 = region_layouts
                .iter()
                .map(|r| (r.bbox.1 - r.bbox.0) * (r.bbox.3 - r.bbox.2))
                .sum();
            let widest = region_layouts
                .iter()
                .map(|r| r.bbox.1 - r.bbox.0)
                .fold(0.0_f64, f64::max);
            // Shelf width is swept by the fitting loop (`region_target_w`)
            // rather than fixed at A4's content width. Which regions
            // share a shelf decides the stack's height, and only a
            // concrete candidate sheet can say whether a wider, shorter
            // arrangement or a narrower, taller one lands on the smaller
            // page: a three-region board packs 150+110 on one shelf and
            // drops from A2 to A3, which the A4-width target could never
            // find. Floored at the widest region so one wide region
            // never overflows its own shelf.
            let target_w = region_target_w.max(widest).min(
                (total_area * TARGET_ASPECT)
                    .sqrt()
                    .max(widest)
                    .max(region_target_w),
            );
            let mut region_offset: Vec<(f64, f64)> = vec![(0.0, 0.0); region_layouts.len()];
            {
                let mut cx = 0.0;
                let mut cy = 0.0;
                let mut shelf_h = 0.0;
                for (i, region) in region_layouts.iter().enumerate() {
                    let w = region.bbox.1 - region.bbox.0 + 2.0 * GROUP_BOX_PAD;
                    // Pack against what is *drawn* — the box — not the
                    // bodies inside it. The box reaches above the bodies
                    // for its caption and below them for its note block,
                    // so packing on body bounds alone let two boxes
                    // overlap (`E-SYNTH-SCHEM-013`) even with a generous
                    // gap between the parts themselves.
                    let h = region.bbox.3 - region.bbox.2
                        + region.box_overhang.0
                        + region.box_overhang.1;
                    if cx > 0.0 && cx + w > target_w {
                        cy += shelf_h + region_gap;
                        cx = 0.0;
                        shelf_h = 0.0;
                    }
                    region_offset[i] = (cx, cy);
                    cx += w + region_gap;
                    shelf_h = shelf_h.max(h);
                }
            }

            // Phase C: translate each region's local placements to its
            // slot, normalising the region's own top-left to the slot
            // origin.
            // Hug the page margin rather than the grid origin: the
            // per-region bbox already reserves whatever its members
            // need above and left of their anchors, so `origin_x`/
            // `origin_y`'s guard band is pure waste here — ~55 mm of it,
            // enough to cost a sheet size on its own.
            for (ri, region) in region_layouts.iter().enumerate() {
                let dx = PAGE_MARGIN + GROUP_BOX_PAD + region_offset[ri].0 - region.bbox.0;
                let dy = PAGE_MARGIN + region.box_overhang.0 + region_offset[ri].1 - region.bbox.2;
                for placement in &region.placements {
                    placements.push(ComponentPlacement {
                        id: placement.id,
                        center_mm: (
                            snap_grid(placement.center_mm.0 + dx),
                            snap_grid(placement.center_mm.1 + dy),
                        ),
                        rotation: placement.rotation,
                    });
                }
            }
        }

        // Restore IR component order in the output so consumers that
        // iterate by index see the same order as `board.components`.
        placements.sort_by_key(|p| p.id.0);

        // Fit the sheet to the *actual* content bounding box rather than
        // the theoretical grid. The grid over-counts: cells are padded
        // to the largest symbol in the
        // whole board, so a design with one large MCU and a handful of
        // small parts is only as big as the parts actually drawn. The
        // reference designs (e.g. a sensor logger with a 76 mm-tall STM32
        // symbol) land in A4/A3 this way instead of blowing out to A2.
        let (min_x, max_x, min_y, max_y) = body_bbox_of(board, &placements).unwrap_or((
            origin_x,
            origin_x + col_groups.len().max(1) as f64 * grid_w,
            origin_y,
            origin_y + rows as f64 * grid_h,
        ));
        (placements, (min_x, max_x, min_y, max_y))
    };

    // Grow the column height until the content fits a real sheet.
    //
    // `rows_aspect` alone only balances the grid's *aspect*; it says
    // nothing about whether the result fits any page we can declare.
    // Capping it to A4's title-block band (which is all this used to
    // do) makes that worse for boards whose symbols force a tall
    // `grid_h`: only 2-3 rows fit above A4's block, so every extra
    // cluster wraps into another column and the sheet grows sideways
    // without bound while the lower two thirds of the page stay empty
    // (observed: a 23-part USB-C power board reaching x≈1031mm on a
    // 594mm-wide A2, and `esp32_c61_devboard` reaching x≈605mm).
    // Sheets only escalate to A2, so unbounded width is real overflow
    // (`E-SYNTH-SCHEM-007`), not just an ugly aspect ratio.
    //
    // So: start at the aspect-derived height capped to A4 — the shape
    // that has always been used, and the one every design that already
    // fits keeps — then retry with progressively taller columns (fewer,
    // longer columns → narrower content) up to what A2 can hold, and
    // keep the attempt that lands on the smallest sheet. Smallest, not
    // first-fit: `sheet_size_for`'s product decision is that a design
    // should stay on the smallest page that holds it, and a stack one
    // row taller often drops a board from A2 back to A3. Ties keep the
    // earliest (most aspect-balanced) attempt. If nothing fits — only
    // possible past A2, where sheets stop growing (the §P26 multi-sheet
    // split handles it) — keep whichever overflowed least, so the ERC
    // diagnostic reports the smallest violation.
    let rows_start = rows_aspect.min(max_rows_for_sheet(SheetSize::A4)).max(1);
    let rows_cap = max_rows_for_sheet(SheetSize::A2)
        .min(cluster_count)
        .max(rows_start);

    // Banding preference order: keep declared groups contiguous if any
    // page can hold that, otherwise fall back to layer-only banding.
    // A group boundary costs columns — six banded groups do not pack as
    // tightly as six layers — and a schematic that runs off the page is
    // strictly worse than one whose captions sit over slightly mixed
    // regions. Ungrouped boards produce identical keys for both passes,
    // so they simply place once.
    let bandings: [Bandings<'_>; 2] = [(&grouped_key, &grouped_order), (&flat_key, &flat_order)];

    // Selection key: smallest sheet first, then the attempt whose
    // content shape best matches that sheet's (schematic-quality plan
    // Phase A5/C3).
    //
    // Area alone was not enough. Every attempt that lands on the same
    // sheet ties on area, so the first — the most aspect-balanced
    // `rows_start`, which is also the *shortest* column stack and
    // therefore the widest content — always won. On A3 that left a
    // 290 x 118 mm band across the top of a 420 x 297 mm page: 27 %
    // fill, the lower two thirds empty, and every net long enough to
    // degrade into a label (`E-SYNTH-SCHEM-012`, and the reason the
    // reference sheet's "wires inside a block" reads so much better).
    // Ranking ties by how close the content's aspect is to the page's
    // picks the squarer stack instead, which uses the page the way a
    // drawn-by-hand sheet does.
    let mut fitted: Option<(f64, usize, usize, f64, Vec<ComponentPlacement>, SheetSize)> = None;
    let mut closest: Option<(f64, Vec<ComponentPlacement>, SheetSize)> = None;
    for (fallback_key, fallback_order) in bandings {
        if fitted.is_some() {
            break;
        }
        // `BASE_CLUSTER_DX` is a generous default gap between unrelated
        // columns. It is tried first, so a board that already fits its
        // page keeps the airier spacing; the tighter floor is only
        // reached for a board that would otherwise need a larger sheet,
        // where a denser page beats a sparser bigger one.
        for (rows, shelves, pitch_floor, region_w) in (rows_start..=rows_cap).flat_map(|r| {
            (1..=MAX_SHELVES).flat_map(move |sh| {
                [BASE_CLUSTER_DX, INTRA_GROUP_CLUSTER_DX]
                    .into_iter()
                    .flat_map(move |p| REGION_SHELF_WIDTHS.map(move |w| (r, sh, p, w)))
            })
        }) {
            let (placements, (min_x, max_x, min_y, max_y)) = place_for_rows(
                rows,
                shelves,
                pitch_floor,
                region_w,
                fallback_key,
                fallback_order,
            );
            let (need_w, need_h) = sheet_needs(min_x, max_x, min_y, max_y);
            let sheet_size = sheet_size_for(need_w, need_h);
            let (sheet_w, sheet_h) = sheet_size.dims_mm();
            if need_w <= sheet_w && need_h <= sheet_h {
                let area = sheet_w * sheet_h;
                let mismatch = if need_h > 0.0 && sheet_h > 0.0 {
                    (need_w / need_h - sheet_w / sheet_h).abs()
                } else {
                    f64::INFINITY
                };
                // Rank: smallest sheet, then fewest shelves, then the
                // airier column pitch, then the best aspect match.
                //
                // Both shelf-wrapping and pitch-tightening are spent
                // only on *saving a sheet size*, never on a page that
                // already fits. Wrapping costs the left-to-right
                // power-flow reading order — a shelf break continues
                // on the next line, so a late-layer passive can sit
                // left of an early-layer connector — and tightening
                // pushes nets past the span threshold until they
                // degrade into labels. Both are worth it to drop A3 to
                // A4; neither is worth it otherwise.
                let pitch_rank = usize::from(pitch_floor < BASE_CLUSTER_DX);
                let key = (area, shelves, pitch_rank, mismatch);
                let improves = fitted.as_ref().is_none_or(
                    |(best_area, best_shelves, best_pitch, best_mismatch, _, _)| {
                        key < (*best_area, *best_shelves, *best_pitch, *best_mismatch)
                    },
                );
                if improves {
                    fitted = Some((area, shelves, pitch_rank, mismatch, placements, sheet_size));
                }
                continue;
            }
            let overflow = (need_w - sheet_w).max(0.0) + (need_h - sheet_h).max(0.0);
            if closest.as_ref().is_none_or(|(best, _, _)| overflow < *best) {
                closest = Some((overflow, placements, sheet_size));
            }
        }
    }
    let fitted = fitted.map(|(_, _, _, _, placements, sheet)| (0.0, placements, sheet));
    let (components, sheet_size) = fitted
        .or(closest)
        .map_or((Vec::new(), SheetSize::A4), |(_, placements, sheet)| {
            (placements, sheet)
        });

    Layout {
        components,
        wires: Vec::new(),
        junctions: Vec::new(),
        power_flags: Vec::new(),
        net_labels: Vec::new(),
        annotations: Vec::new(),
        hierarchical_labels: Vec::new(),
        group_boxes: Vec::new(),
        sheet_size,
    }
}

/// One region's independently-laid-out content and its measured
/// rectangle, in the region's own local frame (Phase C1).
struct RegionLayout {
    placements: Vec<ComponentPlacement>,
    bbox: (f64, f64, f64, f64),
    /// Extra height its group box needs beyond the body bounds:
    /// padding above, plus padding and the note block below.
    box_overhang: (f64, f64),
}

/// Body bounding box of a set of placements, in the same frame as
/// their centres: `(min_x, max_x, min_y, max_y)`, or `None` when
/// empty. Shared by the per-region measurement (Phase C1) and the
/// final content box so region packing and sheet fitting agree on how
/// big the drawn content actually is.
fn body_bbox_of(board: &Board, placements: &[ComponentPlacement]) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for placement in placements {
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
        min_x = min_x.min(cx - bw / 2.0);
        max_x = max_x.max(cx + bw / 2.0);
        min_y = min_y.min(cy - bh / 2.0);
        max_y = max_y.max(cy + bh / 2.0);
    }
    min_x.is_finite().then_some((min_x, max_x, min_y, max_y))
}

/// Place one cluster's anchor and members into `placements`, updating
/// `occupied` so later clusters avoid the same grid points.
///
/// `anchor_x`/`anchor_y` are the anchor's centre in whatever frame the
/// caller lays the cluster out in — absolute for a single ungrouped
/// region, region-local for the packed path (Phase C1) — so the same
/// member geometry serves both and can never diverge between them.
#[allow(clippy::too_many_arguments)]
fn place_cluster_into(
    board: &Board,
    cluster: &Cluster,
    anchor_x: f64,
    anchor_y: f64,
    grid_w: f64,
    placements: &mut Vec<ComponentPlacement>,
    occupied: &mut std::collections::HashSet<(i64, i64)>,
) {
    // Anchor body half-extents so member placement clears the
    // actual rendered footprint regardless of how big the
    // KiCad symbol is. Fixed `MEMBER_DX/MEMBER_DY` constants
    // assume a small generic rectangle; an ATmega328P-P DIP
    // (~50 mm tall) or a USB-C receptacle (~50 mm tall × 15 mm
    // wide) needs more headroom than that.
    let (anchor_body_w, anchor_body_h) = board
        .component(cluster.anchor)
        .and_then(|c| c.part.as_ref())
        .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
    let anchor_half_w = anchor_body_w / 2.0;
    let anchor_half_h = anchor_body_h / 2.0;

    let anchor_key = (
        (anchor_x * 10.0).round() as i64,
        (anchor_y * 10.0).round() as i64,
    );
    occupied.insert(anchor_key);

    placements.push(ComponentPlacement {
        id: cluster.anchor,
        center_mm: (anchor_x, anchor_y),
        rotation: Rotation::Zero,
    });

    // Bucket members by side.
    let below: Vec<ComponentId> = cluster
        .members
        .iter()
        .filter(|m| m.side == MemberSide::Below)
        .map(|m| m.id)
        .collect();
    let right: Vec<ComponentId> = cluster
        .members
        .iter()
        .filter(|m| m.side == MemberSide::Right)
        .map(|m| m.id)
        .collect();
    let above: Vec<ComponentId> = cluster
        .members
        .iter()
        .filter(|m| m.side == MemberSide::Above)
        .map(|m| m.id)
        .collect();
    let left: Vec<ComponentId> = cluster
        .members
        .iter()
        .filter(|m| m.side == MemberSide::Left)
        .map(|m| m.id)
        .collect();

    // Below: members fan out in a single row beneath the anchor,
    // horizontally aligned with their connected pins where possible.
    if !below.is_empty() {
        let member_dx = compute_dynamic_member_dx(board, &below);
        let row_y = snap_grid(anchor_y + anchor_half_h + row_clearance(board, &below, true));
        let total_width = (below.len().saturating_sub(1)) as f64 * member_dx;
        let row_start_x = snap_grid(anchor_x - total_width / 2.0);
        let mut last_x: Option<f64> = None;
        for (i, id) in below.iter().enumerate() {
            let mut placed_x = snap_grid(row_start_x + i as f64 * member_dx);
            if let Some(lx) = last_x {
                placed_x = placed_x.max(snap_grid(lx + member_dx));
            }
            if let Some(pin_idx) = find_connecting_active_pin(board, cluster.anchor, *id) {
                if let Some(anchor) = board.component(cluster.anchor) {
                    if let Some(part) = anchor.part.as_ref() {
                        let (px, _py, _side) = compute_anchor_pin_offset(part, pin_idx);
                        let target_x = snap_grid(anchor_x + px);
                        let coord_key = (
                            (target_x * 10.0).round() as i64,
                            (row_y * 10.0).round() as i64,
                        );
                        let remains_local = (target_x - anchor_x).abs() <= grid_w / 3.0;
                        let clears_neighbour = placements.iter().all(|placed| {
                            (placed.center_mm.1 - row_y).abs() > 0.1
                                || (placed.center_mm.0 - target_x).abs() >= member_dx * 0.8
                        });
                        let clears_last = last_x.is_none_or(|lx| target_x >= lx + member_dx * 0.8);
                        if remains_local
                            && clears_neighbour
                            && clears_last
                            && !occupied.contains(&coord_key)
                        {
                            placed_x = target_x;
                        }
                    }
                }
            }
            if let Some(lx) = last_x {
                placed_x = placed_x.max(snap_grid(lx + member_dx));
            }
            last_x = Some(placed_x);
            let coord_key = (
                (placed_x * 10.0).round() as i64,
                (row_y * 10.0).round() as i64,
            );
            occupied.insert(coord_key);

            placements.push(ComponentPlacement {
                id: *id,
                center_mm: (placed_x, row_y),
                rotation: Rotation::Zero,
            });
        }
    }

    // Right: members stack in a vertical column to the right of
    // the anchor, vertically aligned with their connected pins
    // where possible. Spacing between consecutive members is
    // dynamic (`min_member_dy`, text-inclusive) rather than the
    // fixed `MEMBER_DX`, and pin-alignment nudges are clamped
    // against it — a fixed constant or an unclamped pin-aligned
    // position can both leave less room than a member's own
    // Reference/Value text needs, overlapping its neighbour.
    if !right.is_empty() {
        let default_col_x = snap_grid(anchor_x + anchor_half_w + MEMBER_CLEARANCE);
        let total_height: f64 = right
            .windows(2)
            .map(|pair| min_member_dy(board, pair[0], pair[1]))
            .sum();
        let col_start_y = snap_grid(anchor_y - total_height / 2.0);
        let mut prev: Option<(ComponentId, f64)> = None;
        for (i, id) in right.iter().enumerate() {
            let mut col_x = default_col_x;
            let mut placed_y = if i == 0 {
                col_start_y
            } else {
                let (prev_id, prev_y) = prev.unwrap();
                snap_grid(prev_y + min_member_dy(board, prev_id, *id))
            };
            // Pin-Y alignment is only safe for a single member —
            // with more than one, distinct members can each
            // align to a different anchor pin that happens to
            // sit within a couple mm of another (e.g. a reset
            // network's pullup/button/debounce-cap all landing
            // near VCC/RESET/GND pins on a real KiCad symbol),
            // collapsing their text-inclusive extents on top of
            // each other. Mirrors the identical guard already on
            // the Left column below.
            if right.len() == 1 {
                if let Some(pin_idx) = find_connecting_active_pin(board, cluster.anchor, *id) {
                    if let Some(anchor) = board.component(cluster.anchor) {
                        if let Some(part) = anchor.part.as_ref() {
                            let (px, py, side) = compute_anchor_pin_offset(part, pin_idx);
                            // Only honour pin-Y alignment when the
                            // connecting pin actually sits on the
                            // Right side of the anchor. Otherwise
                            // (e.g. a reset pull-up whose other end
                            // hits VCC on Top), `find_connecting_*`
                            // would pick that wrong pin and yank
                            // the member up-and-inside the body.
                            if side == PinSide::Right {
                                col_x = snap_grid(anchor_x + px + MEMBER_CLEARANCE);
                                let target_y = snap_grid(anchor_y + py);
                                let coord_key = (
                                    (col_x * 10.0).round() as i64,
                                    (target_y * 10.0).round() as i64,
                                );
                                if !occupied.contains(&coord_key) {
                                    placed_y = target_y;
                                }
                            }
                        }
                    }
                }
            } else if let Some((prev_id, prev_y)) = prev {
                // Multi-member column, default spacing already
                // applied above — still clamp in case a future
                // change reintroduces per-member pin alignment
                // here without threading it through this check.
                placed_y = placed_y.max(snap_grid(prev_y + min_member_dy(board, prev_id, *id)));
            }
            let coord_key = (
                (col_x * 10.0).round() as i64,
                (placed_y * 10.0).round() as i64,
            );
            occupied.insert(coord_key);

            placements.push(ComponentPlacement {
                id: *id,
                center_mm: (col_x, placed_y),
                rotation: Rotation::Zero,
            });
            prev = Some((*id, placed_y));
        }
    }

    // Above: LED limit resistors remain in a vertical chain.
    // Pull-ups for an IC fan into a horizontal row above it,
    // which is the usual readable bus-pull-up arrangement.
    if !above.is_empty() {
        let member_dx = compute_dynamic_member_dx(board, &above);
        let horizontal_row_y = snap_grid(anchor_y - anchor_half_h - MEMBER_CLEARANCE);
        let horizontal_width = (above.len().saturating_sub(1)) as f64 * member_dx;
        let horizontal_start_x = snap_grid(anchor_x - horizontal_width / 2.0);
        let mut last_x: Option<f64> = None;
        for (i, id) in above.iter().enumerate() {
            let mut placed_x = if cluster.anchor_vertical {
                anchor_x
            } else {
                snap_grid(horizontal_start_x + i as f64 * member_dx)
            };
            let target_y = if cluster.anchor_vertical {
                snap_grid(anchor_y - anchor_half_h - MEMBER_CLEARANCE - (i as f64) * MEMBER_DX)
            } else {
                horizontal_row_y
            };
            if !cluster.anchor_vertical {
                if let Some(lx) = last_x {
                    placed_x = placed_x.max(snap_grid(lx + member_dx));
                }
            }
            if let Some(pin_idx) = find_connecting_active_pin(board, cluster.anchor, *id) {
                if let Some(anchor) = board.component(cluster.anchor) {
                    if let Some(part) = anchor.part.as_ref() {
                        let (px, _py, _side) = compute_anchor_pin_offset(part, pin_idx);
                        let target_x = snap_grid(anchor_x + px);
                        let coord_key = (
                            (target_x * 10.0).round() as i64,
                            (target_y * 10.0).round() as i64,
                        );
                        let remains_local = (target_x - anchor_x).abs() <= grid_w / 3.0;
                        let clears_neighbour = placements.iter().all(|placed| {
                            (placed.center_mm.1 - target_y).abs() > 0.1
                                || (placed.center_mm.0 - target_x).abs() >= member_dx * 0.8
                        });
                        let clears_last = last_x.is_none_or(|lx| target_x >= lx + member_dx * 0.8);
                        if remains_local
                            && clears_neighbour
                            && clears_last
                            && !occupied.contains(&coord_key)
                        {
                            placed_x = target_x;
                        }
                    }
                }
            }
            if !cluster.anchor_vertical {
                if let Some(lx) = last_x {
                    placed_x = placed_x.max(snap_grid(lx + member_dx));
                }
                last_x = Some(placed_x);
            }
            let coord_key = (
                (placed_x * 10.0).round() as i64,
                (target_y * 10.0).round() as i64,
            );
            occupied.insert(coord_key);

            placements.push(ComponentPlacement {
                id: *id,
                center_mm: (placed_x, target_y),
                rotation: Rotation::Zero,
            });
        }
    }

    // Left: members stack vertically just outside the anchor's
    // left edge. Dynamic (`min_member_dy`, text-inclusive)
    // spacing so members are visually distinct — pin-Y alignment
    // would collapse them on top of each other when the anchor's
    // pins are 2.54mm apart (e.g. USB connector D+/D- adjacency).
    // A single Left member has no such adjacency, so it y-aligns
    // with its connecting pin for a straight-across wire.
    if !left.is_empty() {
        let col_x = snap_grid(anchor_x - anchor_half_w - MEMBER_CLEARANCE);
        let total_height: f64 = left
            .windows(2)
            .map(|pair| min_member_dy(board, pair[0], pair[1]))
            .sum();
        let col_start_y = snap_grid(anchor_y - total_height / 2.0);
        let mut prev: Option<(ComponentId, f64)> = None;
        for (i, id) in left.iter().enumerate() {
            let mut placed_y = if i == 0 {
                col_start_y
            } else {
                let (prev_id, prev_y) = prev.unwrap();
                snap_grid(prev_y + min_member_dy(board, prev_id, *id))
            };
            if left.len() == 1 {
                if let Some(pin_idx) = find_connecting_active_pin(board, cluster.anchor, *id) {
                    if let Some(anchor) = board.component(cluster.anchor) {
                        if let Some(part) = anchor.part.as_ref() {
                            let (_px, py, side) = compute_anchor_pin_offset(part, pin_idx);
                            if side == PinSide::Left {
                                let target_y = snap_grid(anchor_y + py);
                                let coord_key = (
                                    (col_x * 10.0).round() as i64,
                                    (target_y * 10.0).round() as i64,
                                );
                                if !occupied.contains(&coord_key) {
                                    placed_y = target_y;
                                }
                            }
                        }
                    }
                }
            } else if let Some((prev_id, prev_y)) = prev {
                placed_y = placed_y.max(snap_grid(prev_y + min_member_dy(board, prev_id, *id)));
            }
            let coord_key = (
                (col_x * 10.0).round() as i64,
                (placed_y * 10.0).round() as i64,
            );
            occupied.insert(coord_key);

            placements.push(ComponentPlacement {
                id: *id,
                center_mm: (col_x, placed_y),
                rotation: Rotation::OneEighty,
            });
            prev = Some((*id, placed_y));
        }
    }
}

/// Page dimensions a content bounding box needs, in mm.
///
/// Both axes take the larger of two demands, because a sheet has to
/// contain the content in *absolute* page coordinates, not merely be
/// as large as the content's own extent:
///
/// - width: the content's right edge plus a margin, or its own width
///   plus a margin on each side, whichever is larger;
/// - height: the content's bottom edge plus the title-block band it
///   must clear (see `TITLE_BLOCK_H`), or its own height plus a margin
///   on each side, whichever is larger.
///
/// Public so a consumer can tell whether a sheet is already the smallest
/// that fits it: a page sized to `sheet_needs` cannot be shrunk further,
/// whatever its area ratio.
pub fn sheet_needs(min_x: f64, max_x: f64, min_y: f64, max_y: f64) -> (f64, f64) {
    let w = (max_x + PAGE_MARGIN).max(max_x - min_x + 2.0 * PAGE_MARGIN);
    let h = (max_y + TITLE_BLOCK_H + TEXT_MARGIN_Y).max(max_y - min_y + 2.0 * PAGE_MARGIN);
    (w, h)
}

// ----- Power-net classification -------------------------------------------

/// Walk every net in `board` and emit `PowerFlag`s for nets that
/// the renderer should treat as power rails (drawn as
/// per-pin symbols, not as wires).
///
/// Classification rules:
///
/// - A net with ≥2 pins named `gnd` / `vss` / `vssa` / `gnda` is
///   GND. Label `"GND"`.
/// - A net with a `power_output` pin OR ≥2 `power_input` pins (and
///   not classified as GND above) is a positive rail. Label is the
///   uppercased name of the first defining pin we see (`"VOUT"`,
///   `"VBUS"`, …) — better than a generic `"VCC"` when the design
///   has multiple rails.
/// - Any net touching a `power_output` or `power_input` pin
///   becomes a power net regardless of size — even a 2-endpoint
///   net like `U1.vin <-> C2.p1` qualifies, so the bulk cap on
///   VBUS gets a VCC flag and rotates vertical alongside the
///   regulator's own decoupling.
///
/// Derive the rail label from the net's *user-declared* name, if it
/// carries real identity.
///
/// A declared name like `AGND`, `DGND` or `+5V_USB` is the
/// strongest available signal of rail intent — stronger than any
/// pin-name heuristic — because distinct declared grounds/rails must
/// stay distinct after export (power symbols connect globally by
/// Value in KiCad). Generic tokens (`gnd`, `vcc`, ...) and the
/// auto-generated `net_N` names carry no such identity and return
/// `None` so the pre-existing pin-name/regulator derivation applies,
/// keeping legacy single-domain designs byte-identical.
#[must_use]
pub fn declared_rail_name(net_name: &str) -> Option<String> {
    let lower = net_name.to_ascii_lowercase();
    let generic = matches!(
        lower.as_str(),
        "gnd" | "ground" | "vcc" | "vdd" | "vss" | "vdda" | "vssa"
    ) || lower.starts_with("net_");
    if generic {
        return None;
    }
    sanitize_rail_label(net_name)
}

/// Restrict a rail label to KiCad-safe characters (alphanumerics,
/// `+`, `-`, `_`). Returns `None` when nothing survives — such a
/// label would render an unusable power symbol Value.
#[must_use]
fn sanitize_rail_label(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn classify_power_flags(board: &Board) -> Vec<PowerFlag> {
    let mut flags = Vec::new();
    // Label uniqueness guard: two distinct nets sharing one flag
    // label would merge into a single global net inside KiCad
    // (power-symbol Values connect by name). Colliding latecomers
    // get a deterministic `_<net ordinal>` suffix.
    let mut seen_labels: std::collections::HashMap<String, NetId> =
        std::collections::HashMap::new();
    for net in &board.nets {
        let Some((kind, label)) = classify_power_net(board, net) else {
            continue;
        };
        // Cross-net collision guard (see `seen_labels` above): if a
        // different net already claimed this label, suffix this
        // net's unique ordinal — one suffix always suffices because
        // the ordinal is unique per net.
        let label = if seen_labels
            .iter()
            .any(|(l, owner)| *owner != net.id && l == &label)
        {
            format!("{label}_{}", net.id.0)
        } else {
            label
        };
        seen_labels.insert(label.clone(), net.id);

        for endpoint in &net.endpoints {
            flags.push(PowerFlag {
                net: net.id,
                component: endpoint.component,
                pin: endpoint.pin,
                kind,
                label: label.clone(),
            });
        }
    }
    flags
}

/// Classify a single net as a positive rail or ground, using exactly
/// the rule the schematic power flags use, or `None` for a signal net.
///
/// Shared with [`crate::netclass`] so the drawn power symbol and the
/// net-class colour can never disagree about what is a rail. The
/// returned label is the pre-collision suggestion; callers that emit
/// symbols apply their own uniqueness guard.
///
/// - Ground: any endpoint pin is a `power_input` named `gnd`/`vss`/…
///   A declared name (`AGND`, `DGND`, …) wins so separate grounds stay
///   separate; unnamed nets keep the classic global `GND`.
/// - Rail: a `power_output` pin, or any `power_input` pin with a
///   non-ground name (`vin`, `vcc`, `vdd`, `vbus`).
pub(crate) fn classify_power_net(
    board: &Board,
    net: &synth_ir::Net,
) -> Option<(PowerFlagKind, String)> {
    use synth_registry::ElectricalType;
    if net.endpoints.len() < 2 {
        return None;
    }
    let mut has_power_output = false;
    let mut power_input_count = 0_usize;
    let mut gnd_pin_count = 0_usize;
    let mut output_label: Option<String> = None;
    let mut input_label: Option<String> = None;

    for endpoint in &net.endpoints {
        let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
            continue;
        };
        match pin.electrical_type {
            ElectricalType::PowerOutput => {
                has_power_output = true;
                if output_label.is_none() {
                    output_label = Some(
                        board
                            .component(endpoint.component)
                            .and_then(|c| c.part.as_ref())
                            .filter(|p| p.kind == "regulator")
                            .and_then(regulator_rail_label)
                            .unwrap_or_else(|| pin.name.to_ascii_uppercase()),
                    );
                }
            }
            ElectricalType::PowerInput => {
                power_input_count += 1;
                let lower = pin.name.to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "gnd" | "vss" | "vssa" | "gnda" | "gnd_a" | "ground"
                ) {
                    gnd_pin_count += 1;
                } else {
                    input_label.get_or_insert_with(|| pin.name.to_ascii_uppercase());
                }
            }
            _ => {}
        }
    }

    let declared = declared_rail_name(&net.name);
    if gnd_pin_count >= 1 {
        // Any net touching a gnd-named power pin is ground. One
        // endpoint is enough — even a 2-endpoint `U1.gnd → C2.p2` net
        // should fly a GND symbol.
        Some((
            PowerFlagKind::Gnd,
            declared.unwrap_or_else(|| "GND".to_string()),
        ))
    } else if has_power_output {
        Some((
            PowerFlagKind::Vcc,
            declared
                .or(output_label)
                .unwrap_or_else(|| "VCC".to_string()),
        ))
    } else if power_input_count >= 1 {
        // A `power_input` pin without a matching ground name (e.g.
        // `vin`, `vcc`, `vdd`, `vbus`) anchors a positive rail.
        Some((
            PowerFlagKind::Vcc,
            declared
                .or(input_label)
                .unwrap_or_else(|| "VCC".to_string()),
        ))
    } else {
        None
    }
}

/// Derive a human rail label for a fixed-voltage regulator part.
///
/// A rail driven by `ams1117_3v3`'s `vout` pin reads as `+3V3` on a
/// hand-drawn schematic, not `VOUT`. Beyond readability the name
/// also resolves against KiCad's bundled power-symbol library
/// (`power:+3V3` exists; `power:VOUT` does not), so the exporter
/// draws a real power arrow instead of a synthesized rectangle.
///
/// Searches the part id, then the MPN, then the description for a
/// voltage token and returns the first match.
fn regulator_rail_label(part: &synth_registry::Part) -> Option<String> {
    voltage_token(part.id.as_str())
        .or_else(|| voltage_token(part.mpn.as_deref().unwrap_or("")))
        .or_else(|| voltage_token(part.description.as_deref().unwrap_or("")))
}

/// Extract a `+3V3`-style label from free text containing a voltage
/// token. Recognises `3v3` / `1v8` / `5v` and dotted `3.3` / `1.8`
/// forms; the digit run must not be preceded by another digit so
/// `ams1117_3v3` matches `3v3`, not the `1117` model number.
fn voltage_token(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let n = b.len();
    // (start, int_part, frac_part) candidates in scan order.
    let mut dotted: Option<String> = None;
    let mut i = 0;
    while i < n {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        let int_part = &lower[start..i];
        // `<int>v<frac>` or `<int>v` — the preferred forms.
        if i < n && b[i] == b'v' {
            let fstart = i + 1;
            let mut fend = fstart;
            while fend < n && b[fend].is_ascii_digit() {
                fend += 1;
            }
            return Some(if fend > fstart {
                format!("+{}V{}", int_part, lower[fstart..fend].to_ascii_uppercase())
            } else {
                format!("+{int_part}V")
            });
        }
        // `<int>.<frac>` with no `v` (e.g. MPN `AMS1117-3.3`) —
        // remember the first one but keep scanning: a later
        // v-style token is a stronger signal.
        if dotted.is_none() && i < n && b[i] == b'.' {
            let fstart = i + 1;
            let mut fend = fstart;
            while fend < n && b[fend].is_ascii_digit() {
                fend += 1;
            }
            if fend > fstart {
                dotted = Some(format!(
                    "+{}V{}",
                    int_part,
                    lower[fstart..fend].to_ascii_uppercase()
                ));
            }
        }
    }
    dotted
}

/// Maximum span (in mm) of a signal net's endpoint bounding box
/// that still renders cleanly as a wire. Beyond this threshold the
/// wire would almost certainly cross other component bodies on the
/// way; emit per-endpoint net labels instead.
const LABEL_SPAN_THRESHOLD_MM: f64 = 80.0;

/// Multi-drop nets (≥3 endpoints, e.g. an I²C SDA/SCL bus touching an
/// MCU, a sensor, and its pull-up resistors) render as per-pin net
/// labels regardless of physical span, exactly like a hand-drawn
/// schematic. Drawing a star topology of wires for three-or-more
/// scattered endpoints produces a crossing tangle that labels avoid.
const LABEL_MIN_ENDPOINTS_MULTIDROP: usize = 3;

/// Classify which signal nets should render as labels instead of
/// wires. A net qualifies when:
///
/// - It is NOT already a power net (those use `PowerFlag` instead).
/// - It has ≥2 endpoints (1-endpoint nets are warnings, not wires).
/// - **Region crossing (Phase C2):** on a board that declares `group`s,
///   a net whose endpoints live in different regions becomes a label —
///   *"wires inside a region, labels between regions"*, the mechanism
///   that makes the reference sheet readable. A net entirely inside one
///   region is drawn. The distance threshold below stays as a secondary
///   guard for a large single region.
/// - **Ungrouped boards** keep the historic rule verbatim: a multi-drop
///   net (≥3 endpoints) or one spanning more than
///   [`LABEL_SPAN_THRESHOLD_MM`] gets labelled, so their output is
///   byte-identical.
fn classify_net_labels(board: &Board, layout: &Layout) -> Vec<NetLabel> {
    let centres: std::collections::HashMap<ComponentId, (f64, f64)> = layout
        .components
        .iter()
        .map(|p| (p.id, p.center_mm))
        .collect();
    let power_nets = layout.power_net_ids();
    // Whether the board declares any region at all. Only then does the
    // region-crossing test apply; an ungrouped board has one implicit
    // region and must keep its historic labelling.
    let region_of = |id: ComponentId| -> Option<&str> { effective_group(board, id) };
    let has_regions = board.components.iter().any(|c| c.group.is_some());
    let mut labels = Vec::new();
    for net in &board.nets {
        if power_nets.contains(&net.id) || net.endpoints.len() < 2 {
            continue;
        }
        // Span of endpoint centres.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for ep in &net.endpoints {
            if let Some(&(x, y)) = centres.get(&ep.component) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        let span_x = max_x - min_x;
        let span_y = max_y - min_y;
        let spans_far = span_x > LABEL_SPAN_THRESHOLD_MM || span_y > LABEL_SPAN_THRESHOLD_MM;
        let label_it = if has_regions {
            // Region crossing is the primary test; the span guard is a
            // secondary backstop for one very large region.
            let first = region_of(net.endpoints[0].component);
            let crosses = net
                .endpoints
                .iter()
                .any(|ep| region_of(ep.component) != first);
            crosses || spans_far
        } else {
            net.endpoints.len() >= LABEL_MIN_ENDPOINTS_MULTIDROP || spans_far
        };
        if !label_it {
            continue;
        }
        let label = pick_net_label(board, net).unwrap_or_else(|| format!("NET_{}", net.id.0));
        for ep in &net.endpoints {
            labels.push(NetLabel {
                net: net.id,
                component: ep.component,
                pin: ep.pin,
                label: label.clone(),
            });
        }
    }
    labels
}

/// Pick a human-meaningful label for a signal net.
///
/// Prefers a semantic function name derived from an active-IC pin's
/// capabilities (`i2c_scl` → `SCL`, `usb_dp` → `D+`, `spi_mosi` →
/// `MOSI`, …) so the schematic reads like a hand-drawn one. Falls
/// back to the active-IC pin's name uppercased (MCU, sensor, memory
/// chip, regulator), then to whatever endpoint is first if no
/// active-IC pin is available.
///
/// Also used by the KiCad exporter when a net proves unroutable as
/// a wire: the net is truncated to per-endpoint labels instead, the
/// same escape hatch a human reaches for.
pub fn pick_net_label(board: &Board, net: &synth_ir::Net) -> Option<String> {
    pick_net_label_with_source(board, net).map(|(text, _)| text)
}

/// [`pick_net_label`] paired with the component whose pin supplied
/// the returned token. The uniquify pass
/// ([`uniquify_net_labels`]) needs that component's refdes to build
/// deterministic `_REFDES` disambiguation suffixes; callers that only
/// want the string should keep using [`pick_net_label`].
pub(crate) fn pick_net_label_with_source(
    board: &Board,
    net: &synth_ir::Net,
) -> Option<(String, ComponentId)> {
    use synth_registry::PinCapability;
    // A net that is a member of a declared bus renders its full
    // `<bus>.<member>` name. This is the second exception to the
    // pin-derived-token policy (the first is `declared_rail_name`):
    // the name is what the exporter's `bus_alias` associates with, and
    // what a hand-drawn bus schematic shows, so a guessed `SDA` would
    // both hide the bus and collide across two buses with the same
    // member names.
    if let Some(label) = declared_bus_member_label(board, &net.name) {
        if let Some(ep) = net.endpoints.first() {
            return Some((label, ep.component));
        }
    }
    // NOTE: signal-net labels stay pin-derived (capability tokens
    // like `SDA`, else the active-IC pin name) even when the net
    // carries a user-declared name: the declared name already shows
    // in the netlist, the PCB, and (for rails) the power-flag
    // labels, while schematic local labels keep the hand-drawn
    // semantic tokens this function exists to produce. Power rails
    // are the exception — see `declared_rail_name`, which prefers
    // the declared name so `power "+3V3"` renders `+3V3`, never a
    // guessed `VCC`.
    let active_kinds: &[&str] = &[
        "mcu",
        "ic",
        "sensor",
        "memory",
        "opamp",
        "secure_element",
        "regulator",
    ];
    let semantic = |caps: &[PinCapability]| -> Option<String> {
        let name = caps.iter().find_map(|c| match c {
            PinCapability::I2cScl => Some("SCL"),
            PinCapability::I2cSda => Some("SDA"),
            PinCapability::SpiMosi => Some("MOSI"),
            PinCapability::SpiMiso => Some("MISO"),
            PinCapability::SpiSck => Some("SCK"),
            PinCapability::SpiCs => Some("CS"),
            PinCapability::UartTx => Some("TX"),
            PinCapability::UartRx => Some("RX"),
            PinCapability::UsbDp => Some("D+"),
            PinCapability::UsbDn => Some("D-"),
            PinCapability::UsbVbus => Some("VBUS"),
            PinCapability::UsbCc => Some("CC"),
            PinCapability::Reset => Some("RESET"),
            PinCapability::BootMode => Some("BOOT"),
            PinCapability::ClockInput => Some("CLK"),
            PinCapability::ClockOutput => Some("CLKO"),
            PinCapability::RfFeed => Some("RF"),
            _ => None,
        })?;
        Some(name.to_string())
    };
    for ep in &net.endpoints {
        let Some(component) = board.component(ep.component) else {
            continue;
        };
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if !active_kinds.contains(&part.kind.as_str()) {
            continue;
        }
        let Some(pin) = part.pins.get(ep.pin.0 as usize) else {
            continue;
        };
        if let Some(sem) = semantic(&pin.capabilities) {
            return Some((sem, ep.component));
        }
        return Some((pin.name.to_ascii_uppercase(), ep.component));
    }
    // Fallback: any endpoint's pin name.
    let ep = net.endpoints.first()?;
    let component = board.component(ep.component)?;
    let part = component.part.as_ref()?;
    let pin = part.pins.get(ep.pin.0 as usize)?;
    Some((pin.name.to_ascii_uppercase(), ep.component))
}

/// The full `<bus>.<member>` name when `net_name` is a member of a
/// declared bus, else `None`. Mirrors [`declared_rail_name`]: a
/// declared name beats the pin-derived heuristic so the schematic
/// label matches the bus the design actually declared.
fn declared_bus_member_label(board: &Board, net_name: &str) -> Option<String> {
    for bus in &board.buses {
        let Some(member) = net_name
            .strip_prefix(bus.name.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
        else {
            continue;
        };
        if bus.members.iter().any(|m| m == member) {
            return Some(net_name.to_string());
        }
    }
    None
}

/// Refdes of the component that supplied `expected` as `net`'s label
/// token, or `None` when the string cannot be re-derived (the
/// caller-side `NET_<id>` fallback, or a net with no resolvable
/// endpoint). Re-deriving through [`pick_net_label_with_source`]
/// keeps this consistent with how the string was produced in the
/// first place.
fn label_supplier_refdes(board: &Board, net: NetId, expected: &str) -> Option<String> {
    let ir_net = board.net(net)?;
    let (text, component) = pick_net_label_with_source(board, ir_net)?;
    if text != expected {
        return None;
    }
    Some(board.component(component)?.refdes.clone())
}

/// Post-classification uniqueness pass over rendered signal-net
/// labels (finding C1).
///
/// The KiCad exporter treats same-named local labels on one sheet as
/// electrically connected, so two distinct nets rendering the same
/// string (`I2C1.SDA` and `I2C2.SDA` both picking `SDA`, or two
/// sensors whose interrupt pins are both named `INT`) export as one
/// silently shorted net. This pass renames colliding strings so that,
/// after it runs, no two distinct [`NetId`]s share a rendered label:
///
/// - A string used by only one net is left byte-identical — boards
///   without collisions keep exactly the labels they had before.
/// - Within a collision group the bare token stays on the net whose
///   token-supplying endpoint carries the lexicographically lowest
///   refdes; every other member gains a `_<REFDES>` suffix
///   (`SDA_U4`). Ties — one component supplying several colliding
///   nets, e.g. an MCU with two I²C peripherals — break to the lowest
///   [`NetId`] ordinal, which keeps the bare-token winner stable even
///   then.
/// - Nets whose string has no identifiable supplier (the
///   `NET_<id>` fallback) get a `_N<ordinal>` suffix instead.
/// - Every decision iterates nets in ascending [`NetId`] ordinal and
///   renames are checked against all surviving strings, so results
///   never depend on hash iteration order and can never land on an
///   unrelated net's label; any residual clash grows trailing
///   underscores until the string is free.
///
/// # Maintenance
///
/// This pass must run wherever labels are produced. Today those are
/// [`route_and_label`] (which covers [`classify_net_labels`],
/// [`layout_with_placer`] and every `apply_op` mutation that reroutes)
/// and the `ReplaceWireWithLabel` op in `ops.rs`. If a future pass
/// pushes [`NetLabel`]s anywhere else, it bypasses this guarantee —
/// route it through here too.
pub(crate) fn uniquify_net_labels(board: &Board, labels: &mut [NetLabel]) {
    // Distinct nets, ascending ordinal.
    let mut nets: Vec<NetId> = labels.iter().map(|l| l.net).collect();
    nets.sort_unstable();
    nets.dedup();
    if nets.len() < 2 {
        return;
    }

    // One rendered string per net: producers stamp every endpoint of
    // a net with the same text.
    let mut text_of: std::collections::HashMap<NetId, String> = std::collections::HashMap::new();
    for label in labels.iter() {
        text_of
            .entry(label.net)
            .or_insert_with(|| label.label.clone());
    }

    // Group nets by their current string; a group holding ≥2 distinct
    // nets is exactly the KiCad short-circuit hazard.
    let mut by_text: std::collections::HashMap<String, Vec<NetId>> =
        std::collections::HashMap::new();
    for net in &nets {
        by_text.entry(text_of[net].clone()).or_default().push(*net);
    }

    let mut collisions: Vec<(String, Vec<NetId>)> = by_text
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .collect();
    if collisions.is_empty() {
        return;
    }
    // Member vectors hold ascending ordinals of disjoint sets, so
    // lexicographic comparison orders groups by smallest member.
    collisions.sort_by(|a, b| a.1.cmp(&b.1));

    // Final text per net, seeded unchanged; every current string
    // reserves its slot (each has at least its keeper) so suffixed
    // renames can never land on some unrelated net's label.
    let mut final_text = text_of.clone();
    let mut used: std::collections::HashSet<String> = text_of.values().cloned().collect();

    for (text, members) in &collisions {
        let supplier: std::collections::HashMap<NetId, Option<String>> = members
            .iter()
            .map(|&net| (net, label_supplier_refdes(board, net, text)))
            .collect();
        let mut ranked: Vec<&NetId> = members.iter().collect();
        ranked.sort_by(|&a, &b| {
            supplier[a]
                .as_ref()
                .cmp(&supplier[b].as_ref())
                .then(a.cmp(b))
        });
        let winner = *ranked[0];
        for &net in members.iter().filter(|&&n| n != winner) {
            let mut candidate = match supplier[&net].as_ref() {
                Some(refdes) => format!("{text}_{refdes}"),
                None => format!("{text}_N{}", net.0),
            };
            while !used.insert(candidate.clone()) {
                candidate.push('_');
            }
            final_text.insert(net, candidate);
        }
    }

    for label in labels.iter_mut() {
        if let Some(text) = final_text.get(&label.net) {
            label.label.clone_from(text);
        }
    }
}

/// Power-flow layer for a cluster's anchor.
///
/// - **Layer 0** — board edges / power sources: connectors (USB-C,
///   battery, header) and any component with `power_output` and no
///   `power_input`. From the schematic's POV these are where power
///   and signals enter the board.
/// - **Layer 1** — converters: components with both `power_input`
///   and `power_output` (regulators, load switches, chargers).
/// - **Layer 2** — active sinks (MCU / ic / sensor / memory / opamp /
///   secure_element).
/// - **Layer 3** — passives and everything else.
fn layer_for(component: &synth_ir::Component) -> u32 {
    use synth_registry::ElectricalType;
    let Some(part) = component.part.as_ref() else {
        return 3;
    };
    // Connectors are always at the top — they're the board edges
    // even when their `vbus`-style pins are marked `power_input`
    // (the connector receives voltage from the host, but from the
    // schematic's reading-direction POV, that's where power
    // enters).
    if part.kind == "connector" {
        return 0;
    }
    let has_power_out = part
        .pins
        .iter()
        .any(|p| matches!(p.electrical_type, ElectricalType::PowerOutput));
    let has_power_in = part
        .pins
        .iter()
        .any(|p| matches!(p.electrical_type, ElectricalType::PowerInput));
    if has_power_out && !has_power_in {
        0
    } else if has_power_out && has_power_in {
        1
    } else if matches!(
        part.kind.as_str(),
        "mcu" | "ic" | "sensor" | "memory" | "opamp" | "secure_element"
    ) {
        2
    } else {
        3
    }
}

/// Sheet size for a content bounding box of `w × h` millimetres
/// (already including page margins).
///
/// Paper-size selection for the placed content.
///
/// Default stays A4 by product decision: designs should read as one
/// consistent sheet size rather than silently growing. The one
/// exception is content that genuinely exceeds A4 landscape —
/// previously the declared page stayed A4 while symbols rendered
/// past the page edge (the canonical `sensor_logger` demo overflowed
/// to x≈565 mm against 297 mm). For that case we escalate to the
/// smallest ISO size whose landscape dims contain `(w, h)`, ceiling
/// at A2; beyond A2 the declared size stops growing and the
/// multi-sheet split (§P26) takes over. `w`/`h` are the
/// caller-computed content bounds including page margins.
fn sheet_size_for(w: f64, h: f64) -> SheetSize {
    const SIZES: [(SheetSize, f64, f64); 3] = [
        (SheetSize::A4, 297.0, 210.0),
        (SheetSize::A3, 420.0, 297.0),
        (SheetSize::A2, 594.0, 420.0),
    ];
    for (size, max_w, max_h) in SIZES {
        if w <= max_w && h <= max_h {
            return size;
        }
    }
    SheetSize::A2
}

/// Smallest standard sheet fitting the given content bounds, in mm
/// page coordinates. Used by the §P26 multi-sheet exporter to size
/// the root sheet around placed sheet instances.
pub fn fit_sheet_size(min_x: f64, max_x: f64, min_y: f64, max_y: f64) -> SheetSize {
    let (need_w, need_h) = sheet_needs(min_x, max_x, min_y, max_y);
    sheet_size_for(need_w, need_h)
}

/// A4 page width (mm) — the floor of the standard sheet ladder, and the
/// reference below which a custom page becomes worthwhile.
const A4_W: f64 = 297.0;
/// A4 page height (mm).
const A4_H: f64 = 210.0;
/// Custom sheet dimensions are rounded up to this quantum (mm), so the page
/// is a clean number rather than a raw content measurement.
const CUSTOM_SHEET_QUANTUM_MM: f64 = 5.0;
/// A custom page is only chosen when it saves at least this fraction of A4 in
/// at least one axis; a marginal saving is not worth leaving the standard
/// ladder and its title-block geometry.
const CUSTOM_SHEET_MIN_SAVING: f64 = 0.7;
/// Inset (mm) of KiCad's default page frame from the paper edge. The
/// title block is drawn inside this frame, so it bounds how small a
/// custom page can be.
const FRAME_INSET: f64 = 10.0;
/// Width (mm) of KiCad's default title block (`(rect (start 110 34) (end 2
/// 2))`, frame-relative). A custom page narrower than this plus the frame
/// would clip the title block.
const TITLE_BLOCK_W: f64 = 110.0;
/// Glyph height (mm) of the exporter's Reference/Value fields.
const FIELD_TEXT_SIZE: f64 = 1.27;

/// Smallest sheet that fits content of the given bounds once it is
/// re-centred by [`centre_on_sheet`], including a [`SheetSize::Custom`]
/// page below A4.
///
/// [`fit_sheet_size`] is bounded below by A4 because the standard ladder has
/// nothing smaller, so a three-part design on A4 is always mostly blank. A
/// human drawing three parts uses a small sheet. This sizes the page from the
/// content's *extent* (not its absolute position, since the caller centres
/// it): a margin on every side plus the title-block band, and never narrower
/// than the title block itself. A rounded custom page is returned when that
/// needs materially less than A4; otherwise the standard ladder.
pub fn fit_sheet_size_any(min_x: f64, max_x: f64, min_y: f64, max_y: f64) -> SheetSize {
    let need_w =
        ((max_x - min_x).max(0.0) + 2.0 * PAGE_MARGIN).max(TITLE_BLOCK_W + 2.0 * FRAME_INSET);
    let need_h = (max_y - min_y).max(0.0) + 2.0 * PAGE_MARGIN + TITLE_BLOCK_H;
    let standard = sheet_size_for(need_w, need_h);
    let (std_w, std_h) = standard.dims_mm();
    // Only shrink below A4 when the standard fit is already A4 and the
    // content needs clearly less — a marginal saving is not worth leaving
    // the standard ladder.
    if std_w > A4_W || std_h > A4_H {
        return standard;
    }
    let w = (need_w / CUSTOM_SHEET_QUANTUM_MM).ceil() * CUSTOM_SHEET_QUANTUM_MM;
    let h = (need_h / CUSTOM_SHEET_QUANTUM_MM).ceil() * CUSTOM_SHEET_QUANTUM_MM;
    if w >= A4_W * CUSTOM_SHEET_MIN_SAVING || h >= A4_H * CUSTOM_SHEET_MIN_SAVING {
        return standard;
    }
    SheetSize::Custom {
        width_mm: w,
        height_mm: h,
    }
}

/// [`content_bounds`] widened to cover each component's Reference and
/// Value fields, which the exporter places around the body after layout
/// and so `content_bounds` cannot see.
///
/// Page fitting and centring use this rather than the bare bounds: a sheet
/// sized to bodies and wires alone puts the refdes/value text of the
/// outermost parts on the margin or the frame. The field extent is an
/// estimate (the same stroke-font advance [`resolve_text_overlaps`] uses),
/// taken on the conservative side — a field may sit beside a rotated body
/// or above/below an upright one.
pub fn drawing_bounds(board: &Board, layout: &Layout) -> Option<(f64, f64, f64, f64)> {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = content_bounds(board, layout)?;
    for placement in &layout.components {
        let Some(comp) = board.component(placement.id) else {
            continue;
        };
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = comp
            .part
            .as_ref()
            .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
        let value = comp.value.as_deref().unwrap_or("(no value)");
        let field_w = text_run_width(&comp.refdes, FIELD_TEXT_SIZE)
            .max(text_run_width(value, FIELD_TEXT_SIZE));
        // Upright parts centre their fields over the body; rotated parts
        // put them beside it. Half a field past the body edge covers both.
        let reach = bw / 2.0 + field_w / 2.0;
        min_x = min_x.min(cx - reach);
        max_x = max_x.max(cx + reach);
        min_y = min_y.min(cy - bh / 2.0 - TEXT_MARGIN_Y);
        max_y = max_y.max(cy + bh / 2.0 + TEXT_MARGIN_Y);
    }
    Some((min_x, max_x, min_y, max_y))
}

/// Translate everything on the sheet so the drawing sits centred in the
/// page area above the title-block band.
///
/// The auto-layout packs content against the top-left page margin, which
/// is right for a page sized to fit it from the origin but leaves a
/// shrunken page (see [`fit_sheet_size_any`]) with all its slack on the
/// right and bottom — the drawing hugs the frame in one corner. Page
/// coordinates are absolute, so centring means moving the content, not
/// the frame. The shift is snapped to the 2.54 mm pin grid so wires stay
/// on grid, and applied to every geometric field (bodies, wires,
/// junctions, text, group boxes); labels and power flags are anchored to
/// pins and follow their component.
///
/// Never shifts content past the top/left margin: a drawing larger than
/// the page area stays anchored at the margin rather than being pushed
/// off the sheet.
pub fn centre_on_sheet(board: &Board, layout: &mut Layout) {
    let Some((min_x, max_x, min_y, max_y)) = drawing_bounds(board, layout) else {
        return;
    };
    let (sheet_w, sheet_h) = layout.sheet_size.dims_mm();
    let area_h = sheet_h - TITLE_BLOCK_H;
    let target_x = ((sheet_w - (max_x - min_x)) / 2.0).max(PAGE_MARGIN);
    let target_y = ((area_h - (max_y - min_y)) / 2.0).max(PAGE_MARGIN);
    let dx = snap_grid(target_x - min_x);
    let dy = snap_grid(target_y - min_y);
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let shift = |p: &mut (f64, f64)| {
        p.0 += dx;
        p.1 += dy;
    };
    for placement in &mut layout.components {
        shift(&mut placement.center_mm);
    }
    for wire in &mut layout.wires {
        wire.points.iter_mut().for_each(shift);
        wire.junctions.iter_mut().for_each(shift);
    }
    layout.junctions.iter_mut().for_each(shift);
    for text in &mut layout.annotations {
        shift(&mut text.at_mm);
    }
    for group in &mut layout.group_boxes {
        shift(&mut group.min_mm);
        shift(&mut group.max_mm);
    }
}

// ----- Human-like schematic alignment helpers --------------------------------

fn classify_ic_pin_layout(pin: &synth_registry::Pin) -> PinSide {
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
    // signal) stay left. Kept in lockstep with
    // `route::classify_ic_pin` and
    // `synth_kicad::symbol_lib::classify_ic_pin` so preview sizing,
    // wire terminals and drawn pins always agree.
    if pin.electrical_type == ElectricalType::Output {
        return PinSide::Right;
    }
    PinSide::Left
}

fn is_two_pin_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "resistor"
            | "capacitor"
            | "diode"
            | "led"
            | "ferrite_bead"
            | "zener_diode"
            | "crystal"
            | "resonator"
    )
}

/// Offset of a pin's terminal from its component's centre, plus
/// the side of the body the pin sits on.
///
/// Prefers the **stock KiCad symbol's** real geometry when the part
/// has a `kicad_symbol` mapping: the synthesized-rectangle model
/// (capability → side heuristic) diverges from stock reality — e.g.
/// the STM32F103C8Tx symbol carries NRST on the *left*, while the
/// heuristic says `Reset → Right`. Aligning cluster members to a
/// phantom pin position produces wires that wrap around the body.
///
/// Coordinate conversions: the loader returns symbol-local coords
/// (y-up); layout is y-down, so `py` is negated. KiCad's pin
/// `angle` points *inward* (terminal → body), so the outward
/// direction is `angle + 180°`; combined with the y-flip that gives
/// `outward = (-cos θ, +sin θ)` in layout space.
fn compute_anchor_pin_offset(part: &synth_registry::Part, pin_idx: usize) -> (f64, f64, PinSide) {
    if let Some(lib_id) = part.kicad_symbol.as_deref() {
        if let Some(pin_map) = kicad_lib_loader::pin_positions(lib_id) {
            if let Some(pin) = part.pins.get(pin_idx) {
                if let Some(&(px, py, angle)) = pin_map
                    .get(&pin.number.0)
                    .or_else(|| pin_map.get(&pin.name.to_lowercase()))
                {
                    let rad = angle.to_radians();
                    let (ox, oy) = (-rad.cos(), rad.sin());
                    let side = if ox.abs() >= oy.abs() {
                        if ox > 0.0 {
                            PinSide::Right
                        } else {
                            PinSide::Left
                        }
                    } else if oy > 0.0 {
                        PinSide::Bottom
                    } else {
                        PinSide::Top
                    };
                    // A multi-unit symbol draws its units as a stack;
                    // the pin belongs to unit `u`, so its offset is the
                    // unit-local position shifted by that unit's slot.
                    let (ux, uy) = kicad_lib_loader::pin_unit_offset(lib_id, &pin.number.0);
                    return (px + ux, -py + uy, side);
                }
            }
        }
    }
    compute_anchor_pin_offset_synthesized(part, pin_idx)
}

fn compute_anchor_pin_offset_synthesized(
    part: &synth_registry::Part,
    pin_idx: usize,
) -> (f64, f64, PinSide) {
    let pin_count = part.pins.len();
    if pin_count == 2 && is_two_pin_symbol_kind(&part.kind) {
        let is_vertical = part.kind == "led" || part.id.as_str().starts_with("led_");
        match (is_vertical, pin_idx) {
            (true, 0) => return (0.0, -6.54, PinSide::Top),
            (true, _) => return (0.0, 6.54, PinSide::Bottom),
            (false, 0) => return (-6.54, 0.0, PinSide::Left),
            (false, _) => return (6.54, 0.0, PinSide::Right),
        }
    }

    let sides: Vec<PinSide> = part.pins.iter().map(classify_ic_pin_layout).collect();
    let top_n = sides.iter().filter(|s| **s == PinSide::Top).count();
    let bottom_n = sides.iter().filter(|s| **s == PinSide::Bottom).count();
    let left_n = sides.iter().filter(|s| **s == PinSide::Left).count();
    let right_n = sides.iter().filter(|s| **s == PinSide::Right).count();

    let horiz_max = top_n.max(bottom_n).max(2);
    let vert_max = left_n.max(right_n).max(2);
    let body_w = ((horiz_max as f64) * 2.54 + 2.0 * 2.54).max(7.62 * 2.0);
    let body_h = ((vert_max as f64) * 2.54 + 2.0 * 2.54).max(10.16);

    let bx = -body_w / 2.0;
    let by = -body_h / 2.0;

    let mut top_idx = 0_usize;
    let mut bottom_idx = 0_usize;
    let mut left_idx = 0_usize;
    let mut right_idx = 0_usize;

    for (idx, side) in sides.iter().enumerate() {
        let (x, y) = match side {
            PinSide::Top => {
                let rx = bx + 2.54 + 2.54 * (top_idx as f64);
                let ry = by - 2.54;
                top_idx += 1;
                (rx, ry)
            }
            PinSide::Bottom => {
                let rx = bx + 2.54 + 2.54 * (bottom_idx as f64);
                let ry = by + body_h + 2.54;
                bottom_idx += 1;
                (rx, ry)
            }
            PinSide::Left => {
                let rx = bx - 2.54;
                let ry = by + 2.54 + 2.54 * (left_idx as f64);
                left_idx += 1;
                (rx, ry)
            }
            PinSide::Right => {
                let rx = bx + body_w + 2.54;
                let ry = by + 2.54 + 2.54 * (right_idx as f64);
                right_idx += 1;
                (rx, ry)
            }
        };
        if idx == pin_idx {
            return (x, y, *side);
        }
    }
    (0.0, 0.0, PinSide::Left)
}

fn find_connecting_active_pin(
    board: &Board,
    anchor_id: ComponentId,
    member_id: ComponentId,
) -> Option<usize> {
    use synth_registry::ElectricalType;
    let mut best_pin_idx: Option<usize> = None;
    for net in &board.nets {
        let has_anchor = net.endpoints.iter().any(|ep| ep.component == anchor_id);
        let has_member = net.endpoints.iter().any(|ep| ep.component == member_id);
        if has_anchor && has_member {
            // Find the pin on the anchor. Skip power pins (power
            // input/output) — a member often shares BOTH a signal
            // net and a power rail with the anchor (e.g. a reset
            // pull-up ties to NRST on one net and to +3V3 on
            // another), and aligning to the power pin would place
            // the member beside VDD instead of beside NRST.
            for ep in &net.endpoints {
                if ep.component == anchor_id {
                    let pin_idx = ep.pin.0 as usize;
                    if let Some(component) = board.component(anchor_id) {
                        if let Some(part) = component.part.as_ref() {
                            if let Some(pin) = part.pins.get(pin_idx) {
                                if matches!(
                                    pin.electrical_type,
                                    ElectricalType::PowerInput | ElectricalType::PowerOutput
                                ) {
                                    continue;
                                }
                                let side = classify_ic_pin_layout(pin);
                                if side == PinSide::Bottom {
                                    best_pin_idx = Some(pin_idx);
                                } else {
                                    return Some(pin_idx);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    best_pin_idx
}

#[cfg(test)]
mod barycenter_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};

    use super::*;

    fn component(id: u32, refdes: &str) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: "test".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board_with(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn singleton_cluster(anchor: u32) -> Cluster {
        Cluster {
            anchor: ComponentId(anchor),
            kind: ClusterKind::Singleton,
            anchor_vertical: false,
            members: Vec::new(),
        }
    }

    #[test]
    fn adjacency_sums_strong_weights_for_distinct_shared_nets() {
        // Cluster 0 (component 0) and cluster 1 (component 1) share
        // two separate nets, each a strong functional signal (the test
        // components have no parts, so no power pins -> strong), so
        // the strength-weighted sum is 2 × STRONG_NET_WEIGHT. Cluster
        // 2 (component 2) shares nothing with either.
        let board = board_with(
            vec![component(0, "A"), component(1, "B"), component(2, "C")],
            vec![
                net(0, "n0", &[(0, 0), (1, 0)]),
                net(1, "n1", &[(0, 1), (1, 1)]),
            ],
        );
        let clusters = vec![
            singleton_cluster(0),
            singleton_cluster(1),
            singleton_cluster(2),
        ];
        let adjacency = build_cluster_adjacency(&board, &clusters);
        assert_eq!(adjacency[0].get(&1), Some(&(2 * STRONG_NET_WEIGHT)));
        assert_eq!(adjacency[1].get(&0), Some(&(2 * STRONG_NET_WEIGHT)));
        assert!(adjacency[2].is_empty());
    }

    #[test]
    fn barycenter_pass_uncrosses_bottom_row_against_top_row() {
        // Top row (already placed): A (cluster idx 0) at column 0,
        // B (cluster idx 1) at column 1. Bottom row, naive
        // anchor-id order: X (idx 2), Y (idx 3, no connectivity),
        // Z (idx 4). X shares a net with B (col 1); Z shares a net
        // with A (col 0). Under the naive order [X, Y, Z] the wires
        // X-B and Z-A would cross; barycenter ordering should
        // produce [Z, X, Y] so Z sits under A and X sits under B.
        let board = board_with(
            vec![
                component(0, "A"),
                component(1, "B"),
                component(2, "X"),
                component(3, "Y"),
                component(4, "Z"),
            ],
            vec![
                net(0, "n_xb", &[(1, 0), (2, 0)]), // B <-> X
                net(1, "n_za", &[(0, 0), (4, 0)]), // A <-> Z
            ],
        );
        let clusters = vec![
            singleton_cluster(0),
            singleton_cluster(1),
            singleton_cluster(2),
            singleton_cluster(3),
            singleton_cluster(4),
        ];
        let adjacency = build_cluster_adjacency(&board, &clusters);
        let fallback_key: Vec<(u32, u32, u32)> =
            vec![(0, 0, 0), (0, 0, 1), (0, 1, 2), (0, 1, 3), (0, 1, 4)];

        let mut rows = vec![vec![0usize, 1usize], vec![2usize, 3usize, 4usize]];
        barycenter_order_rows(&mut rows, &adjacency, &fallback_key);

        assert_eq!(
            rows[0],
            vec![0, 1],
            "top row has no row above it on the first (downward) sweep, so it stays put"
        );
        assert_eq!(
            rows[1],
            vec![4, 2, 3],
            "bottom row reorders to Z, X, Y so its wires align under A/B"
        );
    }

    #[test]
    fn barycenter_pass_falls_back_to_anchor_id_order_without_adjacency() {
        // No nets at all: nothing is adjacent to anything, so the
        // only way to end up in a deterministic order is the
        // (layer, anchor id) fallback key.
        let board = board_with(
            vec![component(0, "A"), component(1, "X"), component(2, "Y")],
            vec![],
        );
        let clusters = vec![
            singleton_cluster(0),
            singleton_cluster(1),
            singleton_cluster(2),
        ];
        let adjacency = build_cluster_adjacency(&board, &clusters);
        let fallback_key: Vec<(u32, u32, u32)> = vec![(0, 0, 0), (0, 1, 1), (0, 1, 2)];

        // Bottom row deliberately out of anchor-id order.
        let mut rows = vec![vec![0usize], vec![2usize, 1usize]];
        barycenter_order_rows(&mut rows, &adjacency, &fallback_key);

        assert_eq!(
            rows[1],
            vec![1, 2],
            "no adjacency to the row above: falls back to (layer, anchor id) order, not the input order"
        );
    }
}

#[cfg(test)]
mod semantic_weights_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    use super::*;

    fn pin(name: &str, et: ElectricalType) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: et,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(kind: &str, pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId(kind.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, p: Part) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: p.kind.clone(),
            part: Some(p),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn singleton_cluster(anchor: u32) -> Cluster {
        Cluster {
            anchor: ComponentId(anchor),
            kind: ClusterKind::Singleton,
            anchor_vertical: false,
            members: Vec::new(),
        }
    }

    fn clusters(ids: &[u32]) -> Vec<Cluster> {
        ids.iter().copied().map(singleton_cluster).collect()
    }

    /// Two-resistor passive pair sharing a signal net (strong).
    fn strong_pair_board() -> Board {
        let r = part(
            "resistor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("p2", ElectricalType::Passive),
            ],
        );
        board(
            vec![
                component(0, "R0", r.clone()),
                component(1, "R1", r.clone()),
                component(2, "R2", r),
            ],
            vec![net(0, "sig", &[(0, 0), (1, 1)])],
        )
    }

    /// Two components sharing only a power rail (weak).
    fn weak_pair_board() -> Board {
        let r = part(
            "resistor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("p2", ElectricalType::Passive),
            ],
        );
        let vcc_resistor = part(
            "resistor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("vcc", ElectricalType::PowerInput),
            ],
        );
        board(
            vec![
                component(0, "R0", r.clone()),
                component(1, "R1", vcc_resistor),
                component(2, "R2", r),
            ],
            vec![net(0, "vcc", &[(0, 1), (1, 1)])],
        )
    }

    #[test]
    fn strong_signal_nets_classify_as_strong_and_dominate_weight() {
        // R0 and R1 share a passive signal net -> strong (16). A
        // power pin on the same net would flip it to weak.
        let board = strong_pair_board();
        let net = &board.nets[0];
        assert!(!net_is_weak(&board, net), "a passive signal net is strong");
        assert_eq!(STRONG_NET_WEIGHT, 16);
    }

    #[test]
    fn power_rail_nets_classify_as_weak() {
        // R0.vcc (PowerInput) and R1.vcc share a rail -> weak: the
        // rail connects them electrically but must not pull them
        // together on the sheet.
        let board = weak_pair_board();
        let net = &board.nets[0];
        assert!(net_is_weak(&board, net), "a power rail is weak");
        assert_eq!(WEAK_NET_WEIGHT, 1);
    }

    #[test]
    fn strong_pair_gets_higher_adjacency_weight_than_weak_pair() {
        // Two otherwise-identical boards differing only in whether the
        // shared net is a strong signal or a weak power rail. The
        // strong pair must weigh far more than the weak pair, so the
        // barycenter ordering (which reads these weights as
        // multipliers) pulls the strong pair together and leaves the
        // weak pair to fallback ordering.
        let strong = build_cluster_adjacency(&strong_pair_board(), &clusters(&[0, 1, 2]));
        let weak = build_cluster_adjacency(&weak_pair_board(), &clusters(&[0, 1, 2]));
        assert_eq!(strong[0].get(&1), Some(&STRONG_NET_WEIGHT));
        assert_eq!(weak[0].get(&1), Some(&WEAK_NET_WEIGHT));
        const {
            assert!(
                STRONG_NET_WEIGHT > WEAK_NET_WEIGHT,
                "strong edges must dominate ordering"
            );
        }
    }

    #[test]
    fn strong_edges_dominate_barycenter_ordering_over_weak_edges() {
        // Column 0: A(0) above B(1). Column 1, initial fallback order:
        // X(2) above Y(3). X ties to A via a weak rail AND to B via a
        // strong signal; Y is the mirror image (strong to A, weak to
        // B). With unweighted edges both barycenters land on 0.5 and
        // the tie falls back to anchor-id order [X, Y]. With
        // strong-vs-weak weighting the strong partner dominates each
        // cluster's barycenter: Y is pulled toward A (position 0),
        // X toward B (position 1), so the order inverts to [Y, X].
        let r = part(
            "resistor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("p2", ElectricalType::Passive),
            ],
        );
        let vcc_resistor = part(
            "resistor",
            vec![
                pin("p1", ElectricalType::Passive),
                pin("vcc", ElectricalType::PowerInput),
            ],
        );
        let board = board(
            vec![
                component(0, "A", vcc_resistor.clone()),
                component(1, "B", r.clone()),
                component(2, "X", vcc_resistor.clone()),
                component(3, "Y", vcc_resistor),
            ],
            vec![
                // X's weak rail to A and strong signal to B.
                net(0, "x_weak_a", &[(0, 1), (2, 1)]),
                net(1, "x_strong_b", &[(1, 0), (2, 0)]),
                // Y's strong signal to A and weak rail to B.
                net(2, "y_strong_a", &[(0, 0), (3, 0)]),
                net(3, "y_weak_b", &[(1, 1), (3, 1)]),
            ],
        );
        let clusters = clusters(&[0, 1, 2, 3]);
        let adjacency = build_cluster_adjacency(&board, &clusters);
        // Fallback keys put X(2) before Y(3) within column 1 — the
        // order the (unweighted) tie would fall back to.
        let fallback_key: Vec<(u32, u32, u32)> = vec![(0, 0, 0), (0, 0, 1), (0, 0, 2), (0, 0, 3)];
        let mut rows = vec![vec![0usize, 1usize], vec![2usize, 3usize]];
        barycenter_order_rows(&mut rows, &adjacency, &fallback_key);
        assert_eq!(
            rows[1],
            vec![3, 2],
            "the strong edge must win the ordering: Y (strong to A) sits above X (strong to B)"
        );
    }
}

#[cfg(test)]
mod brandes_koepf_tests {
    use super::*;

    /// Adjacency for the BK tests: `edge(a, b)` adds a symmetric,
    /// weight-1 connection between clusters `a` and `b`.
    fn adjacency_for(
        n: usize,
        edges: &[(usize, usize)],
    ) -> Vec<std::collections::HashMap<usize, u32>> {
        let mut adjacency: Vec<std::collections::HashMap<usize, u32>> =
            vec![std::collections::HashMap::new(); n];
        for &(a, b) in edges {
            *adjacency[a].entry(b).or_insert(0) += 1;
            *adjacency[b].entry(a).or_insert(0) += 1;
        }
        adjacency
    }

    #[test]
    fn aligned_nodes_share_vertical_coordinates() {
        // Two columns. Column 0: A(0) above B(1). Column 1: X(2) above
        // Y(3). A connects to X, B connects to Y — the textbook case
        // where Brandes–Köpf aligns A↔X and B↔Y vertically (straight
        // wires, no crossing).
        let layers = vec![vec![0usize, 1usize], vec![2usize, 3usize]];
        let adjacency = adjacency_for(4, &[(0, 2), (1, 3)]);
        let y = bk_y_coordinates(&layers, &adjacency);

        // Aligned pairs share a coordinate.
        assert!(
            (y[0] - y[2]).abs() < 1e-9,
            "A and X must align: y[0]={} y[2]={}",
            y[0],
            y[2]
        );
        assert!(
            (y[1] - y[3]).abs() < 1e-9,
            "B and Y must align: y[1]={} y[3]={}",
            y[1],
            y[3]
        );
        // Within each column the barycenter order is preserved and
        // strictly increasing.
        assert!(y[0] < y[1], "A must sit above B in column 0");
        assert!(y[2] < y[3], "X must sit above Y in column 1");
    }

    #[test]
    fn within_column_order_is_never_inverted_even_without_edges() {
        // No edges at all: BK must still keep a two-column graph
        // non-overlapping and in order (the per-column clamp).
        let layers = vec![vec![0usize, 1usize, 2usize], vec![3usize]];
        let adjacency = adjacency_for(4, &[]);
        let y = bk_y_coordinates(&layers, &adjacency);
        assert!(y[0] < y[1], "column 0 must stay in order");
        assert!(y[1] < y[2], "column 0 must stay in order");
        assert!(y[3].is_finite());
    }

    #[test]
    fn balanced_two_column_chain_keeps_alignment_and_order() {
        // A small chain where every node has one cross-column neighbour
        // — exercises all four sweeps plus the balance.
        let layers = vec![vec![0usize, 1usize], vec![2usize, 3usize]];
        let adjacency = adjacency_for(4, &[(0, 3), (1, 2)]);
        let y = bk_y_coordinates(&layers, &adjacency);
        for &v in &[0usize, 1, 2, 3] {
            assert!(y[v].is_finite(), "cluster {v} must have a finite y");
        }
        assert!(y[0] < y[1], "column 0 order preserved");
        assert!(y[2] < y[3], "column 1 order preserved");
    }
}

#[cfg(test)]
mod soft_pin_swap_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{
        ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinCapability, PinNumber,
    };

    use super::*;

    fn pin(name: &str, et: ElectricalType, caps: &[PinCapability]) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: et,
            capabilities: caps.to_vec(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(kind: &str, pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId(kind.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, p: Part) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: p.kind.clone(),
            part: Some(p),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn layout_at(positions: &[(u32, f64, f64)]) -> Layout {
        Layout {
            components: positions
                .iter()
                .map(|&(id, x, y)| ComponentPlacement {
                    id: ComponentId(id),
                    center_mm: (x, y),
                    rotation: Rotation::Zero,
                })
                .collect(),
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            hierarchical_labels: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    /// An MCU with two left-side, interchangeable GPIO pins (indices 2
    /// and 3), plus two passives F1 and F2 that hang off those GPIOs.
    fn swap_fixture() -> (Board, Layout) {
        let u1 = component(
            0,
            "U1",
            part(
                "mcu",
                vec![
                    pin("vcc", ElectricalType::PowerInput, &[]),
                    pin("gnd", ElectricalType::PowerInput, &[]),
                    pin(
                        "gpio0",
                        ElectricalType::Bidirectional,
                        &[PinCapability::Gpio],
                    ),
                    pin(
                        "gpio1",
                        ElectricalType::Bidirectional,
                        &[PinCapability::Gpio],
                    ),
                ],
            ),
        );
        let f1 = component(
            1,
            "F1",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let f2 = component(
            2,
            "F2",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        // gpio0 (the higher pin) ↔ F1, gpio1 (the lower pin) ↔ F2.
        let nets = vec![
            net(0, "gpio0_net", &[(0, 2), (1, 0)]),
            net(1, "gpio1_net", &[(0, 3), (2, 0)]),
        ];
        let board = board(vec![u1, f1, f2], nets);
        // F1 sits low (y=+8), F2 sits high (y=-10): the current
        // assignment crosses; swapping (F2 up to gpio0, F1 down to
        // gpio1) uncrosses.
        let layout = layout_at(&[(0, 0.0, 0.0), (1, -20.0, 8.0), (2, -20.0, -10.0)]);
        (board, layout)
    }

    #[test]
    #[allow(clippy::similar_names)] // pa_x/pa_y vs pb_x/pb_y read fine in context
    fn swap_removes_a_crossing() {
        let (board, layout) = swap_fixture();
        let swaps = soft_pin_swap_pass(&board, &layout);

        assert_eq!(
            swaps,
            vec![SoftPinSwap {
                component: ComponentId(0),
                pin_a: PinId(2),
                pin_b: PinId(3),
            }],
            "the pass must emit the one swap that uncrosses gpio0/gpio1"
        );

        // Independently confirm the crossing-count reduction.
        let (pa_x, pa_y, _) = compute_anchor_pin_offset(
            board
                .component(ComponentId(0))
                .unwrap()
                .part
                .as_ref()
                .unwrap(),
            2,
        );
        let (pb_x, pb_y, _) = compute_anchor_pin_offset(
            board
                .component(ComponentId(0))
                .unwrap()
                .part
                .as_ref()
                .unwrap(),
            3,
        );
        let pa = (pa_x, pa_y);
        let pb = (pb_x, pb_y);
        let current = segments_cross(pa, (-20.0, 8.0), pb, (-20.0, -10.0));
        let swapped = segments_cross(pa, (-20.0, -10.0), pb, (-20.0, 8.0));
        assert!(current, "current assignment must cross");
        assert!(!swapped, "swapped assignment must not cross");
    }

    #[test]
    fn no_swap_when_nets_already_uncrossed() {
        // Same board, but F1 is high and F2 is low: gpio0(high)→F1(high)
        // and gpio1(low)→F2(low) are parallel — nothing to uncross, so
        // the pass must stay silent.
        let (board, layout) = board_with_uncrossed_fars();
        let swaps = soft_pin_swap_pass(&board, &layout);
        assert!(
            swaps.is_empty(),
            "already-uncrossed nets must not be swapped: {swaps:?}"
        );
    }

    fn board_with_uncrossed_fars() -> (Board, Layout) {
        let (board, _) = swap_fixture();
        let layout = layout_at(&[(0, 0.0, 0.0), (1, -20.0, -10.0), (2, -20.0, 8.0)]);
        (board, layout)
    }
}

#[cfg(test)]
mod patterns_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{
        ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinCapability, PinNumber,
    };

    use super::*;

    fn pin(name: &str, et: ElectricalType, caps: &[PinCapability]) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: et,
            capabilities: caps.to_vec(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(kind: &str, pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId(kind.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, p: Part) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: p.kind.clone(),
            part: Some(p),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn find_cluster(clusters: &[Cluster], anchor: u32) -> &Cluster {
        clusters
            .iter()
            .find(|c| c.anchor.0 == anchor)
            .unwrap_or_else(|| panic!("no cluster anchored at component {anchor}"))
    }

    fn assert_member(cluster: &Cluster, id: u32, side: MemberSide) {
        assert!(
            cluster
                .members
                .iter()
                .any(|m| m.id.0 == id && m.side == side),
            "cluster {} must contain member {id} on {side:?}",
            cluster.anchor.0
        );
    }

    fn assert_layout_places_everything(board: &Board) {
        let layout = place_clusters(board, &build_clusters(board));
        assert_eq!(layout.components.len(), board.components.len());
        let mut ids: Vec<u32> = layout.components.iter().map(|p| p.id.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), board.components.len(), "no duplicates");
    }

    #[test]
    fn ldo_block_is_recognized_and_laid_out() {
        let u1 = component(
            0,
            "U1",
            part(
                "regulator",
                vec![
                    pin("gnd", ElectricalType::PowerInput, &[]),
                    pin("vout", ElectricalType::PowerOutput, &[]),
                    pin("vin", ElectricalType::PowerInput, &[]),
                ],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part(
                "capacitor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let c2 = component(
            2,
            "C2",
            part(
                "capacitor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![u1, c1, c2],
            vec![
                net(0, "vin", &[(0, 2), (1, 0)]),
                net(1, "gnd1", &[(0, 0), (1, 1)]),
                net(2, "vout", &[(0, 1), (2, 0)]),
                net(3, "gnd2", &[(0, 0), (2, 1)]),
            ],
        );
        let clusters = build_clusters(&b);
        let ldo = find_cluster(&clusters, 0);
        assert_member(ldo, 1, MemberSide::Below);
        assert_member(ldo, 2, MemberSide::Below);
        assert_layout_places_everything(&b);
    }

    #[test]
    fn i2c_bus_is_recognized_and_laid_out() {
        let u0 = component(
            0,
            "U0",
            part(
                "sensor",
                vec![
                    pin("vdd", ElectricalType::PowerInput, &[]),
                    pin("gnd", ElectricalType::PowerInput, &[]),
                    pin(
                        "sda",
                        ElectricalType::Bidirectional,
                        &[PinCapability::I2cSda],
                    ),
                    pin(
                        "scl",
                        ElectricalType::Bidirectional,
                        &[PinCapability::I2cScl],
                    ),
                ],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let r2 = component(
            2,
            "R2",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![u0, r1, r2],
            vec![
                net(0, "sda", &[(0, 2), (1, 0)]),
                net(1, "vdd_sda", &[(1, 1), (0, 0)]),
                net(2, "scl", &[(0, 3), (2, 0)]),
                net(3, "vdd_scl", &[(2, 1), (0, 0)]),
            ],
        );
        let clusters = build_clusters(&b);
        let bus = find_cluster(&clusters, 0);
        assert_member(bus, 1, MemberSide::Above);
        assert_member(bus, 2, MemberSide::Above);
        assert_layout_places_everything(&b);
    }

    #[test]
    fn crystal_is_recognized_and_laid_out() {
        let x0 = component(
            0,
            "X0",
            part(
                "crystal",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part(
                "capacitor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let c2 = component(
            2,
            "C2",
            part(
                "capacitor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![x0, c1, c2],
            vec![
                net(0, "x1", &[(0, 0), (1, 0)]),
                net(1, "x2", &[(0, 1), (2, 0)]),
                net(2, "gnd", &[(1, 1), (2, 1)]),
            ],
        );
        let clusters = build_clusters(&b);
        let x = find_cluster(&clusters, 0);
        assert_member(x, 1, MemberSide::Below);
        assert_member(x, 2, MemberSide::Below);
        assert_layout_places_everything(&b);
    }

    #[test]
    fn divider_is_recognized_and_laid_out() {
        let j0 = component(
            0,
            "J0",
            part(
                "connector",
                vec![
                    pin("p1", ElectricalType::PowerOutput, &[]),
                    pin("p2", ElectricalType::PowerInput, &[]),
                ],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let r2 = component(
            2,
            "R2",
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![j0, r1, r2],
            vec![
                net(0, "rail", &[(0, 0), (1, 0)]),
                net(1, "mid", &[(1, 1), (2, 0)]),
                net(2, "gnd", &[(0, 1), (2, 1)]),
            ],
        );
        let clusters = build_clusters(&b);
        let div = find_cluster(&clusters, 1);
        assert_member(div, 2, MemberSide::Below);
        assert_layout_places_everything(&b);
    }
}

#[cfg(test)]
mod text_width_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    use super::*;

    fn two_pin_part(id: &str, mpn: Option<&str>) -> Part {
        Part {
            id: PartId(id.to_string()),
            kind: "resistor".to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: mpn.map(str::to_string),
            lcsc_pn: None,
            provenance: None,
            pins: vec![
                RegPin {
                    name: "p1".to_string(),
                    number: PinNumber("1".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
                RegPin {
                    name: "p2".to_string(),
                    number: PinNumber("2".to_string()),
                    electrical_type: ElectricalType::Passive,
                    capabilities: Vec::new(),
                    required: false,
                    unit: None,
                    voltage_max_v: None,
                    voltage_min_v: None,
                    voltage_nominal_v: None,
                },
            ],
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(refdes: &str, value: Option<&str>, part: Part) -> Component {
        Component {
            id: ComponentId(0),
            refdes: refdes.to_string(),
            kind: part.kind.clone(),
            part: Some(part),
            value: value.map(str::to_string),
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn text_width(component: &Component) -> f64 {
        let val_text = component
            .value
            .as_deref()
            .or(component.part.as_ref().and_then(|p| p.mpn.as_deref()))
            .unwrap_or("(no value)");
        (val_text.len() as f64) * 1.27 * 0.85 + 2.54
    }

    #[test]
    fn value_precedence_prefers_custom_value_over_mpn_over_part_id() {
        let long = |s: &str| (s.len() as f64) * 1.27 * 0.85 + 2.54;

        // Custom value wins over everything.
        let c = component(
            "R1",
            Some("10k 0.1% THINFILM"),
            two_pin_part("resistor_10k", Some("AC0603FR-0710KL")),
        );
        let p = c.part.as_ref().unwrap();
        let half = text_inclusive_half_width(&c, p);
        assert!(half >= long("10k 0.1% THINFILM") / 2.0);

        // No custom value: MPN wins over the part id.
        let c = component(
            "R1",
            None,
            two_pin_part("resistor_10k", Some("AC0603FR-0710KL")),
        );
        let p = c.part.as_ref().unwrap();
        let half = text_inclusive_half_width(&c, p);
        assert!(half >= long("AC0603FR-0710KL") / 2.0);

        // Neither: falls back to the `(no value)` sentinel (Phase A1:
        // the part id must never render as a value).
        let c = component("R1", None, two_pin_part("resistor_10k", None));
        let p = c.part.as_ref().unwrap();
        let half = text_inclusive_half_width(&c, p);
        assert!(half >= long("(no value)") / 2.0);
    }

    #[test]
    fn long_value_widens_half_width_beyond_body_and_refdes() {
        // The regression shape: a long custom value must dominate
        // both the body half-width (7.62/2 for a 2-pin symbol... here
        // 15.24-wide fallback) and a short refdes.
        let c = component(
            "R1",
            Some("SOME VERY LONG CUSTOM DISPLAY VALUE"),
            two_pin_part("r", None),
        );
        let p = c.part.as_ref().unwrap();
        let half = text_inclusive_half_width(&c, p);
        assert!(half > 7.62, "body half-width must not dominate");
        assert!(half >= text_width(&c) / 2.0 - 1e-9);
    }
}

#[cfg(test)]
mod text_overlap_tests {
    use synth_diagnostics::Span;
    use synth_ir::Board;

    use super::*;

    fn empty_board() -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: Vec::new(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            netclasses: Vec::new(),
            buses: Vec::new(),
            modules: Vec::new(),
            variants: Vec::new(),
            source_span: Span::new(0, 0),
        }
    }

    fn empty_layout(annotations: Vec<TextAnnotation>, sheet_size: SheetSize) -> Layout {
        Layout {
            components: Vec::new(),
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            hierarchical_labels: Vec::new(),
            annotations,
            group_boxes: Vec::new(),
            sheet_size,
        }
    }

    fn run(text: &str, size_mm: f64, x: f64, y: f64) -> TextAnnotation {
        TextAnnotation {
            text: text.to_string(),
            at_mm: (x, y),
            size_mm,
            kind: TextKind::NoteLine,
        }
    }

    fn overlaps(layout: &Layout) -> bool {
        for (i, a) in layout.annotations.iter().enumerate() {
            let ra = text_run_rect(a);
            for b in layout.annotations.iter().skip(i + 1) {
                if rects_overlap(ra, text_run_rect(b)) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn overlapping_annotations_are_nudged_apart() {
        let board = empty_board();
        let mut layout = empty_layout(
            vec![
                run("J1 pinout", 2.0, 20.0, 100.0),
                run("J1 pinout", 2.0, 20.0, 100.0),
                run("vbus: VBUS", 1.27, 20.0, 104.0),
            ],
            SheetSize::A4,
        );
        resolve_text_overlaps(&board, &mut layout);
        assert_eq!(layout.annotations.len(), 3, "nothing dropped: {layout:?}");
        assert!(!overlaps(&layout), "runs must separate: {layout:?}");
    }

    #[test]
    fn titles_are_never_shrunk_or_dropped() {
        let board = empty_board();
        let mut layout = empty_layout(
            vec![
                run("Input", 2.0, 20.0, 100.0),
                run("Input", 2.0, 20.0, 100.0),
            ],
            SheetSize::A4,
        );
        resolve_text_overlaps(&board, &mut layout);
        assert_eq!(layout.annotations.len(), 2);
        assert!(
            layout.annotations.iter().all(|a| a.size_mm == 2.0),
            "titles keep their size: {layout:?}"
        );
    }

    #[test]
    fn unplaceable_line_is_dropped() {
        // A line run under a continuous wall of bodies: 16 nudges
        // plus one shrink step cannot clear it, so it is dropped
        // rather than left overlapping.
        let mut board = empty_board();
        board.components = vec![synth_ir::Component {
            id: ComponentId(0),
            refdes: "U1".to_string(),
            kind: "mcu".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }];
        let mut layout = empty_layout(vec![run("buried note", 1.27, 95.0, 100.0)], SheetSize::A4);
        // Fallback body (15 × 10) plus text margins covers ±11.35 mm
        // vertically; bodies every 12 mm form a continuous wall the
        // run cannot nudge past within its step budget.
        for y in [100.0, 112.0, 124.0, 136.0, 148.0] {
            layout.components.push(ComponentPlacement {
                id: ComponentId(0),
                center_mm: (100.0, y),
                rotation: Rotation::Zero,
            });
        }
        resolve_text_overlaps(&board, &mut layout);
        assert!(
            layout.annotations.is_empty(),
            "unplaceable line must drop: {layout:?}"
        );
    }

    #[test]
    fn compact_sheet_shrinks_to_smallest_fitting_sheet() {
        let board = empty_board();
        let mut layout = empty_layout(vec![run("tiny", 1.27, 20.0, 30.0)], SheetSize::A3);
        layout.components.push(ComponentPlacement {
            id: ComponentId(0),
            center_mm: (30.0, 30.0),
            rotation: Rotation::Zero,
        });
        compact_sheet_to_fit(&board, &mut layout);
        assert_eq!(layout.sheet_size, SheetSize::A4);
    }

    #[test]
    fn compact_sheet_never_shrinks_past_content() {
        let board = empty_board();
        let mut layout = empty_layout(vec![], SheetSize::A4);
        // Content near the A4 right edge: A4 must stay.
        layout.components.push(ComponentPlacement {
            id: ComponentId(0),
            center_mm: (280.0, 30.0),
            rotation: Rotation::Zero,
        });
        compact_sheet_to_fit(&board, &mut layout);
        assert_eq!(layout.sheet_size, SheetSize::A4);
    }
}

#[cfg(test)]
mod naming_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{
        ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinCapability, PinNumber,
    };

    use super::*;

    #[test]
    fn sidecar_override_reroutes_wires_to_final_positions() {
        // U1 —R(1)->— R1 chain; drag R1 far away via an override and
        // confirm every wire endpoint lands on a pin terminal of the
        // FINAL placements (no stale pre-drag coordinates survive).
        let p = part(
            "r",
            "resistor",
            vec![
                pin("1", ElectricalType::Passive, &[]),
                pin("2", ElectricalType::Passive, &[]),
            ],
        );
        let board = board(
            vec![
                component(
                    0,
                    "U1",
                    part(
                        "u",
                        "mcu",
                        vec![
                            pin(
                                "SIG",
                                ElectricalType::Bidirectional,
                                &[PinCapability::UartTx],
                            ),
                            pin("VDD", ElectricalType::PowerInput, &[]),
                            pin("GND", ElectricalType::GroundReference, &[]),
                        ],
                    ),
                ),
                component(1, "R1", p.clone()),
            ],
            vec![net(0, "sig", &[(0, 0), (1, 0)])],
        );
        let layout = layout_with_overrides(&board, &default_placer(), &|l: &mut Layout| {
            if let Some(pl) = l.components.iter_mut().find(|pl| pl.id == ComponentId(1)) {
                pl.center_mm = (250.0, 40.0);
            }
        });
        let placements: std::collections::HashMap<_, _> =
            layout.components.iter().map(|pl| (pl.id, pl)).collect();
        assert_eq!(
            placements.get(&ComponentId(1)).unwrap().center_mm,
            (250.0, 40.0),
            "override must be applied before routing"
        );
        for wire in &layout.wires {
            for &(x, y) in &wire.points {
                let on_some_terminal = layout.components.iter().any(|pl| {
                    matches!(
                        route::pin_terminal_xy(&board, pl.id, PinId(0), &placements),
                        Some((tx, ty, _, _)) if (tx - x).abs() < 1e-6 && (ty - y).abs() < 1e-6
                    ) || matches!(
                        route::pin_terminal_xy(&board, pl.id, PinId(1), &placements),
                        Some((tx, ty, _, _)) if (tx - x).abs() < 1e-6 && (ty - y).abs() < 1e-6
                    )
                });
                assert!(
                    on_some_terminal,
                    "wire endpoint ({x},{y}) touches no pin terminal of the                      overridden placement"
                );
            }
        }
    }

    #[test]
    fn declared_rail_names_override_pin_heuristics() {
        assert_eq!(
            declared_rail_name("AGND").as_deref(),
            Some("AGND"),
            "a declared analog-ground name must survive to export"
        );
        assert_eq!(declared_rail_name("+5V_USB").as_deref(), Some("+5V_USB"));
        // Generics and auto-generated names carry no identity.
        for generic in ["gnd", "GND", "ground", "vcc", "vdd", "net_7"] {
            assert!(
                declared_rail_name(generic).is_none(),
                "{generic} must fall through to the legacy derivation"
            );
        }
    }

    #[test]
    fn rail_label_sanitizer_keeps_kicad_safe_characters() {
        assert_eq!(sanitize_rail_label("3V3-RAIL"), Some("3V3-RAIL".into()));
        assert_eq!(
            sanitize_rail_label("rail (main)"),
            Some("rail__main_".into())
        );
        assert_eq!(sanitize_rail_label(""), None);
    }

    fn pin(name: &str, et: ElectricalType, caps: &[PinCapability]) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: et,
            capabilities: caps.to_vec(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(id: &str, kind: &str, pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId(id.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, p: Part) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: p.kind.clone(),
            part: Some(p),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn voltage_token_recognised_forms() {
        let cases: &[(&str, Option<&str>)] = &[
            ("ams1117_3v3", Some("+3V3")),
            ("AMS1117-3.3", Some("+3V3")),
            ("3v3", Some("+3V3")),
            ("5v", Some("+5V")),
            ("12v", Some("+12V")),
            ("3.3", Some("+3V3")),
            ("ldo 1v8 regulator", Some("+1V8")),
            // The digit run is consumed whole: `33` is one candidate,
            // never two single-digit ones.
            ("33v", Some("+33V")),
            // Model numbers without a v/dot suffix match nothing.
            ("1117", None),
            ("ne555", None),
            ("lm75", None),
            // Quirk pinned: a remembered dotted candidate loses to any
            // later v-form — even when what follows the dot was really
            // the fraction and the `v` belongs to it.
            ("1.8v", Some("+8V")),
            // A later v-style token outranks an earlier dotted one.
            ("3.3 or 5v", Some("+5V")),
        ];
        for (text, want) in cases {
            assert_eq!(voltage_token(text).as_deref(), *want, "input {text:?}");
        }
    }

    #[test]
    fn regulator_rail_label_prefers_id_then_mpn_then_description() {
        let from_id = part("ams1117_3v3", "regulator", vec![]);
        assert_eq!(regulator_rail_label(&from_id), Some("+3V3".to_string()));

        let mut from_mpn = part("ldo", "regulator", vec![]);
        from_mpn.mpn = Some("AMS1117-3.3".to_string());
        assert_eq!(regulator_rail_label(&from_mpn), Some("+3V3".to_string()));

        let mut from_desc = part("ldo", "regulator", vec![]);
        from_desc.description = Some("fixed 1.8 V LDO".to_string());
        assert_eq!(regulator_rail_label(&from_desc), Some("+1V8".to_string()));

        let none = part("ldo", "regulator", vec![]);
        assert_eq!(regulator_rail_label(&none), None);
    }

    #[test]
    fn ground_named_pin_beats_positive_input_on_same_net() {
        // Both pins are power inputs; the ground-named one wins the
        // whole net even though the positive input already captured a
        // rail label (`VIN`).
        let u1 = component(
            0,
            "U1",
            part(
                "mcu",
                "mcu",
                vec![pin("vin", ElectricalType::PowerInput, &[])],
            ),
        );
        let u2 = component(
            1,
            "U2",
            part(
                "sensor",
                "sensor",
                vec![pin("gnd", ElectricalType::PowerInput, &[])],
            ),
        );
        let b = board(vec![u1, u2], vec![net(0, "net_0", &[(0, 0), (1, 0)])]);
        let flags = classify_power_flags(&b);
        assert_eq!(flags.len(), 2);
        for flag in &flags {
            assert_eq!(flag.kind, PowerFlagKind::Gnd);
            assert_eq!(flag.label, "GND");
        }
    }

    #[test]
    fn ground_named_pin_beats_power_output_on_same_net() {
        // Quirk pinned: the GND branch runs before the
        // power-output branch, so even a driven rail touching a
        // ground-named pin renders as GND.
        let reg = component(
            0,
            "U1",
            part(
                "regulator",
                "regulator",
                vec![pin("vout", ElectricalType::PowerOutput, &[])],
            ),
        );
        let ic = component(
            1,
            "U2",
            part(
                "ic",
                "ic",
                vec![pin("gnd", ElectricalType::PowerInput, &[])],
            ),
        );
        let b = board(vec![reg, ic], vec![net(0, "net_0", &[(0, 0), (1, 0)])]);
        let flags = classify_power_flags(&b);
        assert_eq!(flags.len(), 2);
        for flag in &flags {
            assert_eq!(flag.kind, PowerFlagKind::Gnd);
            assert_eq!(flag.label, "GND");
        }
    }

    #[test]
    fn regulator_output_rail_is_named_after_the_part_voltage() {
        let reg = component(
            0,
            "U1",
            part(
                "ams1117_3v3",
                "regulator",
                vec![
                    pin("gnd", ElectricalType::PowerInput, &[]),
                    pin("vout", ElectricalType::PowerOutput, &[]),
                    pin("vin", ElectricalType::PowerInput, &[]),
                ],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part(
                "capacitor",
                "capacitor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![reg, c1],
            vec![
                net(0, "net_0", &[(0, 1), (1, 0)]),
                net(1, "net_1", &[(0, 2), (1, 1)]),
            ],
        );
        let flags = classify_power_flags(&b);
        // The regulator's output leg reads as +3V3, derived from the
        // part id's voltage token.
        let rail: Vec<_> = flags.iter().filter(|f| f.net == NetId(0)).collect();
        assert_eq!(rail.len(), 2);
        for flag in rail {
            assert_eq!(flag.kind, PowerFlagKind::Vcc);
            assert_eq!(flag.label, "+3V3");
        }
        // The bulk-cap return leg anchors a plain Vcc named after the
        // first non-ground input pin.
        let ret: Vec<_> = flags.iter().filter(|f| f.net == NetId(1)).collect();
        assert_eq!(ret.len(), 2);
        for flag in ret {
            assert_eq!(flag.kind, PowerFlagKind::Vcc);
            assert_eq!(flag.label, "VIN");
        }
    }

    #[test]
    fn single_endpoint_and_signal_nets_are_skipped() {
        let r1 = component(
            0,
            "R1",
            part(
                "resistor",
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let r2 = component(
            1,
            "R2",
            part(
                "resistor",
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive, &[]),
                    pin("p2", ElectricalType::Passive, &[]),
                ],
            ),
        );
        let b = board(
            vec![r1, r2],
            vec![net(0, "lone", &[(0, 0)]), net(1, "sig", &[(0, 1), (1, 1)])],
        );
        assert!(classify_power_flags(&b).is_empty());
    }
}

/// §21.1 documentation pass: group boxes, design notes, connector
/// pin legends.
#[cfg(test)]
mod documentation_tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, Note, PinId};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    use super::*;

    fn pin(name: &str) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: ElectricalType::Passive,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(kind: &str, pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId(kind.to_string()),
            kind: kind.to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, p: Part, group: Option<&str>) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: p.kind.clone(),
            part: Some(p),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: group.map(str::to_string),
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board_with_notes(components: Vec<Component>, nets: Vec<Net>, notes: Vec<Note>) -> Board {
        Board {
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes,
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn texts(layout: &Layout) -> Vec<&str> {
        layout.annotations.iter().map(|a| a.text.as_str()).collect()
    }

    #[test]
    fn group_box_frames_caption_and_parts() {
        let r = || part("resistor", vec![pin("p1"), pin("p2")]);
        let b = board_with_notes(
            vec![
                component(0, "R1", r(), Some("Input")),
                component(1, "R2", r(), Some("Input")),
                component(2, "R3", r(), None),
            ],
            vec![net(0, "sig", &[(0, 0), (1, 0), (2, 0)])],
            vec![],
        );
        let layout = layout(&b);
        assert_eq!(texts(&layout).iter().filter(|t| **t == "Input").count(), 1);
        assert_eq!(layout.group_boxes.len(), 1);
        let box_ = &layout.group_boxes[0];
        assert_eq!(box_.group, "Input");
        assert!(box_.min_mm.0 < box_.max_mm.0 && box_.min_mm.1 < box_.max_mm.1);
        // The box top clears the caption baseline above it.
        let caption_y = layout
            .annotations
            .iter()
            .find(|a| a.text == "Input")
            .expect("caption")
            .at_mm
            .1;
        assert!(box_.min_mm.1 < caption_y);
        // Every grouped part sits inside the box.
        for placement in &layout.components[0..2] {
            let (x, y) = placement.center_mm;
            assert!(x > box_.min_mm.0 && x < box_.max_mm.0, "{x}");
            assert!(y > box_.min_mm.1 && y < box_.max_mm.1, "{y}");
        }
    }

    #[test]
    fn ungrouped_board_has_no_boxes() {
        let b = board_with_notes(
            vec![component(
                0,
                "R1",
                part("resistor", vec![pin("p1"), pin("p2")]),
                None,
            )],
            vec![net(0, "sig", &[(0, 0)])],
            vec![],
        );
        let layout = layout(&b);
        assert!(layout.group_boxes.is_empty());
    }

    #[test]
    fn design_notes_render_below_content() {
        let b = board_with_notes(
            vec![component(
                0,
                "R1",
                part("resistor", vec![pin("p1"), pin("p2")]),
                None,
            )],
            vec![net(0, "sig", &[(0, 0)])],
            vec![Note {
                title: "Build".to_string(),
                lines: vec!["Assemble at JLCPCB.".to_string()],
                group: None,
                sheet: None,
                source_span: Span::new(0, 0),
            }],
        );
        let layout = layout(&b);
        let labels = texts(&layout);
        assert!(labels.contains(&"Build"));
        assert!(labels.contains(&"Assemble at JLCPCB."));
        let comp_bottom = layout.components[0].center_mm.1;
        let title_y = layout
            .annotations
            .iter()
            .find(|a| a.text == "Build")
            .expect("note title")
            .at_mm
            .1;
        assert!(title_y > comp_bottom);
    }

    #[test]
    fn group_notes_render_inside_their_box() {
        let r = || part("resistor", vec![pin("p1"), pin("p2")]);
        let b = board_with_notes(
            vec![
                component(0, "R1", r(), Some("Input")),
                component(1, "R2", r(), Some("Input")),
            ],
            vec![net(0, "sig", &[(0, 0), (1, 0)])],
            vec![Note {
                title: "Input notes".to_string(),
                lines: vec!["Keep leads short.".to_string()],
                group: Some("Input".to_string()),
                sheet: None,
                source_span: Span::new(0, 0),
            }],
        );
        let layout = layout(&b);
        let box_ = &layout.group_boxes[0];
        let title_y = layout
            .annotations
            .iter()
            .find(|a| a.text == "Input notes")
            .expect("group note title")
            .at_mm
            .1;
        let line_y = layout
            .annotations
            .iter()
            .find(|a| a.text == "Keep leads short.")
            .expect("group note line")
            .at_mm
            .1;
        // Phase B3: the note strip lives inside the box, and the box
        // grew to enclose it.
        assert!(title_y > box_.min_mm.1, "title inside box top: {title_y}");
        assert!(title_y < box_.max_mm.1, "title above box bottom: {title_y}");
        assert!(line_y < box_.max_mm.1, "line inside box: {line_y}");
    }

    #[test]
    fn connector_legend_lists_named_pins_when_opted_in() {
        // Phase A3: legends are opt-in and require ≥4 named pins.
        let j = component(
            0,
            "J1",
            part(
                "connector",
                vec![pin("p1"), pin("p2"), pin("p3"), pin("p4")],
            ),
            None,
        );
        let r = || part("resistor", vec![pin("p1"), pin("p2")]);
        let b = board_with_notes(
            vec![
                j,
                component(1, "R1", r(), None),
                component(2, "R2", r(), None),
            ],
            vec![
                net(0, "SIG", &[(0, 0), (1, 0)]),
                net(1, "CLK", &[(0, 1), (1, 1)]),
                net(2, "RST", &[(0, 2), (2, 0)]),
                net(3, "EN", &[(0, 3), (2, 1)]),
            ],
            vec![],
        );
        // Legends are off by default: no pinout block.
        let off = layout(&b);
        assert!(!texts(&off).iter().any(|t| t.contains("pinout")));
        // Opt in and the named pins render; NC lines never appear.
        let mut on = b.clone();
        on.legends = true;
        let layout = layout(&on);
        let labels = texts(&layout);
        assert!(labels.contains(&"J1 pinout"), "{labels:?}");
        assert!(labels.contains(&"p1: SIG"), "{labels:?}");
        assert!(!labels.iter().any(|t| t.contains("NC")), "{labels:?}");
        // Legend sits below the connector body.
        let j_center = layout.components[0].center_mm.1;
        let title_y = layout
            .annotations
            .iter()
            .find(|a| a.text == "J1 pinout")
            .expect("legend title")
            .at_mm
            .1;
        assert!(title_y > j_center);
    }

    #[test]
    fn non_connector_gets_no_legend() {
        let b = board_with_notes(
            vec![component(
                0,
                "R1",
                part("resistor", vec![pin("p1"), pin("p2")]),
                None,
            )],
            vec![net(0, "sig", &[(0, 0)])],
            vec![],
        );
        let layout = layout(&b);
        assert!(!texts(&layout).iter().any(|t| t.contains("pinout")));
    }
}
