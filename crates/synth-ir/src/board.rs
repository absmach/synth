// SPDX-License-Identifier: Apache-2.0

//! The canonical `Board` IR. Owned arena-indexed types with stable
//! ids that downstream stages (ERC, placement, routing, export) use
//! as the addressable model.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use synth_diagnostics::Span;
use synth_registry::Part;

use crate::modules::{BusBundle, ModuleDesc};
use crate::units::{Current, DielectricConstant, Impedance, Length, Voltage};

/// Stable identifier for a component within a single board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub u32);

/// Identifier for a pin within a specific component. Indexes into
/// `Component::part.pins`; valid only relative to a [`ComponentId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PinId(pub u32);

/// Stable identifier for a net within a single board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NetId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub name: String,
    pub layers: u32,
    pub manufacturer: Option<String>,
    /// Optional revision tag (`revision "A"`), carried into the
    /// schematic title block, which by convention displays the
    /// revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Design-authority company (`company "…"`), carried into the
    /// schematic title block's Company field. Distinct from
    /// `manufacturer` (who builds the board).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    /// Requested schematic page size (`schematic { paper = "A4" }`).
    ///
    /// `None` means the author did not ask, and the layout defaults to A4.
    /// This is the page's *floor*: the layout still enlarges the sheet when
    /// content does not fit, but never compacts below the requested size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic_paper: Option<SchematicPaper>,
    /// What to do when the content does not fit the requested page
    /// (`schematic { overflow = … }`). `None` means
    /// [`SchematicOverflow::Grow`], the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic_overflow: Option<SchematicOverflow>,
    pub components: Vec<Component>,
    pub nets: Vec<Net>,
    pub diff_pairs: Vec<DiffPair>,
    /// Free-text design notes (`notes "Title" { … }`), in source
    /// order. Rendered on the schematic as titled text blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Connector pin legends (`legends on|off`, default off): when
    /// true, board-edge connectors get a compact generated pinout
    /// legend on the schematic. Schematic-quality plan Phase A3 —
    /// the reference sheet carries a one-line prose note instead of
    /// a per-pin dump, so legends are opt-in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legends: bool,
    pub keepouts: Vec<Keepout>,
    /// Declared physical layer stack (`stackup { … }`). `None` when the
    /// design does not declare one; the exported board then keeps its fixed
    /// default thickness and carries no stackup section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stackup: Option<Stackup>,
    /// Declared routing-constraint classes (`netclass "PWR" { … }`).
    /// Nets join a class via `class "PWR"` on their `net`, `power`,
    /// or `connect` statement; the PCB exporter emits one KiCad
    /// `net_class` per declared class with its member nets.
    pub netclasses: Vec<NetClass>,
    /// Declared buses (`bus "I2C0" (sda, scl)`), in declaration order.
    /// Exported to KiCad as buses plus `bus_alias` entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<BusBundle>,
    /// Declared modules, kept for tooling/inspection. Instantiation is
    /// resolved during lowering; nothing downstream needs the bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<ModuleDesc>,
    /// Declared `group` regions, in first-declaration order, with their
    /// optional header attributes (Phase D1): display `title`, box
    /// `color`, and pinned `region`. Components reference these by name
    /// via [`Component::group`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// Declared design variants (`variant "lite" { dnp U3 }`), in
    /// declaration order. Exported to KiCad's native design variants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
    /// Fabrication processes the board declares it needs
    /// (`requires via_in_pad`), deduplicated and in a stable order.
    ///
    /// A requirement, not a permission: it says the design cannot be built
    /// without the process, and the gate still has to find a manufacturer
    /// profile that offers it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub required_processes: BTreeSet<ProcessRequirement>,
    pub source_span: Span,
}

/// A fabrication process a board declares it cannot be built without.
///
/// Deliberately a closed set rather than free text. A misspelled
/// requirement that reached the router as an unknown string would read
/// exactly like a board that asked for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRequirement {
    /// The process must fill and cap a via drilled inside an SMD pad.
    /// At fine pitch this is the only escape the arithmetic leaves; on a
    /// process that cannot do it, the same copper is a solder-bridging
    /// defect.
    ViaInPad,
}

impl ProcessRequirement {
    /// Resolve the spelling a `requires` statement used.
    #[must_use]
    pub fn from_source(name: &str) -> Option<Self> {
        match name {
            "via_in_pad" => Some(Self::ViaInPad),
            _ => None,
        }
    }

    /// The spelling a board writes, for diagnostics and round-tripping.
    #[must_use]
    pub const fn as_source(self) -> &'static str {
        match self {
            Self::ViaInPad => "via_in_pad",
        }
    }

    /// Every requirement this build knows, for the "expected one of" list
    /// an unknown name is reported against.
    pub const ALL: &'static [Self] = &[Self::ViaInPad];
}

/// A standard schematic page, selectable per design with
/// `schematic { paper = "…" }`.
///
/// Deliberately not the layout's `SheetSize`: that enum also carries a
/// `Custom { width_mm, height_mm }` variant produced by content
/// measurement, which is a result of layout rather than something an author
/// declares. Keeping the authored subset here means `synth-ir` stays free of
/// a dependency on the layout crate, and an unparseable paper size is a
/// parser error rather than a silently-ignored string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SchematicPaper {
    A5,
    A4,
    A3,
    A2,
    A1,
    A0,
}

impl SchematicPaper {
    /// Every accepted spelling, for diagnostics and the language reference.
    pub const ALL: [SchematicPaper; 6] = [
        SchematicPaper::A5,
        SchematicPaper::A4,
        SchematicPaper::A3,
        SchematicPaper::A2,
        SchematicPaper::A1,
        SchematicPaper::A0,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "A5" => Some(Self::A5),
            "A4" => Some(Self::A4),
            "A3" => Some(Self::A3),
            "A2" => Some(Self::A2),
            "A1" => Some(Self::A1),
            "A0" => Some(Self::A0),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::A5 => "A5",
            Self::A4 => "A4",
            Self::A3 => "A3",
            Self::A2 => "A2",
            Self::A1 => "A1",
            Self::A0 => "A0",
        }
    }
}

/// What a design does when its content does not fit the requested page.
///
/// `Grow` climbs the standard ladder first and only falls back to a
/// multi-sheet hierarchy once the biggest standard page is exhausted.
/// `Hierarchy` skips the growth entirely: the requested page is the page,
/// and anything that does not fit becomes more sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchematicOverflow {
    Grow,
    Hierarchy,
}

impl SchematicOverflow {
    pub const ALL: [SchematicOverflow; 2] = [Self::Grow, Self::Hierarchy];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "grow" => Some(Self::Grow),
            "hierarchy" | "hierarchical" | "sheets" => Some(Self::Hierarchy),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grow => "grow",
            Self::Hierarchy => "hierarchy",
        }
    }
}

/// A named design variant: the refdes left unpopulated in it, plus an
/// optional description. The schematic and layout are shared; only the
/// population differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Refdes marked do-not-populate in this variant, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dnp: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: ComponentId,
    pub refdes: String,
    /// User-declared kind from SynthSpec (`mcu`, `secure_element`, ...).
    /// Resolved against the registry; matches `part.kind` when the
    /// part exists.
    pub kind: String,
    /// Resolved registry part. `None` if the part id failed
    /// resolution (a diagnostic will have been emitted).
    pub part: Option<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional user-specified display value (`value "10k"`), used by
    /// the schematic emitter and BOM. Falls back to the part's MPN or
    /// id when absent.
    pub value: Option<String>,
    /// Do-not-populate: exports `(dnp yes)` on the KiCad symbol and
    /// leaves the part out of the BOM and pick-and-place. ERC still
    /// checks the part exactly like a populated one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dnp: bool,
    /// Structured component data beyond the display `value`, keyed by
    /// the canonical KiCad field name (`Tolerance`, `Voltage`,
    /// `Power`, `Dielectric`). Exported as hidden symbol properties.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_hint: Option<PlacementConstraint>,
    /// How this package's pins leave it (`escape { … }`). `None` means the
    /// design did not say, and a dense grid array is escaped anyway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escape: Option<EscapeIntent>,
    /// Name of the `group` this component was declared inside — the
    /// sub-circuit an engineer would point at ("USB-C input"). `None`
    /// for components declared directly in the board body.
    ///
    /// Purely an annotation: it never affects connectivity, and refdes
    /// remain board-unique across groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Name of the `sheet` block this component was declared inside —
    /// the hierarchical-sheet boundary (§P26). `None` for components
    /// declared directly in the board body. Like [`Self::group`],
    /// purely an annotation: it never affects connectivity, and
    /// refdes remain board-unique across sheets. A board large enough
    /// to overflow A2 splits on these boundaries at export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub source_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementRegion {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Centre,
    TopEdge,
    BottomEdge,
    LeftEdge,
    RightEdge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementEdge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementSide {
    Above,
    Below,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementPriority {
    Soft,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementConstraint {
    pub region: Option<PlacementRegion>,
    pub edge: Option<PlacementEdge>,
    pub near: Option<String>,
    pub side: Option<PlacementSide>,
    pub priority: PlacementPriority,
}

impl Default for PlacementConstraint {
    fn default() -> Self {
        Self {
            region: None,
            edge: None,
            near: None,
            side: None,
            priority: PlacementPriority::Soft,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "NetWire")]
pub struct Net {
    pub id: NetId,
    /// User-declared name if available; otherwise auto-generated
    /// (`net_0`, `net_1`, ...) for traceability in diagnostics and
    /// the SynthJSON projection.
    ///
    /// Declared via `net "NAME" { … }`, `power "NAME" …`, or
    /// `connect … as "NAME"`.
    pub name: String,
    /// True when `name` is the `net_<n>` placeholder lowering gave a net the
    /// author never named. A declared name that happens to look like that
    /// placeholder (`as "net_7"`) has this false. Always written, so a
    /// declared `net_7` is not read back as the placeholder.
    pub auto_named: bool,
    pub endpoints: Vec<NetEndpoint>,
    /// Netclass this net is joined to (`class "PWR"` on the `net`,
    /// `power`, or `connect` statement). Must name a declared
    /// `netclass`; unknown names are reported at lowering
    /// (`E-SYNTH-NAME-006`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub netclass: Option<String>,
    /// Declared nominal rail voltage (`power "NAME" <voltage>`), in
    /// integer microvolts. Power-domain inference prefers this over
    /// pin-name heuristics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<Voltage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetEndpoint {
    pub component: ComponentId,
    pub pin: PinId,
    pub source_span: Span,
}

/// Differential pair declaration. The `positive`/`negative` strings
/// are the net *names* as written in source. When those names match
/// declared nets (`net "USB_DP" { … }`, `connect … as "USB_DP"`),
/// lowering resolves them to [`NetId`]s in `positive_net` /
/// `negative_net`; legacy designs without named nets leave them
/// `None` and validation falls back to endpoint-name matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffPair {
    pub positive: String,
    pub negative: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positive_net: Option<NetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_net: Option<NetId>,
    pub impedance: Option<Impedance>,
    /// Length the two halves may differ by.
    ///
    /// Without one the router reports skew but has nothing to measure it
    /// against, so a declared tolerance is the only way a design can say
    /// whether a pair is acceptable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_skew: Option<Length>,
    /// How hard the router should work to keep the halves coupled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub couple: Option<Couple>,
    pub source_span: Span,
}

/// How hard the router should try to route a differential pair as a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Couple {
    /// Route both halves however they route.
    Loose,
    /// Prefer a coupled run, even at the cost of not routing.
    Tight,
}

impl Couple {
    #[must_use]
    pub fn parse(mode: &str) -> Option<Self> {
        match mode.to_ascii_lowercase().as_str() {
            "loose" => Some(Self::Loose),
            "tight" => Some(Self::Tight),
            _ => None,
        }
    }
}

/// A free-text design note (`notes "Title" { "line" … }`) with the
/// `group` it was declared inside, if any, and the `sheet` block it
/// was declared inside, if any (for per-sheet placement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub source_span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keepout {
    pub name: String,
    pub radius: Option<Length>,
    pub source_span: Span,
}

/// The board's physical layers in the order written, outermost copper first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stackup {
    pub layers: Vec<StackupLayer>,
    pub source_span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackupLayer {
    Copper {
        thickness: Length,
        source_span: Span,
    },
    Insulator {
        thickness: Length,
        er: DielectricConstant,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<String>,
        source_span: Span,
    },
}

impl StackupLayer {
    pub fn thickness(&self) -> Length {
        match self {
            Self::Copper { thickness, .. } | Self::Insulator { thickness, .. } => *thickness,
        }
    }

    pub fn is_copper(&self) -> bool {
        matches!(self, Self::Copper { .. })
    }
}

impl Stackup {
    pub fn copper_count(&self) -> usize {
        self.layers.iter().filter(|l| l.is_copper()).count()
    }

    pub fn total_thickness(&self) -> Length {
        Length(self.layers.iter().map(|l| l.thickness().0).sum())
    }
}

/// A declared `group` region and its optional header attributes
/// (schematic-quality plan Phase D1).
///
/// The name is the identifier components reference; `title` is what a
/// reader sees when the two differ. `color` overrides the schematic
/// box hue, and `region` pins the region to a page quadrant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<PlacementRegion>,
    pub source_span: Span,
}

impl Group {
    /// The text a caption should show: the title when set, else the
    /// identifier.
    #[must_use]
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// A named routing-constraint class with optional default trace
/// width and clearance rules, in integer base units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub trace_width: Option<Length>,
    pub clearance: Option<Length>,
    /// Explicit schematic hue (`color "#c2410c"`), as RGB. `None`
    /// falls back to the fixed palette / hashed hue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Single-ended characteristic impedance target (`impedance 50ohm`),
    /// checked against the routed geometry on the stackup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impedance: Option<Impedance>,
    /// Current the class's nets carry (`current 1.5A`), checked against the
    /// routed width via the IPC-2221 ampacity curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Current>,
    /// Give the class's nets a copper plane (`plane`): the exporter floods a
    /// zone for each member net on the plane layers too, not only for ground.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plane: bool,
    /// Maximum routed length for the class's nets (`max_length 50mm`), checked
    /// against the routed copper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<Length>,
    /// Length the class's nets may differ from each other (`max_skew 1mm`), a
    /// bus/group length-match budget checked across the class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_skew: Option<Length>,
    pub source_span: Span,
}

/// A `Net` as it is read: `auto_named` is missing from data written before
/// the flag existed, when `net_<n>` was how the placeholder was told apart.
#[derive(Deserialize)]
struct NetWire {
    id: NetId,
    name: String,
    #[serde(default)]
    auto_named: Option<bool>,
    endpoints: Vec<NetEndpoint>,
    #[serde(default)]
    netclass: Option<String>,
    #[serde(default)]
    voltage: Option<Voltage>,
}

impl From<NetWire> for Net {
    fn from(w: NetWire) -> Self {
        let auto_named = w.auto_named.unwrap_or_else(|| {
            w.name
                .strip_prefix("net_")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
        Net {
            id: w.id,
            name: w.name,
            auto_named,
            endpoints: w.endpoints,
            netclass: w.netclass,
            voltage: w.voltage,
        }
    }
}


/// Escape engine for a package's pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscapeMethod {
    /// Route out between the pads, in the channels they leave.
    Channel,
    /// Drop a via inside each pad and route away underneath. The only
    /// method a fully populated fine-pitch array leaves open.
    Underpad,
}

impl EscapeMethod {
    /// Resolve the spelling an `escape { method … }` used.
    #[must_use]
    pub fn from_source(name: &str) -> Option<Self> {
        match name {
            "channel" => Some(Self::Channel),
            "underpad" => Some(Self::Underpad),
            _ => None,
        }
    }

    /// The spelling a board writes, which is also the engine's own.
    #[must_use]
    pub const fn as_source(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Underpad => "underpad",
        }
    }

    /// Every method this build knows, for an unknown name's error.
    pub const ALL: &'static [Self] = &[Self::Channel, Self::Underpad];
}

/// What a component's `escape { … }` block asked for.
///
/// `off` and a method are not contradictory in the grammar, so a board can
/// say both; `off` wins, because refusing to escape a package is the
/// stronger statement and silently escaping it anyway would be the worse
/// failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapeIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<EscapeMethod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
    pub source_span: Span,
}
impl Component {
    /// Look up a pin by name on the resolved part.
    pub fn find_pin(&self, name: &str) -> Option<(PinId, &synth_registry::Pin)> {
        let part = self.part.as_ref()?;
        // Datasheet-style pin names are often written as aliases such as
        // `GPIO26/ADC0`.  The registry stores the canonical primary name,
        // so accept the first slash-separated alias while preserving the
        // original source spelling for diagnostics and round-tripping.
        let canonical = name.split('/').next().unwrap_or(name);
        let registry_alias = name.replace('/', "_");
        let idx = part
            .pins
            .iter()
            .position(|p| p.name == name || p.name == canonical || p.name == registry_alias)?;
        // u32 cast is bounded: parts cannot exceed registry-validated pin counts.
        Some((PinId(idx as u32), &part.pins[idx]))
    }

    /// This component's refdes for diagnostic messages: `` `U1` ``,
    /// or `` `U1` (group "Power") `` when declared inside a group, so
    /// a reader can locate the part's sub-circuit without tracing.
    pub fn describe(&self) -> String {
        match self.group.as_deref() {
            Some(group) => format!("`{}` (group \"{group}\")", self.refdes),
            None => format!("`{}`", self.refdes),
        }
    }

    /// One `refdes.pin` endpoint for diagnostic messages:
    /// `` `U1.vout` ``, or `` `U1.vout` (group "Power") `` when
    /// grouped. Prefer this over hand-formatting `` `{}.{}` `` so
    /// group context can never be silently dropped.
    pub fn describe_pin(&self, pin_name: &str) -> String {
        match self.group.as_deref() {
            Some(group) => format!("`{}.{pin_name}` (group \"{group}\")", self.refdes),
            None => format!("`{}.{pin_name}`", self.refdes),
        }
    }
}

impl Board {
    pub fn component(&self, id: ComponentId) -> Option<&Component> {
        self.components.get(id.0 as usize)
    }

    pub fn net(&self, id: NetId) -> Option<&Net> {
        self.nets.get(id.0 as usize)
    }

    pub fn pin(&self, c: ComponentId, p: PinId) -> Option<&synth_registry::Pin> {
        self.component(c)?.part.as_ref()?.pins.get(p.0 as usize)
    }

    pub fn net_matches_name(&self, net: &Net, target: &str) -> bool {
        if net.name == target {
            return true;
        }
        net.endpoints.iter().any(|e| {
            let (Some(comp), Some(pin)) =
                (self.component(e.component), self.pin(e.component, e.pin))
            else {
                return false;
            };
            let refdes_pin = format!("{}_{}", comp.refdes.to_lowercase(), pin.name.to_lowercase());
            let dot_refdes_pin = format!("{}.{}", comp.refdes, pin.name);
            target.eq_ignore_ascii_case(&refdes_pin) || target.eq_ignore_ascii_case(&dot_refdes_pin)
        })
    }

    pub fn diff_pair_legs<'a>(&'a self, dp: &'a DiffPair) -> [(&'a str, Option<&'a Net>); 2] {
        let net_of = |resolved: Option<NetId>, name: &str| {
            resolved
                .and_then(|id| self.net(id))
                .or_else(|| self.nets.iter().find(|n| self.net_matches_name(n, name)))
        };
        [
            (dp.positive.as_str(), net_of(dp.positive_net, &dp.positive)),
            (dp.negative.as_str(), net_of(dp.negative_net, &dp.negative)),
        ]
    }

    pub fn impedance_legs(&self) -> impl Iterator<Item = (&Net, &Net, &DiffPair, Impedance)> {
        self.diff_pairs.iter().filter_map(|dp| {
            let [(_, Some(pos)), (_, Some(neg))] = self.diff_pair_legs(dp) else {
                return None;
            };
            Some((pos, neg, dp, dp.impedance?))
        })
    }

    pub fn declared_leg_geometry(
        &self,
        leg: &Net,
        default_gap: Length,
    ) -> Option<(Length, Length)> {
        Some((
            self.declared_trace_width(leg)?,
            self.declared_clearance(leg).unwrap_or(default_gap),
        ))
    }

    pub fn netclass_pair_geometry(
        &self,
        class: &str,
        default_gap: Length,
    ) -> Option<(Length, Length)> {
        let mut carried = self
            .impedance_legs()
            .filter(|(pos, neg, ..)| {
                pos.id != neg.id
                    && [pos, neg]
                        .iter()
                        .any(|leg| leg.netclass.as_deref() == Some(class))
            })
            .map(|(pos, neg, ..)| {
                let geometry = self.declared_leg_geometry(pos, default_gap)?;
                (self.declared_leg_geometry(neg, default_gap)? == geometry).then_some(geometry)
            });
        let first = carried.next()??;
        carried.all(|g| g == Some(first)).then_some(first)
    }

    fn declared_netclass(&self, net: &Net) -> Option<&NetClass> {
        let class = net.netclass.as_deref()?;
        self.netclasses.iter().find(|nc| nc.name == class)
    }

    pub fn declared_trace_width(&self, net: &Net) -> Option<Length> {
        self.declared_netclass(net)?.trace_width
    }

    pub fn declared_clearance(&self, net: &Net) -> Option<Length> {
        self.declared_netclass(net)?.clearance
    }

    /// A declared `group` by name, for header attributes (title,
    /// colour, pinned region).
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// A refdes for diagnostic messages looked up by name:
    /// `` `U1` ``, or `` `U1` (group "Power") `` when grouped. Falls
    /// back to the bare refdes when it names no declared component.
    pub fn describe_refdes(&self, refdes: &str) -> String {
        match self.components.iter().find(|c| c.refdes == refdes) {
            Some(component) => component.describe(),
            None => format!("`{refdes}`"),
        }
    }

    /// Returns an iterator over `(NetId, &Net)` pairs that include a
    /// given endpoint. Used by ERC rules that need to inspect both
    /// sides of a connection.
    pub fn nets_containing(
        &self,
        component: ComponentId,
        pin: PinId,
    ) -> impl Iterator<Item = (NetId, &Net)> {
        self.nets.iter().filter_map(move |n| {
            if n.endpoints
                .iter()
                .any(|e| e.component == component && e.pin == pin)
            {
                Some((n.id, n))
            } else {
                None
            }
        })
    }
}
