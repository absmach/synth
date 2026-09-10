// SPDX-License-Identifier: Apache-2.0

//! The canonical `Board` IR. Owned arena-indexed types with stable
//! ids that downstream stages (ERC, placement, routing, export) use
//! as the addressable model.

use serde::{Deserialize, Serialize};
use synth_diagnostics::Span;
use synth_registry::Part;

use crate::units::{Impedance, Length};

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
    pub components: Vec<Component>,
    pub nets: Vec<Net>,
    pub diff_pairs: Vec<DiffPair>,
    pub keepouts: Vec<Keepout>,
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
    pub name: String,
    pub endpoints: Vec<NetEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetEndpoint {
    pub component: ComponentId,
    pub pin: PinId,
    pub source_span: Span,
}

/// Differential pair declaration. The `positive`/`negative` strings
/// are the net *names* as written in source; Phase 2 does not yet
/// resolve them to [`NetId`]s (the V1 grammar provides no way to
/// name nets at the connection site). Resolution will land alongside
/// explicit net-naming syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffPair {
    pub positive: String,
    pub negative: String,
    pub impedance: Option<Impedance>,
    pub source_span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keepout {
    pub name: String,
    pub radius: Option<Length>,
    pub source_span: Span,
}

impl Component {
    /// Look up a pin by name on the resolved part.
    pub fn find_pin(&self, name: &str) -> Option<(PinId, &synth_registry::Pin)> {
        let part = self.part.as_ref()?;
        let idx = part.pins.iter().position(|p| p.name == name)?;
        // u32 cast is bounded: parts cannot exceed registry-validated pin counts.
        Some((PinId(idx as u32), &part.pins[idx]))
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
