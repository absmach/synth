// SPDX-License-Identifier: Apache-2.0

//! Functional-cluster recognition: one pass over [`Board`] that decides
//! which parts belong to which sub-circuit, and through which net and
//! pins.
//!
//! This used to be implemented three times — once per schematic motif in
//! `synth-layout`, once as PCB module extraction in `synth-place`, and
//! once as a decoupling check in the schematic ERC — and the three drifted
//! apart: the PCB placer never saw I²C pull-ups or dividers, the
//! schematic never saw RF matching networks, and each re-derived its own
//! idea of which capacitor decouples which IC. A schematic cluster that
//! the PCB placer could not act on was a cluster that only existed on
//! paper.
//!
//! One pass fixes that. Every consumer now reads the same
//! [`FunctionalCluster`] list, so "these parts belong together" is a fact
//! the IR holds rather than something each consumer re-derives.
//!
//! What the pass decides is *membership and connectivity* — never
//! coordinates. Presentation (which side of an anchor a member is drawn
//! on, where it lands on a board in millimetres) stays with the consumer,
//! because the schematic sheet and the PCB are different coordinate
//! spaces and legitimately lay a motif out differently. What each member
//! carries is a [`MemberBinding`]: the net that joins it to its anchor
//! and the two pins that meet on that net. That is the pin-level fact a
//! placer needs to put a decoupling capacitor next to the pad it
//! decouples, and it is what this module exists to produce.
//!
//! Passes run in a fixed order of specificity and share one `claimed`
//! set, so a part is claimed by the most specific motif that matches it:
//!
//! 1. LED indicator — LED plus its anode current-limit resistor.
//! 2. USB+ESD — USB connector plus ESD diodes on its pair pins.
//! 3. LDO block — regulator plus rail caps on `vin`/`vout`.
//! 4. Crystal — crystal plus its load capacitors.
//! 5. IC block — IC with `required_decoupling` and/or a reset pin, plus
//!    its decoupling caps, reset network, and pull resistors.
//! 6. I²C bus — part with `i2c_sda`/`i2c_scl` plus both pull-ups.
//! 7. Divider — rail-side resistor plus its mid-to-ground partner.
//! 8. RF matching — antenna plus its matching passives.
//! 9. Orphan rail caps — leftover rail-to-ground capacitors adopted by
//!    whichever cluster draws most heavily from their rail.
//!
//! Then declared groups bound membership (a group is a hard boundary),
//! and anything still unclaimed becomes a singleton.

use std::collections::{HashMap, HashSet};

use synth_registry::{ElectricalType, PinCapability};

use crate::board::{Board, ComponentId, Net, NetId, PinId};
use crate::power_domains::{infer_power_domains, PowerDomainMap};

/// The motif a [`FunctionalCluster`] was recognized as.
///
/// One variant per recognition pass. Consumers name sub-circuits the way
/// an engineer would ("U1 LDO block") rather than by anchor refdes alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClusterKind {
    LedIndicator,
    UsbEsd,
    LdoBlock,
    I2cBus,
    Crystal,
    IcBlock,
    /// A general-purpose connector with the series resistors on its pins.
    ConnectorBlock,
    Divider,
    /// Antenna plus its impedance-matching network.
    ///
    /// Recognized for PCB placement, where an antenna's matching network
    /// belongs beside it. The schematic adapter renders it as singletons,
    /// because a matching network has no meaningful schematic grouping.
    RfMatching,
    /// A part no motif claimed — its own one-part cluster.
    Singleton,
}

impl ClusterKind {
    /// Human-readable motif name, for titles and diagnostics.
    ///
    /// `Singleton` has no motif name of its own — a lone part is named
    /// after the part, not after a pattern — so callers that need a
    /// label for one should fall back to its refdes.
    #[must_use]
    pub fn display_name(self) -> Option<&'static str> {
        match self {
            Self::LedIndicator => Some("LED indicator"),
            Self::UsbEsd => Some("USB ESD protection"),
            Self::LdoBlock => Some("LDO block"),
            Self::I2cBus => Some("I2C bus"),
            Self::Crystal => Some("crystal"),
            Self::IcBlock => Some("IC block"),
            Self::ConnectorBlock => Some("connector"),
            Self::Divider => Some("divider"),
            Self::RfMatching => Some("RF matching network"),
            Self::Singleton => None,
        }
    }
}

/// What part a member plays inside its cluster.
///
/// The role is what lets one consumer keep a fact another ignores: the
/// schematic draws a pull-up above its IC while the PCB placer binds
/// only decoupling, rail, load, and protection parts beside their
/// anchor. Roles are set during recognition — membership is never
/// re-derived downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemberRole {
    /// Capacitor satisfying an IC's `required_decoupling` rule.
    DecouplingCap,
    /// Capacitor on a regulator's `vin` or `vout` rail.
    RailCap,
    /// Capacitor loading one side of a crystal.
    LoadCap,
    /// Rail-to-ground capacitor adopted onto a cluster by a later sweep
    /// because no single anchor's `required_decoupling` could reach it.
    OrphanRailCap,
    /// ESD diode clamping a differential pair at the connector.
    EsdDiode,
    /// Resistor in series with a signal leaving the connector, such as a
    /// USB series resistor.
    SeriesElement,
    /// Current-limit resistor feeding an LED's anode.
    SeriesLimit,
    /// Passive on a reset-capability pin's net.
    ResetNetwork,
    /// Resistor pulling a signal line to a rail.
    PullUp,
    /// Resistor in series on an IC or connector signal pin: neither end is
    /// a rail.
    SignalSeries,
    /// Resistor pulling one half of an I²C bus to its rail.
    I2cPullUp,
    /// Second resistor of a rail-to-mid-to-ground divider.
    DividerPartner,
    /// Passive in an antenna's impedance-matching network.
    RfMatchElement,
}

/// The net and the two pins on it that connect a member to its anchor.
///
/// This is the whole point of the module: placement needs to know *which
/// pads* a member is tied to, not merely which components sit near each
/// other. A decoupling capacitor belongs beside `U1.dvdd`'s pad because
/// that is the binding it carries, not because it happens to be a
/// capacitor on a rail net.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemberBinding {
    /// Net shared by the anchor pin and the member pin.
    pub net: NetId,
    /// The anchor's pin on that net.
    pub anchor_pin: PinId,
    /// The member's pin on that net.
    pub member_pin: PinId,
}

/// One part claimed by a cluster, with the connection that claimed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClusterMember {
    pub component: ComponentId,
    pub role: MemberRole,
    pub binding: MemberBinding,
}

/// Anchor and the parts recognized as belonging with it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionalCluster {
    pub kind: ClusterKind,
    pub anchor: ComponentId,
    pub members: Vec<ClusterMember>,
}

impl FunctionalCluster {
    /// Name this sub-circuit the way an engineer would write it on a
    /// sheet — `"U1 LDO block"` — falling back to the anchor's refdes
    /// alone for a part no motif claimed.
    #[must_use]
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

/// Recognize every functional cluster in `board`.
///
/// Deterministic: passes walk `board.components` in declaration order,
/// claims are taken in that same order, and members are sorted by
/// component id before being recorded.
#[must_use]
pub fn recognize_clusters(board: &Board) -> Vec<FunctionalCluster> {
    let mut recognizer = Recognizer::new(board);
    recognizer.led_indicators(board);
    recognizer.usb_esd(board);
    recognizer.ldo_blocks(board);
    recognizer.crystals(board);
    // IcBlock runs before I2cBus: an I²C host is usually a full IC that
    // must claim its own decoupling caps and reset network. If I2cBus ran
    // first it would claim the MCU and starve IcBlock of the anchor,
    // dropping the MCU's decoupling and reset from the cluster.
    recognizer.ic_blocks(board);
    recognizer.i2c_buses(board);
    recognizer.dividers(board);
    recognizer.rf_matching_networks(board);
    recognizer.connector_blocks(board);
    // Second sweep, after every structural pass: a decoupling cap on a
    // *shared* rail is reachable from no single anchor's
    // `required_decoupling`, so it would otherwise fall through to
    // `Singleton` and be placed far from the part it decouples. Running
    // here lets it attach to whichever cluster actually draws from its
    // rail, `LdoBlock` and `Crystal` included.
    recognizer.attach_orphan_rail_caps(board);
    recognizer.evict_cross_group_members(board);
    recognizer.singletons(board);
    recognizer.clusters
}

/// Shared `claimed` set across passes, plus the clusters found so far.
struct Recognizer {
    claimed: HashSet<ComponentId>,
    clusters: Vec<FunctionalCluster>,
    domains: PowerDomainMap,
}

impl Recognizer {
    fn new(board: &Board) -> Self {
        Self {
            claimed: HashSet::new(),
            clusters: Vec::new(),
            domains: infer_power_domains(board),
        }
    }

    /// Record a cluster and mark its anchor claimed. Members must already
    /// be claimed through [`Recognizer::claim_member`].
    ///
    /// Members keep the order recognition found them in — net order, then
    /// endpoint order — which is deterministic and is the order the PCB
    /// placer has always used to fan members around an anchor. A consumer
    /// that needs another order (the schematic draws members sorted by
    /// refdes) sorts its own copy.
    ///
    /// An empty cluster is still recorded: a pass that anchors on a part
    /// with no members has still identified that part as its own motif
    /// (an IC that declares decoupling but has none wired is still an IC
    /// block), and the anchor must not fall through to `Singleton` and
    /// lose that identity. Passes where "matched but empty" means "did
    /// not match" skip the push instead — see the empty-member guards in
    /// [`Recognizer::ldo_blocks`] and [`Recognizer::crystals`].
    fn push(&mut self, kind: ClusterKind, anchor: ComponentId, members: Vec<ClusterMember>) {
        // Re-assert member claims. Members are normally claimed by
        // `claim_member` as they are found, so this is idempotent; it is
        // here so a pass that builds a member list directly still leaves
        // its parts claimed, and an unclaimed member would otherwise
        // also turn up as its own `Singleton`.
        for member in &members {
            self.claimed.insert(member.component);
        }
        self.claimed.insert(anchor);
        self.clusters.push(FunctionalCluster {
            kind,
            anchor,
            members,
        });
    }

    /// Add a member to the cluster being built, claiming it as we go.
    ///
    /// Claiming here rather than at [`Recognizer::push`] time is what
    /// makes one part one claim: an IC whose rails resolve through two
    /// `required_decoupling` rules reaching the same capacitor must
    /// claim that capacitor once, under the first rule that matched, or
    /// it ends up in its own cluster twice.
    ///
    /// Returns whether the member was taken; a pass that is counting
    /// against a declared limit must only count accepted members.
    fn claim_member(&mut self, members: &mut Vec<ClusterMember>, member: ClusterMember) -> bool {
        if self.claimed.contains(&member.component)
            || members.iter().any(|m| m.component == member.component)
        {
            return false;
        }
        self.claimed.insert(member.component);
        members.push(member);
        true
    }

    // ----- Pass 1: LED indicators -----------------------------------------

    /// An LED with the current-limit resistor wired to its anode. The
    /// resistor is drawn stacked above the LED.
    fn led_indicators(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if !is_led(component) {
                continue;
            }
            let Some(anode) = part.pins.iter().position(|p| p.name == "anode") else {
                continue;
            };
            let anchor_pin = PinId(u32::try_from(anode).unwrap_or(0));
            let mut members = Vec::new();
            for (net, member_pin) in
                passive_endpoints(board, component.id, anchor_pin, &self.claimed)
            {
                if board
                    .component(member_pin.0)
                    .is_some_and(|c| part_kind(Some(c)) == Some("resistor"))
                {
                    self.claim_member(
                        &mut members,
                        ClusterMember {
                            component: member_pin.0,
                            role: MemberRole::SeriesLimit,
                            binding: MemberBinding {
                                net,
                                anchor_pin,
                                member_pin: member_pin.1,
                            },
                        },
                    );
                    break; // one limit resistor per LED
                }
            }
            self.push(ClusterKind::LedIndicator, component.id, members);
        }
    }

    // ----- Pass 2: USB + ESD ----------------------------------------------

    /// A connector with `usb_dp`/`usb_dn` capability pins plus the ESD
    /// diodes clamping those pair nets and the CC pull-down resistors.
    fn usb_esd(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "connector" {
                continue;
            }
            let pair_pins: Vec<PinId> = part
                .pins
                .iter()
                .enumerate()
                .filter(|(_, pin)| {
                    pin.capabilities.contains(&PinCapability::UsbDp)
                        || pin.capabilities.contains(&PinCapability::UsbDn)
                })
                .map(|(idx, _)| PinId(u32::try_from(idx).unwrap_or(0)))
                .collect();
            if pair_pins.is_empty() {
                continue;
            }
            let mut members = Vec::new();
            for anchor_pin in pair_pins {
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) == Some("diode") {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::EsdDiode,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }
            // Rd pull-downs on the connector's CC pins.
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if !pin.capabilities.contains(&PinCapability::UsbCc) {
                    continue;
                }
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) == Some("resistor")
                        && resistor_pulls_to_rail(board, &self.domains, other.0)
                    {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::PullUp,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }
            self.push(ClusterKind::UsbEsd, component.id, members);
        }
    }

    // ----- Pass 3: LDO blocks ---------------------------------------------

    /// A regulator with the capacitors on its `vin` and `vout` rails.
    ///
    /// Claims are capped per rail by the regulator's own
    /// `required_decoupling` count (default 1). Without that cap the LDO
    /// would absorb every capacitor on the shared power rail — including
    /// the decoupling caps belonging to the downstream ICs that rail
    /// feeds — leaving those ICs visibly undecoupled.
    fn ldo_blocks(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "regulator" {
                continue;
            }
            let mut members = Vec::new();
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                let lower = pin.name.to_ascii_lowercase();
                if lower != "vin" && lower != "vout" {
                    continue;
                }
                let allowance = part
                    .required_decoupling
                    .iter()
                    .find(|d| d.net == pin.name)
                    .map_or(1, |d| d.count.max(1) as usize);
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                let mut claimed_for_rail = 0_usize;
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if claimed_for_rail >= allowance {
                        break;
                    }
                    if part_kind(board.component(other.0)) == Some("capacitor")
                        && self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::RailCap,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        )
                    {
                        claimed_for_rail += 1;
                    }
                }
            }
            if members.is_empty() {
                continue;
            }
            self.push(ClusterKind::LdoBlock, component.id, members);
        }
    }

    // ----- Pass 4: crystals ------------------------------------------------

    /// A crystal with the capacitors loading each of its pins.
    fn crystals(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "crystal" {
                continue;
            }
            let mut members = Vec::new();
            for pin_idx in 0..part.pins.len() {
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) == Some("capacitor") {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::LoadCap,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }
            if members.is_empty() {
                continue;
            }
            self.push(ClusterKind::Crystal, component.id, members);
        }
    }

    // ----- Pass 5: IC blocks ----------------------------------------------

    /// An IC with declared `required_decoupling` and/or a reset pin,
    /// together with its decoupling capacitors, its reset network, and
    /// any resistor pulling one of its signal pins to a rail.
    #[allow(clippy::too_many_lines)]
    fn ic_blocks(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let has_decoupling = !part.required_decoupling.is_empty();
            let has_reset_pin = part
                .pins
                .iter()
                .any(|p| p.capabilities.contains(&PinCapability::Reset));
            if !has_decoupling && !has_reset_pin {
                continue;
            }
            let mut members = Vec::new();

            // Decoupling capacitors, up to the declared count per rail.
            for rule in &part.required_decoupling {
                let Some(pin_idx) = part.pins.iter().position(|p| p.name == rule.net) else {
                    continue;
                };
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                let max_claims = rule.count as usize;
                let mut claims_for_rail = 0_usize;
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if claims_for_rail >= max_claims {
                        break;
                    }
                    if part_kind(board.component(other.0)) == Some("capacitor")
                        && self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::DecouplingCap,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        )
                    {
                        claims_for_rail += 1;
                    }
                }
            }

            // Reset network: anything on a reset-capability pin's net.
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if !pin.capabilities.contains(&PinCapability::Reset) {
                    continue;
                }
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if matches!(
                        part_kind(board.component(other.0)),
                        Some("resistor" | "capacitor" | "switch")
                    ) {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::ResetNetwork,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }

            // Pull-up / pull-down resistors on the IC's signal pins.
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if !is_side_pin(pin) {
                    continue;
                }
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) != Some("resistor") {
                        continue;
                    }
                    // A resistor with no rail on either end sits in series
                    // on the signal.
                    let role = if resistor_pulls_to_rail(board, &self.domains, other.0) {
                        MemberRole::PullUp
                    } else {
                        MemberRole::SignalSeries
                    };
                    self.claim_member(
                        &mut members,
                        ClusterMember {
                            component: other.0,
                            role,
                            binding: MemberBinding {
                                net,
                                anchor_pin,
                                member_pin: other.1,
                            },
                        },
                    );
                }
            }

            self.push(ClusterKind::IcBlock, component.id, members);
        }
    }

    // ----- Pass 6: I²C buses ----------------------------------------------

    /// A part carrying both `i2c_sda` and `i2c_scl` with a pull-up on
    /// *each* net — one resistor is not a bus, and belongs to the IC
    /// block pass instead.
    fn i2c_buses(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let sda = part
                .pins
                .iter()
                .position(|p| p.capabilities.contains(&PinCapability::I2cSda));
            let scl = part
                .pins
                .iter()
                .position(|p| p.capabilities.contains(&PinCapability::I2cScl));
            let (Some(sda), Some(scl)) = (sda, scl) else {
                continue;
            };
            let mut members = Vec::new();
            let mut has_sda_pull = false;
            let mut has_scl_pull = false;
            for (pin_idx, want) in [(sda, &mut has_sda_pull), (scl, &mut has_scl_pull)] {
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) != Some("resistor") {
                        continue;
                    }
                    if is_pullup_to_power(board, other.0, net)
                        && self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::I2cPullUp,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        )
                    {
                        *want = true;
                    }
                }
            }
            if !has_sda_pull || !has_scl_pull {
                continue;
            }
            self.push(ClusterKind::I2cBus, component.id, members);
        }
    }

    // ----- Pass 7: dividers ------------------------------------------------

    /// A rail→mid→ground divider made of two resistors, anchored on the
    /// rail-side resistor and claiming the mid-to-ground partner.
    fn dividers(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "resistor" || part.pins.len() != 2 {
                continue;
            }
            let Some((mid_net, anchor_pin, member_pin, member)) =
                recognize_divider(board, &self.claimed, component.id)
            else {
                continue;
            };
            self.push(
                ClusterKind::Divider,
                component.id,
                vec![ClusterMember {
                    component: member,
                    role: MemberRole::DividerPartner,
                    binding: MemberBinding {
                        net: mid_net,
                        anchor_pin,
                        member_pin,
                    },
                }],
            );
        }
    }

    // ----- Pass 8: RF matching networks -----------------------------------

    /// An antenna with the resistors, capacitors, and inductors forming
    /// its impedance-matching network. Recognized for PCB placement,
    /// where the network belongs beside the antenna; the schematic
    /// adapter leaves these as singletons.
    fn rf_matching_networks(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            if part_kind(Some(component)) != Some("antenna") {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let mut members = Vec::new();
            for pin_idx in 0..part.pins.len() {
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if matches!(
                        part_kind(board.component(other.0)),
                        Some("resistor" | "capacitor" | "inductor")
                    ) {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::RfMatchElement,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }
            self.push(ClusterKind::RfMatching, component.id, members);
        }
    }

    // ----- Pass 8b: connector series resistors ------------------------------

    /// A general-purpose connector with a resistor in series on one of its
    /// pins: the other end is not a rail, so the resistor is a series
    /// element of a signal leaving the connector (a UART series resistor,
    /// a bus terminator). A connector the USB pass claimed keeps that
    /// cluster, and one with no such resistor falls through to
    /// `Singleton`.
    fn connector_blocks(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "connector" {
                continue;
            }
            let mut members = Vec::new();
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if !is_side_pin(pin) {
                    continue;
                }
                let anchor_pin = PinId(u32::try_from(pin_idx).unwrap_or(0));
                for (net, other) in
                    passive_endpoints(board, component.id, anchor_pin, &self.claimed)
                {
                    if part_kind(board.component(other.0)) == Some("resistor")
                        && !resistor_pulls_to_rail(board, &self.domains, other.0)
                    {
                        self.claim_member(
                            &mut members,
                            ClusterMember {
                                component: other.0,
                                role: MemberRole::SignalSeries,
                                binding: MemberBinding {
                                    net,
                                    anchor_pin,
                                    member_pin: other.1,
                                },
                            },
                        );
                    }
                }
            }
            if !members.is_empty() {
                self.push(ClusterKind::ConnectorBlock, component.id, members);
            }
        }
    }

    // ----- Second sweep: orphan rail capacitors ----------------------------

    /// Adopt rail-to-ground capacitors that no anchor's
    /// `required_decoupling` could reach onto the cluster that draws
    /// hardest from their rail.
    ///
    /// On a shared rail (one merged net feeding the regulator, the MCU
    /// and a sensor) every leftover capacitor would otherwise fall
    /// through to `Singleton` and be placed by power-flow layer — over a
    /// hundred millimetres from the part it decouples. Each orphan
    /// claims the anchor with the most pins on its rail net, tie-broken
    /// by declaration adjacency, which is how authors already express
    /// intent (`C4` sits next to `U2` in the source).
    #[allow(clippy::too_many_lines)]
    fn attach_orphan_rail_caps(&mut self, board: &Board) {
        // Any already-formed cluster may adopt an orphan — an LDO's bulk
        // and output caps hang off an `LdoBlock`, not an `IcBlock`.
        // Passive-anchored clusters (a divider's resistor, an LED's
        // series resistor) are excluded: they consume no rail and would
        // drag caps away from the part that does.
        let anchors: Vec<(ComponentId, usize)> = self
            .clusters
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !matches!(
                    part_kind(board.component(c.anchor)),
                    Some("capacitor" | "resistor" | "inductor")
                )
            })
            .map(|(idx, c)| (c.anchor, idx))
            .collect();

        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            if part.kind != "capacitor" || part.pins.len() != 2 {
                continue;
            }
            // The two nets the capacitor bridges. Both must exist and be
            // distinct: a part with a floating pin is not a decoupling
            // candidate at all.
            let mut cap_nets: Vec<(NetId, PinId)> = Vec::new();
            for pin_idx in 0..2_u32 {
                let pin = PinId(pin_idx);
                let Some(net_id) = board
                    .nets_containing(component.id, pin)
                    .map(|(net_id, _)| net_id)
                    .next()
                else {
                    continue;
                };
                if cap_nets.iter().any(|(existing, _)| *existing == net_id) {
                    continue;
                }
                cap_nets.push((net_id, pin));
            }
            if cap_nets.len() != 2 {
                continue;
            }
            let (first, second) = (cap_nets[0], cap_nets[1]);
            // Whichever side is the rail is the side worth clustering
            // around; the other must be ground, or this is a signal
            // coupling cap and not a decoupling cap at all.
            let first_net = board.net(first.0);
            let second_net = board.net(second.0);
            let (Some(first_net), Some(second_net)) = (first_net, second_net) else {
                continue;
            };
            let (rail_net_id, member_pin) =
                if net_is_rail(board, first_net) && net_is_ground(board, second_net) {
                    (first.0, first.1)
                } else if net_is_rail(board, second_net) && net_is_ground(board, first_net) {
                    (second.0, second.1)
                } else {
                    continue;
                };
            let Some(rail_net) = board.net(rail_net_id) else {
                continue;
            };

            // Heaviest consumer wins; declaration adjacency breaks ties
            // (lower adjacency distance = nearer in source = preferred).
            let mut best: Option<(usize, usize, usize, PinId)> = None;
            for &(anchor, cluster_idx) in &anchors {
                if anchor == component.id {
                    continue;
                }
                // A declared `group` is a hard boundary: adopting across
                // one would place the cap in another group's region.
                if board.component(anchor).and_then(|c| c.group.as_deref())
                    != component.group.as_deref()
                {
                    continue;
                }
                let anchor_pin = rail_net
                    .endpoints
                    .iter()
                    .find(|ep| ep.component == anchor)
                    .map(|ep| ep.pin);
                let Some(anchor_pin) = anchor_pin else {
                    continue;
                };
                let pins_on_rail = rail_net
                    .endpoints
                    .iter()
                    .filter(|ep| ep.component == anchor)
                    .count();
                let adjacency = u64::from(anchor.0).abs_diff(u64::from(component.id.0)) as usize;
                let key = (pins_on_rail, usize::MAX - adjacency);
                if best.is_none_or(|(best_pins, best_adj, _, _)| key > (best_pins, best_adj)) {
                    best = Some((
                        pins_on_rail,
                        usize::MAX - adjacency,
                        cluster_idx,
                        anchor_pin,
                    ));
                }
            }
            let Some((_, _, cluster_idx, anchor_pin)) = best else {
                continue;
            };
            self.clusters[cluster_idx].members.push(ClusterMember {
                component: component.id,
                role: MemberRole::OrphanRailCap,
                binding: MemberBinding {
                    net: rail_net.id,
                    anchor_pin,
                    member_pin,
                },
            });
            self.clusters[cluster_idx]
                .members
                .sort_by_key(|m| m.component.0);
            self.claimed.insert(component.id);
        }
    }

    // ----- Group boundary --------------------------------------------------

    /// A declared `group` bounds cluster membership: drop any member
    /// whose group differs from its anchor's.
    ///
    /// `passive_endpoints` already skips parts in another group, so this
    /// is a backstop for the passes that walk nets themselves (dividers,
    /// orphan rail caps). Without it an I²C pull-up declared inside a
    /// sensor's group could be claimed by an MCU two groups away.
    /// Placement then puts it in the *anchor's* region while the group
    /// box still measures it as part of its own, stretching that box
    /// across the sheet and overlapping every other one. Ungrouped
    /// boards are unaffected: every group is `None`, so nothing is
    /// ever evicted.
    fn evict_cross_group_members(&mut self, board: &Board) {
        let group_of = |id: ComponentId| -> Option<String> {
            board.component(id).and_then(|c| c.group.clone())
        };
        let mut evicted: Vec<ComponentId> = Vec::new();
        for cluster in &mut self.clusters {
            let anchor_group = group_of(cluster.anchor);
            cluster.members.retain(|m| {
                if group_of(m.component) == anchor_group {
                    true
                } else {
                    evicted.push(m.component);
                    false
                }
            });
        }
        // Evicted members become their own single-part clusters, in id
        // order so the result stays deterministic.
        evicted.sort_by_key(|id| id.0);
        evicted.dedup();
        for id in evicted {
            self.clusters.push(FunctionalCluster {
                kind: ClusterKind::Singleton,
                anchor: id,
                members: Vec::new(),
            });
        }
    }

    // ----- Pass 9: singletons ----------------------------------------------

    /// Every still-unclaimed component becomes a cluster of its own.
    fn singletons(&mut self, board: &Board) {
        for component in &board.components {
            if self.claimed.contains(&component.id) {
                continue;
            }
            self.clusters.push(FunctionalCluster {
                kind: ClusterKind::Singleton,
                anchor: component.id,
                members: Vec::new(),
            });
            self.claimed.insert(component.id);
        }
    }
}

// ----- Recognition helpers --------------------------------------------------

/// Every `(net, (component, pin))` pair sharing a net with
/// `anchor_id.anchor_pin`, in net-then-endpoint order, skipping the
/// anchor itself, anything already claimed and anything in another group
/// than the anchor: such a part would be evicted from the cluster anyway,
/// and skipping it here leaves it for an anchor in its own group.
///
/// This is the traversal the pin-anchored passes use, so "what does this
/// pin touch" is answered one way for them.
fn passive_endpoints(
    board: &Board,
    anchor_id: ComponentId,
    anchor_pin: PinId,
    claimed: &HashSet<ComponentId>,
) -> Vec<(NetId, (ComponentId, PinId))> {
    let mut out = Vec::new();
    let group_of = |id: ComponentId| board.component(id).map(|c| &c.group);
    for (net_id, net) in board.nets_containing(anchor_id, anchor_pin) {
        for endpoint in &net.endpoints {
            if endpoint.component == anchor_id
                || claimed.contains(&endpoint.component)
                || group_of(endpoint.component) != group_of(anchor_id)
            {
                continue;
            }
            out.push((net_id, (endpoint.component, endpoint.pin)));
        }
    }
    out
}

/// The part's kind, or `None` for a component with no resolved part.
///
/// Takes an `Option` so call sites can pass `board.component(id)`
/// directly without unwrapping first.
fn part_kind(component: Option<&crate::board::Component>) -> Option<&str> {
    component
        .and_then(|c| c.part.as_ref())
        .map(|p| p.kind.as_str())
}

fn is_led(component: &crate::board::Component) -> bool {
    component
        .part
        .as_ref()
        .is_some_and(|p| p.kind == "led" || p.id.as_str().starts_with("led_"))
}

/// Which side of a drawn IC body a pin belongs to.
///
/// Ports the schematic placer's pin-layout heuristic so recognition and
/// drawing agree on where a pin is. Ground names sit on the bottom edge,
/// any other power pin on the top edge, reset/boot/clock/RF pins and
/// outputs on the right, and everything else on the left.
fn pin_side(pin: &synth_registry::Pin) -> PinSide {
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
    if pin.electrical_type == ElectricalType::Output {
        return PinSide::Right;
    }
    PinSide::Left
}

/// Whether an IC pin belongs to a drawn side of the body. Power pins sit
/// on the top and bottom edges and are skipped when hunting for pull
/// resistors: a pull belongs to a signal pin.
/// Which side of a drawn IC body a pin belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinSide {
    Left,
    Right,
    Top,
    Bottom,
}

fn is_side_pin(pin: &synth_registry::Pin) -> bool {
    matches!(pin_side(pin), PinSide::Left | PinSide::Right)
}

/// Whether a two-pin resistor is pulling its other net to a rail, making
/// it a pull-up or pull-down on the signal net that shares this IC pin.
///
/// The rail side is recognized two ways: by the net's inferred power
/// domain (a rail or ground whatever its pins are called), and by pin
/// *name*: a design that labels its rails `3v3`, `5v`, or `vout` is
/// expressing intent that electrical type alone does not carry — a
/// resistor tied to a mid-voltage divider node has no power-typed pin
/// on it at all.
fn resistor_pulls_to_rail(
    board: &Board,
    domains: &PowerDomainMap,
    resistor_id: ComponentId,
) -> bool {
    for pin_idx in 0..2_u32 {
        for (net_id, net) in board.nets_containing(resistor_id, PinId(pin_idx)) {
            // A net that carries a power or ground pin is a rail whatever
            // its pins are called.
            if domains
                .get(net_id)
                .is_some_and(|d| d.is_rail() || d.is_ground())
            {
                return true;
            }
            for endpoint in &net.endpoints {
                let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
                    continue;
                };
                if matches!(
                    pin.name.to_ascii_lowercase().as_str(),
                    "vcc" | "vdd" | "gnd" | "vss" | "vbus" | "vin" | "vout" | "3v3" | "5v"
                ) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether `resistor_id` has a pin other than the one on `signal_net`
/// whose net touches a power pin — i.e. it pulls that line to a rail.
fn is_pullup_to_power(board: &Board, resistor_id: ComponentId, signal_net: NetId) -> bool {
    for pin_idx in 0..2_u32 {
        for (_net_id, net) in board.nets_containing(resistor_id, PinId(pin_idx)) {
            if net.id == signal_net {
                continue;
            }
            let touches_rail = net.endpoints.iter().any(|ep| {
                ep.component != resistor_id
                    && board.pin(ep.component, ep.pin).is_some_and(|p| {
                        matches!(
                            p.electrical_type,
                            ElectricalType::PowerInput | ElectricalType::PowerOutput
                        )
                    })
            });
            if touches_rail {
                return true;
            }
        }
    }
    false
}

/// Whether a pin name denotes a ground reference (`gnd`, `vss`, …).
fn is_ground_pin_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "gnd" | "vss" | "vssa" | "gnda" | "gnd_a" | "vee" | "vneg" | "agnd" | "dgnd" | "ground"
    ) || lower.starts_with("gnd")
        || lower.starts_with("vss")
}

/// Whether a net is a supply rail: it carries a power-output pin, or a
/// non-ground power-input pin (a regulator input, an MCU `vdd`).
fn net_is_rail(board: &Board, net: &Net) -> bool {
    net.endpoints.iter().any(|ep| {
        board.pin(ep.component, ep.pin).is_some_and(|p| {
            matches!(p.electrical_type, ElectricalType::PowerOutput)
                || (matches!(p.electrical_type, ElectricalType::PowerInput)
                    && !is_ground_pin_name(&p.name))
        })
    })
}

/// Whether a net is a ground: it touches a ground-named power pin.
fn net_is_ground(board: &Board, net: &Net) -> bool {
    net.endpoints.iter().any(|ep| {
        board.pin(ep.component, ep.pin).is_some_and(|p| {
            matches!(
                p.electrical_type,
                ElectricalType::PowerInput | ElectricalType::GroundReference
            ) && is_ground_pin_name(&p.name)
        })
    })
}

/// Recognize one divider anchored at `anchor_id`, returning the mid net,
/// both pins meeting on it, and the partner resistor.
fn recognize_divider(
    board: &Board,
    claimed: &HashSet<ComponentId>,
    anchor_id: ComponentId,
) -> Option<(NetId, PinId, PinId, ComponentId)> {
    for mid_pin in 0..2_u32 {
        let rail_pin = PinId(1 - mid_pin);
        let mid_pin_id = PinId(mid_pin);

        // The candidate pin must sit on the divider's mid net: exactly
        // two endpoints, both resistors (this one plus its partner).
        // Either pin could be the mid node, so a miss here only rules
        // out this orientation.
        let Some(mid_net) = board
            .nets_containing(anchor_id, mid_pin_id)
            .find(|(_, n)| is_two_resistor_mid_net(board, n, anchor_id))
            .map(|(net_id, _)| net_id)
        else {
            continue;
        };

        // The other pin must reach a non-resistor source, or both sides
        // are resistors and this is not a rail→mid→ground divider.
        let has_rail = board.nets_containing(anchor_id, rail_pin).any(|(_, n)| {
            n.endpoints.iter().any(|ep| {
                ep.component != anchor_id
                    && board
                        .component(ep.component)
                        .and_then(|c| c.part.as_ref())
                        .is_some_and(|p| p.kind != "resistor")
            })
        });
        if !has_rail {
            continue;
        }

        let Some(net) = board.net(mid_net) else {
            continue;
        };
        let partner = net.endpoints.iter().find(|ep| ep.component != anchor_id)?;
        if claimed.contains(&partner.component) {
            // A claimed partner means this orientation belongs to
            // another divider; try the anchor's other pin before giving
            // up, since either pin could be the mid node.
            continue;
        }
        return Some((mid_net, mid_pin_id, partner.pin, partner.component));
    }
    None
}

/// Whether `net` is a divider's mid net: exactly two endpoints, both
/// resistors, one of them `anchor_id`.
fn is_two_resistor_mid_net(board: &Board, net: &Net, anchor_id: ComponentId) -> bool {
    if net.endpoints.len() != 2 {
        return false;
    }
    net.endpoints.iter().any(|ep| ep.component == anchor_id)
        && net.endpoints.iter().all(|ep| {
            board
                .component(ep.component)
                .and_then(|c| c.part.as_ref())
                .is_some_and(|p| p.kind == "resistor")
        })
}

/// Index every cluster by the components it owns: its anchor and its
/// members. Used by callers that need to resolve a net endpoint back to
/// the cluster that contains it.
#[must_use]
pub fn cluster_index_by_component(clusters: &[FunctionalCluster]) -> HashMap<ComponentId, usize> {
    let mut map = HashMap::new();
    for (idx, cluster) in clusters.iter().enumerate() {
        map.insert(cluster.anchor, idx);
        for member in &cluster.members {
            map.insert(member.component, idx);
        }
    }
    map
}

#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::implicit_hasher)]
mod tests {
    use super::*;
    use crate::test_support::{
        component, net, part, part_with_decoupling, passive_pin, pin, pin_with, plain_board,
    };

    fn find(clusters: &[FunctionalCluster], anchor: u32) -> &FunctionalCluster {
        clusters
            .iter()
            .find(|c| c.anchor.0 == anchor)
            .unwrap_or_else(|| panic!("no cluster anchored at component {anchor}"))
    }

    fn member_of(cluster: &FunctionalCluster, id: u32) -> &ClusterMember {
        cluster
            .members
            .iter()
            .find(|m| m.component.0 == id)
            .unwrap_or_else(|| panic!("cluster {} has no member {id}", cluster.anchor.0))
    }

    /// The binding is the whole reason this module exists: a decoupling
    /// cap must say *which* rail pin it decouples, or a placer can only
    /// guess "somewhere near the chip".
    #[test]
    fn a_decoupling_cap_binds_to_the_rail_pin_it_decouples() {
        let u1 = component(
            0,
            "U1",
            part_with_decoupling(
                "mcu",
                vec![
                    pin("dvdd", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                    pin("swdio", ElectricalType::Bidirectional),
                ],
                &[("dvdd", 1)],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![u1, c1],
            vec![
                net(0, "3V3", &[(0, 0), (1, 0)]),
                net(1, "GND", &[(0, 1), (1, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let ic = find(&clusters, 0);
        assert_eq!(ic.kind, ClusterKind::IcBlock);
        let cap = member_of(ic, 1);
        assert_eq!(cap.role, MemberRole::DecouplingCap);
        assert_eq!(cap.binding.net, NetId(0));
        assert_eq!(cap.binding.anchor_pin, PinId(0), "U1's dvdd pad");
        assert_eq!(cap.binding.member_pin, PinId(0), "C1's rail-side pad");
    }

    /// A rail is a rail because the design declared it (`power "VIO" 3.3v`),
    /// not because a pin happens to be named `vcc`: a resistor from a signal
    /// pin to a declared rail is a pull-up, whatever the rail's pins are
    /// called.
    #[test]
    fn a_pull_resistor_to_a_declared_rail_is_a_pull_up() {
        let u1 = component(
            0,
            "U1",
            part_with_decoupling(
                "ic",
                vec![
                    pin("dvdd", ElectricalType::PowerInput),
                    pin("vio_in", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                    pin("mdm", ElectricalType::Bidirectional),
                ],
                &[("dvdd", 1)],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let mut vio = net(1, "VIO", &[(0, 1), (1, 1)]);
        vio.voltage = Some(crate::units::Voltage::from_v(3.3));
        let b = plain_board(
            vec![u1, r1],
            vec![
                net(0, "MODEM", &[(0, 3), (1, 0)]),
                vio,
                net(2, "3V3", &[(0, 0)]),
                net(3, "GND", &[(0, 2)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let ic = find(&clusters, 0);
        let r = member_of(ic, 1);
        assert_eq!(r.role, MemberRole::PullUp);
        assert_eq!(r.binding.anchor_pin, PinId(3), "the pin it pulls, U1.mdm");
    }

    /// A resistor on a signal that two ICs share belongs to the IC in its
    /// own group. The earlier IC used to claim it, and the layout then
    /// evicted it for sitting in another group than its anchor, leaving it
    /// loose on the sheet.
    #[test]
    fn a_pull_resistor_goes_to_the_ic_in_its_own_group() {
        let ic = |id: u32, name: &str, group: &str| {
            let mut c = component(
                id,
                name,
                part_with_decoupling(
                    "ic",
                    vec![
                        pin("dvdd", ElectricalType::PowerInput),
                        pin("gnd", ElectricalType::GroundReference),
                        pin("mdm", ElectricalType::Bidirectional),
                    ],
                    &[("dvdd", 1)],
                ),
            );
            c.group = Some(group.to_string());
            c
        };
        let mut r1 = component(
            2,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        r1.group = Some("B".to_string());
        let b = plain_board(
            vec![ic(0, "U1", "A"), ic(1, "U3", "B"), r1],
            vec![
                net(0, "MODEM", &[(0, 2), (1, 2), (2, 0)]),
                net(1, "GND", &[(0, 1), (1, 1), (2, 1)]),
                net(2, "3V3", &[(0, 0), (1, 0)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        assert!(
            find(&clusters, 0)
                .members
                .iter()
                .all(|m| m.component.0 != 2),
            "U1 is in another group"
        );
        assert_eq!(member_of(find(&clusters, 1), 2).role, MemberRole::PullUp);
    }

    /// A resistor in series on an IC signal pin (the other end goes to a
    /// connector, not a rail) is part of the IC's cluster, bound to the pin
    /// it sits on, so it can be drawn beside that pin.
    #[test]
    fn a_series_resistor_on_a_signal_pin_joins_the_ics_cluster() {
        let u1 = component(
            0,
            "U1",
            part_with_decoupling(
                "ic",
                vec![
                    pin("dvdd", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                    pin("txd", ElectricalType::Bidirectional),
                ],
                &[("dvdd", 1)],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let j1 = component(
            2,
            "J1",
            part("connector", vec![pin("p1", ElectricalType::Passive)]),
        );
        let b = plain_board(
            vec![u1, r1, j1],
            vec![
                net(0, "TXD", &[(0, 2), (1, 0)]),
                net(1, "TXD_OUT", &[(1, 1), (2, 0)]),
                net(2, "GND", &[(0, 1)]),
                net(3, "3V3", &[(0, 0)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let r = member_of(find(&clusters, 0), 1);
        assert_eq!(r.role, MemberRole::SignalSeries);
        assert_eq!(r.binding.anchor_pin, PinId(2), "the pin it sits on, U1.txd");
    }

    /// A resistor in series on a connector pin (the other end goes to an IC,
    /// not a rail) is part of the connector's cluster, bound to the pin it
    /// sits on, so it can be drawn beside that pin.
    #[test]
    fn a_series_resistor_on_a_connector_pin_joins_the_connectors_cluster() {
        let j1 = component(
            0,
            "J1",
            part(
                "connector",
                vec![passive_pin("p1"), passive_pin("p2"), passive_pin("p3")],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let u1 = component(
            2,
            "U1",
            part("ic", vec![pin("rxd", ElectricalType::Bidirectional)]),
        );
        let b = plain_board(
            vec![j1, r1, u1],
            vec![
                net(0, "RXD", &[(0, 2), (1, 1)]),
                net(1, "RXD_MCU", &[(1, 0), (2, 0)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let j = find(&clusters, 0);
        assert_eq!(j.kind, ClusterKind::ConnectorBlock);
        let r = member_of(j, 1);
        assert_eq!(r.role, MemberRole::SignalSeries);
        assert_eq!(r.binding.anchor_pin, PinId(2), "the pin it sits on, J1.p3");
        assert_eq!(r.binding.member_pin, PinId(1));
    }

    /// A resistor that pulls a connector pin to a rail is not in series on
    /// the signal, so the connector pass leaves it alone.
    #[test]
    fn a_pull_resistor_on_a_connector_pin_is_not_a_series_member() {
        let j1 = component(
            0,
            "J1",
            part("connector", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let r1 = component(
            1,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let u1 = component(
            2,
            "U1",
            part("ic", vec![pin("gnd", ElectricalType::GroundReference)]),
        );
        let b = plain_board(
            vec![j1, r1, u1],
            vec![
                net(0, "SIG", &[(0, 0), (1, 0)]),
                net(1, "GND", &[(1, 1), (2, 0)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        assert_eq!(find(&clusters, 0).kind, ClusterKind::Singleton);
        assert!(clusters
            .iter()
            .flat_map(|c| &c.members)
            .all(|m| m.role != MemberRole::SignalSeries));
    }

    /// A connector with no series resistor on it stays a singleton, so a
    /// small board with only plain headers is not regrouped.
    #[test]
    fn a_connector_with_no_series_resistor_stays_a_singleton() {
        let j1 = component(
            0,
            "J1",
            part("connector", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let u1 = component(
            1,
            "U1",
            part("ic", vec![pin("io", ElectricalType::Bidirectional)]),
        );
        let b = plain_board(vec![j1, u1], vec![net(0, "IO", &[(0, 0), (1, 0)])]);
        let clusters = recognize_clusters(&b);
        assert_eq!(find(&clusters, 0).kind, ClusterKind::Singleton);
    }

    /// A USB-C connector's CC pins carry Rd pull-downs to ground. They belong
    /// beside the connector's CC pins, not loose on the sheet.
    #[test]
    fn a_cc_pull_down_on_a_usb_connector_joins_the_connectors_cluster() {
        use synth_registry::PinCapability as Cap;
        let j1 = component(
            0,
            "J1",
            part(
                "connector",
                vec![
                    pin_with("dp", ElectricalType::Bidirectional, &[Cap::UsbDp]),
                    pin_with("dn", ElectricalType::Bidirectional, &[Cap::UsbDn]),
                    pin_with("cc1", ElectricalType::Bidirectional, &[Cap::UsbCc]),
                    pin("gnd", ElectricalType::GroundReference),
                ],
            ),
        );
        let r1 = component(
            1,
            "R1",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![j1, r1],
            vec![
                net(0, "CC1", &[(0, 2), (1, 0)]),
                net(1, "GND", &[(0, 3), (1, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let usb = find(&clusters, 0);
        assert_eq!(usb.kind, ClusterKind::UsbEsd);
        let r = member_of(usb, 1);
        assert_eq!(r.role, MemberRole::PullUp);
        assert_eq!(r.binding.anchor_pin, PinId(2), "the CC pin");
    }

    /// A part belongs to exactly one cluster. Two `required_decoupling`
    /// rules that reach the same capacitor must claim it once.
    #[test]
    fn a_cap_reachable_from_two_rails_is_claimed_once() {
        let u1 = component(
            0,
            "U1",
            part_with_decoupling(
                "mcu",
                vec![
                    pin("vdd", ElectricalType::PowerInput),
                    pin("vdd", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
                &[("vdd", 1), ("vdd", 1)],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![u1, c1],
            vec![
                net(0, "3V3", &[(0, 0), (0, 1), (1, 0)]),
                net(1, "GND", &[(0, 2), (1, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let claims = clusters
            .iter()
            .flat_map(|c| c.members.iter())
            .filter(|m| m.component.0 == 1)
            .count();
        assert_eq!(claims, 1, "C1 must be claimed exactly once");
    }

    /// The IC block pass takes precedence over the I²C pass: an I²C host
    /// is an IC that must keep its own decoupling and reset network.
    #[test]
    fn an_i2c_host_keeps_its_own_decoupling() {
        let mut host_pins = vec![
            pin("vdd", ElectricalType::PowerInput),
            pin("gnd", ElectricalType::GroundReference),
        ];
        host_pins[0].capabilities = vec![PinCapability::I2cSda];
        host_pins[1].capabilities = vec![PinCapability::I2cScl];
        // I2C pins need signal types for the side-pin classification.
        let mut host = component(
            0,
            "U1",
            part_with_decoupling("mcu", host_pins, &[("vdd", 1)]),
        );
        host.part
            .as_mut()
            .expect("part")
            .pins
            .push(pin("sda", ElectricalType::Bidirectional));
        host.part
            .as_mut()
            .expect("part")
            .pins
            .push(pin("scl", ElectricalType::Bidirectional));
        host.part.as_mut().expect("part").pins[2].capabilities = vec![PinCapability::I2cSda];
        host.part.as_mut().expect("part").pins[3].capabilities = vec![PinCapability::I2cScl];

        let decap = component(
            2,
            "C2",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let sda_pull = component(
            3,
            "R3",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let scl_pull = component(
            4,
            "R4",
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![host, decap, sda_pull, scl_pull],
            vec![
                net(0, "3V3", &[(0, 0), (2, 0), (3, 1), (4, 1)]),
                net(1, "GND", &[(0, 1), (2, 1)]),
                net(2, "SDA", &[(0, 2), (3, 0)]),
                net(3, "SCL", &[(0, 3), (4, 0)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        assert_eq!(
            find(&clusters, 0).kind,
            ClusterKind::IcBlock,
            "the IC pass claims the host before the I2C pass can"
        );
        assert_eq!(
            clusters
                .iter()
                .filter(|c| c.kind == ClusterKind::I2cBus)
                .count(),
            0,
            "no I2C cluster is left once the host is claimed"
        );
    }

    /// A decoupling cap on a rail shared by several parts is reachable
    /// from no single anchor's declared rules, and used to be left to
    /// the singleton sweep — over 150 mm from the part it decouples.
    #[test]
    fn a_rail_cap_is_adopted_by_its_heaviest_consumer() {
        let regulator = component(
            0,
            "U1",
            part(
                "regulator",
                vec![
                    pin("vin", ElectricalType::PowerInput),
                    pin("vout", ElectricalType::PowerOutput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
            ),
        );
        let mcu = component(
            1,
            "U2",
            part_with_decoupling(
                "mcu",
                vec![
                    pin("vdd", ElectricalType::PowerInput),
                    pin("vdd", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
                &[("vdd", 1)],
            ),
        );
        let declared = component(
            2,
            "C2",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let mcu_decap = component(
            3,
            "C3",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let orphan = component(
            4,
            "C4",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![regulator, mcu, declared, mcu_decap, orphan],
            vec![
                net(0, "VIN", &[(0, 0)]),
                net(1, "3V3", &[(0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (4, 0)]),
                net(2, "GND", &[(0, 2), (1, 2), (2, 1), (3, 1), (4, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        // The regulator's rail allowance takes C2, the MCU's declared
        // rule takes C3, and C4 is reachable from no declared rule.
        assert_eq!(member_of(find(&clusters, 0), 2).role, MemberRole::RailCap);
        let mcu_cluster = find(&clusters, 1);
        assert_eq!(mcu_cluster.kind, ClusterKind::IcBlock);
        assert_eq!(member_of(mcu_cluster, 3).role, MemberRole::DecouplingCap);
        // The MCU draws two pins from the rail and the regulator one, so
        // the leftover cap belongs to the MCU.
        let adopted = member_of(mcu_cluster, 4);
        assert_eq!(adopted.role, MemberRole::OrphanRailCap);
        assert_eq!(adopted.binding.net, NetId(1), "bound through the rail net");
        assert_eq!(adopted.binding.anchor_pin, PinId(0));
    }

    /// A declared group is a hard membership boundary: adoption across
    /// one would place the cap in another group's region.
    #[test]
    fn a_group_boundary_stops_cap_adoption() {
        let mut reg = component(
            0,
            "U1",
            part_with_decoupling(
                "regulator",
                vec![
                    pin("vin", ElectricalType::PowerInput),
                    pin("vout", ElectricalType::PowerOutput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
                &[("vout", 1)],
            ),
        );
        reg.group = Some("power".to_string());
        let mut mcu = component(
            1,
            "U2",
            part_with_decoupling(
                "mcu",
                vec![
                    pin("vdd", ElectricalType::PowerInput),
                    pin("vdd", ElectricalType::PowerInput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
                &[("vdd", 1)],
            ),
        );
        mcu.group = Some("logic".to_string());
        let orphan = component(
            2,
            "C2",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![reg, mcu, orphan],
            vec![
                net(0, "3V3", &[(0, 1), (1, 0), (1, 1), (2, 0)]),
                net(1, "GND", &[(0, 2), (1, 2), (2, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        assert!(
            !find(&clusters, 1)
                .members
                .iter()
                .any(|m| m.component.0 == 2),
            "the MCU is in another group, so the cap must not be adopted there"
        );
    }

    /// A crystal and the capacitors loading each of its pins.
    #[test]
    fn a_crystal_claims_its_load_caps() {
        let crystal = component(
            0,
            "Y1",
            part(
                "crystal",
                vec![
                    pin("osc1", ElectricalType::Passive),
                    pin("osc2", ElectricalType::Passive),
                ],
            ),
        );
        let c1 = component(
            1,
            "C1",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let c2 = component(
            2,
            "C2",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![crystal, c1, c2],
            vec![
                net(0, "OSC1", &[(0, 0), (1, 0)]),
                net(1, "OSC2", &[(0, 1), (2, 0)]),
                net(2, "GND", &[(1, 1), (2, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let xtal = find(&clusters, 0);
        assert_eq!(xtal.kind, ClusterKind::Crystal);
        assert_eq!(xtal.members.len(), 2);
        assert_eq!(member_of(xtal, 1).binding.anchor_pin, PinId(0));
        assert_eq!(member_of(xtal, 2).binding.anchor_pin, PinId(1));
    }

    /// The regulator claims at most its declared allowance per rail, so
    /// it cannot absorb the downstream parts' decoupling as its own.
    #[test]
    fn a_regulator_claims_only_its_declared_rail_caps() {
        let reg = component(
            0,
            "U1",
            part_with_decoupling(
                "regulator",
                vec![
                    pin("vin", ElectricalType::PowerInput),
                    pin("vout", ElectricalType::PowerOutput),
                    pin("gnd", ElectricalType::GroundReference),
                ],
                &[("vout", 1)],
            ),
        );
        let c_in = component(
            1,
            "C1",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let c_out_a = component(
            2,
            "C2",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let c_out_b = component(
            3,
            "C3",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![reg, c_in, c_out_a, c_out_b],
            vec![
                net(0, "VIN", &[(0, 0), (1, 0)]),
                net(1, "3V3", &[(0, 1), (2, 0), (3, 0)]),
                net(2, "GND", &[(0, 2), (1, 1), (2, 1), (3, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let ldo = find(&clusters, 0);
        assert_eq!(ldo.kind, ClusterKind::LdoBlock);
        let rail_caps: Vec<u32> = ldo
            .members
            .iter()
            .filter(|m| m.role == MemberRole::RailCap)
            .map(|m| m.component.0)
            .collect();
        assert_eq!(
            rail_caps,
            vec![1, 2],
            "one cap per rail: the input cap, and only one of the two output caps"
        );
        // The surplus output cap is not silently relabelled as the
        // regulator's; if it joins the LDO at all, it does so explicitly
        // as an adopted rail cap.
        if let Some(surplus) = ldo.members.iter().find(|m| m.component.0 == 3) {
            assert_eq!(surplus.role, MemberRole::OrphanRailCap);
        }
    }

    /// An RF matching network is recognized for the board, where the
    /// network belongs beside its antenna.
    #[test]
    fn an_antenna_claims_its_matching_network() {
        let antenna = component(
            0,
            "AE1",
            part("antenna", vec![pin("feed", ElectricalType::Passive)]),
        );
        let series = component(
            1,
            "L1",
            part("inductor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let shunt = component(
            2,
            "C1",
            part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
        );
        let b = plain_board(
            vec![antenna, series, shunt],
            vec![
                net(0, "FEED", &[(0, 0), (1, 0), (2, 0)]),
                net(1, "GND", &[(1, 1), (2, 1)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let rf = find(&clusters, 0);
        assert_eq!(rf.kind, ClusterKind::RfMatching);
        assert_eq!(rf.members.len(), 2);
        assert!(rf
            .members
            .iter()
            .all(|m| m.role == MemberRole::RfMatchElement));
    }

    /// Every component ends up in exactly one cluster, so no consumer
    /// can place a part twice or leave it out.
    #[test]
    fn every_component_lands_in_exactly_one_cluster() {
        let parts = vec![
            component(
                0,
                "J1",
                part(
                    "connector",
                    vec![
                        pin("vbus", ElectricalType::PowerOutput),
                        pin("dp", ElectricalType::Output),
                        pin("dn", ElectricalType::Output),
                        pin("gnd", ElectricalType::GroundReference),
                    ],
                ),
            ),
            component(
                1,
                "U1",
                part_with_decoupling(
                    "mcu",
                    vec![
                        pin("vdd", ElectricalType::PowerInput),
                        pin("gnd", ElectricalType::GroundReference),
                    ],
                    &[("vdd", 2)],
                ),
            ),
            component(
                2,
                "C1",
                part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
            ),
            component(
                3,
                "C2",
                part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
            ),
            component(
                4,
                "D1",
                part("diode", vec![passive_pin("a"), passive_pin("k")]),
            ),
        ];
        let b = plain_board(
            parts,
            vec![
                net(0, "3V3", &[(0, 0), (1, 0), (2, 0), (3, 0)]),
                net(1, "GND", &[(0, 3), (1, 1), (2, 1), (3, 1), (4, 1)]),
                net(2, "DP", &[(0, 1), (4, 0)]),
                net(3, "DN", &[(0, 2)]),
            ],
        );
        let clusters = recognize_clusters(&b);
        let mut counts: HashMap<u32, usize> = HashMap::new();
        for cluster in &clusters {
            *counts.entry(cluster.anchor.0).or_default() += 1;
            for member in &cluster.members {
                *counts.entry(member.component.0).or_default() += 1;
            }
        }
        for id in 0..5 {
            assert_eq!(counts.get(&id), Some(&1), "component {id} ownership");
        }
    }

    /// Recognition is a pure function of the board, so two consumers
    /// reading it independently cannot disagree.
    #[test]
    fn recognition_is_deterministic() {
        let b = plain_board(
            vec![
                component(
                    0,
                    "U1",
                    part_with_decoupling(
                        "mcu",
                        vec![
                            pin("vdd", ElectricalType::PowerInput),
                            pin("gnd", ElectricalType::GroundReference),
                        ],
                        &[("vdd", 1)],
                    ),
                ),
                component(
                    1,
                    "C1",
                    part("capacitor", vec![passive_pin("p1"), passive_pin("p2")]),
                ),
            ],
            vec![
                net(0, "3V3", &[(0, 0), (1, 0)]),
                net(1, "GND", &[(0, 1), (1, 1)]),
            ],
        );
        assert_eq!(recognize_clusters(&b), recognize_clusters(&b));
    }
}
