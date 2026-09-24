// SPDX-License-Identifier: Apache-2.0

//! The canonical `Board` IR. Owned arena-indexed types with stable
//! ids that downstream stages (ERC, placement, routing, export) use
//! as the addressable model.

use serde::{Deserialize, Serialize};
use synth_diagnostics::Span;
use synth_registry::Part;

use crate::modules::{BusBundle, ModuleDesc};
use crate::units::{Impedance, Length, Voltage};

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
    /// schematic title block (Sierra Circuits: the title block should
    /// display the Revision).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Design-authority company (`company "…"`), carried into the
    /// schematic title block's Company field. Distinct from
    /// `manufacturer` (who builds the board).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    pub components: Vec<Component>,
    pub nets: Vec<Net>,
    pub diff_pairs: Vec<DiffPair>,
    /// Free-text design notes (`notes "Title" { … }`), in source
    /// order. Rendered on the schematic as titled text blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    pub keepouts: Vec<Keepout>,
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
    pub source_span: Span,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement_hint: Option<PlacementConstraint>,
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
pub struct Net {
    pub id: NetId,
    /// User-declared name if available; otherwise auto-generated
    /// (`net_0`, `net_1`, ...) for traceability in diagnostics and
    /// the SynthJSON projection.
    ///
    /// Declared via `net "NAME" { … }`, `power "NAME" …`, or
    /// `connect … as "NAME"`.
    pub name: String,
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
    pub source_span: Span,
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

/// A named routing-constraint class with optional default trace
/// width and clearance rules, in integer base units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub trace_width: Option<Length>,
    pub clearance: Option<Length>,
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
