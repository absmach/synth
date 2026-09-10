// SPDX-License-Identifier: Apache-2.0

//! Agent-facing structured layout mutations (§7.8.8 Stage E).
//!
//! Five ops, deliberately not a general "set arbitrary coordinates"
//! API — that would just be pixel-dragging with extra steps. Each op
//! answers a specific refinement an agent (or the browser's
//! `/api/v1/layout/save` endpoint) needs after inspecting a
//! rendered schematic:
//!
//! - [`LayoutOp::MoveComponent`] / [`LayoutOp::Rotate`] — reposition
//!   or reorient one component.
//! - [`LayoutOp::GroupBlock`] — pull a set of components into a
//!   tidy column beside an anchor (e.g. "put these three decoupling
//!   caps together next to U1").
//! - [`LayoutOp::ReplaceWireWithLabel`] — force a specific net to
//!   render as net-label stubs instead of a wire.
//! - [`LayoutOp::RerouteNet`] — re-run routing for one net (e.g.
//!   after moving something that was blocking it).
//!
//! `apply_op` never touches `board` (the connectivity source of
//! truth, per §7.5.7): it can move, rotate, group, or re-route, but
//! it cannot add or remove a connection.
//!
//! ## Known limitation: overrides don't survive a later structural op
//!
//! The structural ops (`MoveComponent`/`Rotate`/`GroupBlock`) re-run
//! `crate::route_and_label`, which recomputes `net_labels` and
//! `wires` for the *whole* board from scratch — this codebase's
//! router has no incremental per-net mode to re-run just the nets a
//! move actually affects, so "only Stage C+D for the affected nets"
//! (as originally scoped in §7.8.8) is approximated here as "all of
//! Stage C+D," not a true incremental re-route. In practice this is
//! fast enough for a single discrete op (unlike continuous drag,
//! §7.8's `synth-web` migration note), but it has one real
//! consequence: a net you forced to a label via
//! [`LayoutOp::ReplaceWireWithLabel`] reverts to the automatic
//! distance/crossing heuristic the next time a structural op runs,
//! because nothing currently remembers "this net was manually
//! forced." Re-apply `ReplaceWireWithLabel` after a later structural
//! op if the override needs to persist. Fixing this for real needs
//! either a `Layout` field tracking forced-label nets or genuine
//! incremental Stage C — both out of scope here.

use serde::{Deserialize, Serialize};
use synth_ir::{Board, ComponentId, NetId};

use crate::{route_and_label, snap_grid, Layout, NetLabel, Rotation};

/// A single structured edit to an existing [`Layout`]. See the
/// module docs for what each variant is for.
///
/// `#[serde(tag = "kind")]` gives this a friendly wire shape for the
/// MCP `synth_mutate_layout` tool and the browser's
/// `/api/v1/layout/save` endpoint, e.g.
/// `{"kind": "move_component", "id": 3, "x_mm": 50.8, "y_mm": 25.4}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutOp {
    /// Move one component's centre to an absolute sheet position (mm).
    MoveComponent {
        id: ComponentId,
        x_mm: f64,
        y_mm: f64,
    },
    /// Set one component's rotation.
    Rotate { id: ComponentId, rotation: Rotation },
    /// Arrange `ids` into a tidy vertical column immediately to the
    /// right of `anchor`'s current position, in the given order.
    /// `anchor` itself does not move. Useful for tidying a set of
    /// components the auto-layout scattered — e.g. pulling every
    /// decoupling cap for one IC into one visual group.
    GroupBlock {
        ids: Vec<ComponentId>,
        anchor: ComponentId,
    },
    /// Force `net` to render as per-endpoint net-label stubs instead
    /// of a wire, regardless of span or crossing count.
    ReplaceWireWithLabel { net: NetId },
    /// Re-run routing so `net` (and, as a side effect of the current
    /// implementation — see the module docs — every other net too)
    /// gets a fresh route. Use this after a move/rotate that should
    /// let a previously-blocked net find a path; those ops already
    /// re-route everything on their own, so `RerouteNet` mainly
    /// exists as an explicit "just re-route, nothing moved" op and
    /// as the forward-compatible entry point for when this router
    /// gains real per-net incremental routing.
    RerouteNet { net: NetId },
}

/// Why a [`LayoutOp`] could not be applied.
///
/// Plain (externally tagged) serde representation, e.g.
/// `{"UnknownComponent": 3}` — unlike [`LayoutOp`], this can't use
/// `#[serde(tag = "...")]` (internal tagging), because
/// [`ComponentId`]/[`NetId`] are `#[serde(transparent)]` newtypes
/// that serialize as bare numbers, and serde's internally-tagged
/// representation requires every variant to serialize as a JSON map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutOpError {
    UnknownComponent(ComponentId),
    UnknownNet(NetId),
    /// [`LayoutOp::GroupBlock`] was given an empty `ids` list.
    EmptyGroup,
}

impl std::fmt::Display for LayoutOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownComponent(id) => {
                write!(f, "component {} is not in this board", id.0)
            }
            Self::UnknownNet(id) => write!(f, "net {} is not in this board", id.0),
            Self::EmptyGroup => write!(f, "GroupBlock needs at least one component id"),
        }
    }
}

impl std::error::Error for LayoutOpError {}

/// Vertical spacing (mm) between members of a [`LayoutOp::GroupBlock`]
/// column. Matches `MEMBER_DX`'s role for the auto-layout's own
/// member columns — close enough to read as "grouped", far enough
/// apart that refdes labels don't collide.
const GROUP_BLOCK_SPACING_MM: f64 = 14.0;
/// Horizontal clearance (mm) from the anchor's centre to the group
/// column, matching the auto-layout's `MEMBER_CLEARANCE`.
const GROUP_BLOCK_CLEARANCE_MM: f64 = 17.78;

/// Apply `op` to `layout` in place.
///
/// Structural ops ([`LayoutOp::MoveComponent`], [`LayoutOp::Rotate`],
/// [`LayoutOp::GroupBlock`]) reposition components and then re-run
/// `crate::route_and_label` so wires/labels reflect the new
/// positions — see the module docs for why this is a full re-route
/// rather than a true per-net incremental one.
/// [`LayoutOp::ReplaceWireWithLabel`] is the one genuinely local op:
/// it only touches that net's wire/label entries, no re-route.
/// [`LayoutOp::RerouteNet`] also re-runs the full
/// `crate::route_and_label` today, for the same reason.
///
/// Returns an error (leaving `layout` unmodified) if the op
/// references a component or net id that isn't on `board`.
pub fn apply_op(layout: &mut Layout, board: &Board, op: LayoutOp) -> Result<(), LayoutOpError> {
    match op {
        LayoutOp::MoveComponent { id, x_mm, y_mm } => {
            require_component(board, id)?;
            let placement =
                find_placement_mut(layout, id).ok_or(LayoutOpError::UnknownComponent(id))?;
            placement.center_mm = (snap_grid(x_mm), snap_grid(y_mm));
            route_and_label(board, layout);
        }
        LayoutOp::Rotate { id, rotation } => {
            require_component(board, id)?;
            let placement =
                find_placement_mut(layout, id).ok_or(LayoutOpError::UnknownComponent(id))?;
            placement.rotation = rotation;
            route_and_label(board, layout);
        }
        LayoutOp::GroupBlock { ids, anchor } => {
            if ids.is_empty() {
                return Err(LayoutOpError::EmptyGroup);
            }
            require_component(board, anchor)?;
            for &id in &ids {
                require_component(board, id)?;
                // Validate every id has a layout placement up front, before
                // any mutation starts below — `apply_op` promises to leave
                // `layout` unmodified on error, so this can't be checked
                // lazily inside the mutation loop.
                if id != anchor && layout.placement(id).is_none() {
                    return Err(LayoutOpError::UnknownComponent(id));
                }
            }
            let (anchor_x, anchor_y) = layout
                .placement(anchor)
                .ok_or(LayoutOpError::UnknownComponent(anchor))?
                .center_mm;
            let col_x = snap_grid(anchor_x + GROUP_BLOCK_CLEARANCE_MM);
            let total_height = (ids.len().saturating_sub(1)) as f64 * GROUP_BLOCK_SPACING_MM;
            let start_y = snap_grid(anchor_y - total_height / 2.0);
            for (i, &id) in ids.iter().enumerate() {
                // The anchor is the fixed reference point the column is
                // built beside — even if a caller includes it in `ids`
                // (e.g. by accident), it must not be relocated to a
                // column slot itself. See the `anchor` doc comment above.
                if id == anchor {
                    continue;
                }
                let y = snap_grid(start_y + i as f64 * GROUP_BLOCK_SPACING_MM);
                let placement =
                    find_placement_mut(layout, id).ok_or(LayoutOpError::UnknownComponent(id))?;
                placement.center_mm = (col_x, y);
            }
            route_and_label(board, layout);
        }
        LayoutOp::ReplaceWireWithLabel { net } => {
            let net_ir = board.net(net).ok_or(LayoutOpError::UnknownNet(net))?;
            layout.wires.retain(|w| w.net != net);
            layout.net_labels.retain(|l| l.net != net);
            let text =
                crate::pick_net_label(board, net_ir).unwrap_or_else(|| format!("NET_{}", net.0));
            for ep in &net_ir.endpoints {
                layout.net_labels.push(NetLabel {
                    net,
                    component: ep.component,
                    pin: ep.pin,
                    label: text.clone(),
                });
            }
            // A hand-forced label can collide with labels the last
            // routing pass produced; re-run the uniqueness pass so
            // the sheet-wide guarantee still holds.
            crate::uniquify_net_labels(board, &mut layout.net_labels);
        }
        LayoutOp::RerouteNet { net } => {
            // `route_and_label` recomputes wires/labels for the
            // whole board from scratch (see module docs), so there's
            // nothing to prune for `net` specifically first — this
            // call already supersedes any prior wire/label state.
            require_net(board, net)?;
            route_and_label(board, layout);
        }
    }
    Ok(())
}

fn require_component(board: &Board, id: ComponentId) -> Result<(), LayoutOpError> {
    if board.component(id).is_some() {
        Ok(())
    } else {
        Err(LayoutOpError::UnknownComponent(id))
    }
}

fn require_net(board: &Board, id: NetId) -> Result<(), LayoutOpError> {
    if board.net(id).is_some() {
        Ok(())
    } else {
        Err(LayoutOpError::UnknownNet(id))
    }
}

fn find_placement_mut(
    layout: &mut Layout,
    id: ComponentId,
) -> Option<&mut crate::ComponentPlacement> {
    layout.components.iter_mut().find(|p| p.id == id)
}
