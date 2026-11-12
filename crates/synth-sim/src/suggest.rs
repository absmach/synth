// SPDX-License-Identifier: Apache-2.0

//! Draft a testbench from the circuit shapes in a compiled board.
//!
//! The shapes are deliberately few (`led` + series resistor, resistor divider,
//! RC low-pass). Every emitted limit cites a design `value` or a registry
//! field, and a missing supply becomes a `# TODO`, never a guessed number.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use synth_ir::PowerDomainKind;
use synth_ir::{infer_power_domains, Board, Component, ComponentId, Net, NetId, PinId};
use synth_registry::{ElectricalType, PinDrive};
use synth_validate::{parse_capacitance, parse_resistance};

/// A drafted `<design>.sim.toml`, plus everything the draft could not cover.
#[derive(Debug, Clone)]
pub struct TestbenchSuggestion {
    /// `<design>.sim.toml` text, or `None` when no measure could be derived.
    pub toml: Option<String>,
    /// Human-readable reasons parts/shapes were left out.
    pub not_covered: Vec<String>,
    /// Problems the draft worked around, such as an overdriven LED whose
    /// measure is deliberately pinned to the current cap.
    pub warnings: Vec<String>,
}

/// Draft a testbench from the shapes in `design`.
pub fn suggest_testbench(design: &Board) -> TestbenchSuggestion {
    let domains = infer_power_domains(design);
    let grounds: Vec<&Net> = design
        .nets
        .iter()
        .filter(|net| domains.get(net.id).is_some_and(PowerDomainKind::is_ground))
        .collect();
    if grounds.len() != 1 {
        let reason = if grounds.is_empty() {
            "no net is classified as ground; declare a ground net named GND".to_string()
        } else {
            let names: Vec<&str> = grounds.iter().map(|net| net.name.as_str()).collect();
            format!(
                "several nets are classified as ground ({}); a testbench needs exactly one",
                names.join(", ")
            )
        };
        return TestbenchSuggestion {
            toml: None,
            not_covered: vec![reason],
            warnings: Vec::new(),
        };
    }
    let ground = grounds[0];
    if has_line_break(&ground.name) {
        return TestbenchSuggestion {
            toml: None,
            not_covered: vec!["the ground net name contains a newline".to_string()],
            warnings: Vec::new(),
        };
    }

    let mut draft = Draft::new(design, ground.id);
    draft.detect_leds();
    draft.detect_dividers();
    draft.detect_rcs();
    draft.report_unused();
    // A retained modelled regulator needs its input rail driven; add that
    // source before the dead-rail sweep so its decoupling caps stay live.
    draft.source_regulator_inputs();
    // `report_unused` is what ignores a connector; run the dead-rail sweep
    // after it so the cap sees every ignored neighbour.
    draft.ignore_orphan_passives();
    let toml = draft.render().and_then(|text| {
        // The generated file is a trust boundary too: never write something
        // the loader would reject. This catches a name or value that escaped
        // the per-field guards.
        match crate::Testbench::from_toml(&text) {
            Ok(_) => Some(text),
            Err(diagnostic) => {
                draft.not_covered.push(format!(
                    "the drafted testbench was rejected by the loader: {}",
                    diagnostic.message.as_deref().unwrap_or("unknown error")
                ));
                None
            }
        }
    });
    TestbenchSuggestion {
        toml,
        not_covered: draft.not_covered,
        warnings: draft.warnings,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnalysisKind {
    Op,
    Ac,
}

/// The declared or pin-derived voltage that drives a net.
#[derive(Debug, Clone)]
struct Supply {
    volts: f64,
    /// The `# source:` comment, naming where the number came from.
    note: String,
    /// Power-output parts whose pin supplied the net; replaced by the source.
    drivers: Vec<ComponentId>,
    /// Set when the supply is an IC output pin driven at its high level.
    pin_drive: Option<PinSupply>,
}

/// A pin whose `[pins.drive]` data supplies a net in the draft.
#[derive(Debug, Clone)]
struct PinSupply {
    /// `REF.PIN`, e.g. `U1.gpio5`.
    pin: String,
    drive: PinDrive,
    datasheet_ref: String,
}

#[derive(Debug)]
struct DraftMeasure {
    analysis: AnalysisKind,
    name: String,
    kind: &'static str,
    node: Option<String>,
    element: Option<String>,
    /// `REF.PIN` for a `pin_current` measure.
    pin: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    at_frequency: Option<f64>,
    comment: String,
    supply_net: NetId,
    supply: Supply,
    /// `fc/100 .. fc*100` for an RC measure.
    sweep: Option<(f64, f64)>,
}

#[derive(Debug)]
struct Draft<'a> {
    design: &'a Board,
    ground: NetId,
    used: HashSet<ComponentId>,
    not_covered: Vec<String>,
    warnings: Vec<String>,
    todo_nets: BTreeSet<NetId>,
    names: HashSet<String>,
    measures: Vec<DraftMeasure>,
    /// Rails that must be driven for a retained model but are not a measure's
    /// own supply, such as the input of an unignored regulator.
    extra_sources: Vec<(NetId, Supply)>,
    ignored: BTreeMap<ComponentId, String>,
}

impl<'a> Draft<'a> {
    fn new(design: &'a Board, ground: NetId) -> Self {
        Self {
            design,
            ground,
            used: HashSet::new(),
            not_covered: Vec::new(),
            warnings: Vec::new(),
            todo_nets: BTreeSet::new(),
            names: HashSet::new(),
            measures: Vec::new(),
            extra_sources: Vec::new(),
            ignored: BTreeMap::new(),
        }
    }

    fn detect_leds(&mut self) {
        let ids: Vec<ComponentId> = self
            .design
            .components
            .iter()
            .filter(|component| is_kind(component, "led"))
            .map(|component| component.id)
            .collect();
        for id in ids {
            if !self.used.contains(&id) {
                self.detect_led(id);
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn detect_led(&mut self, id: ComponentId) {
        let design = self.design;
        let Some(component) = design.component(id) else {
            return;
        };
        let Some(part) = component.part.as_ref() else {
            return;
        };
        let (Some((cathode, _)), Some((anode, _))) =
            (component.find_pin("cathode"), component.find_pin("anode"))
        else {
            return;
        };
        let (Some(cathode_net), Some(anode_net)) =
            (self.pin_net(id, cathode), self.pin_net(id, anode))
        else {
            return;
        };
        // A reversed LED still has one terminal on ground; the series resistor
        // then sits between the other terminal and the supply.
        let (other_net, reversed) = if cathode_net == self.ground {
            (anode_net, false)
        } else if anode_net == self.ground {
            (cathode_net, true)
        } else {
            return;
        };
        let Some(other) = design.net(other_net) else {
            return;
        };
        if other_net == self.ground {
            return;
        }
        // The series resistor must carry the whole LED current. A third
        // endpoint (two LEDs sharing one resistor) splits it, so a single-LED
        // band would fail a valid board.
        if other.endpoints.len() != 2 {
            return;
        }
        if has_line_break(&other.name) {
            self.use_components(&[id]);
            self.not_covered.push(format!(
                "{}, a net name contains a newline",
                self.describe(id)
            ));
            return;
        }

        let mut series = Vec::new();
        for candidate in &design.components {
            if self.used.contains(&candidate.id) || !is_kind(candidate, "resistor") {
                continue;
            }
            if !self.has_endpoint(candidate.id, other_net) {
                continue;
            }
            let Some(outer) = self.other_net_of(candidate.id, other_net) else {
                continue;
            };
            series.push((candidate.id, outer));
        }
        let [(resistor, supply_net)] = series.as_slice() else {
            return;
        };
        let (resistor, supply_net) = (*resistor, *supply_net);
        if supply_net == self.ground {
            return;
        }
        let Some(supply_name) = design.net(supply_net).map(|net| net.name.clone()) else {
            return;
        };
        if has_line_break(&supply_name) {
            self.use_components(&[id, resistor]);
            self.not_covered.push(format!(
                "{}, a net name contains a newline",
                self.describe(id)
            ));
            return;
        }

        self.use_components(&[id, resistor]);
        let part_id = part.id.as_str();
        let Some(datasheet) = datasheet_url(part) else {
            self.not_covered.push(format!(
                "{} ({part_id}), no datasheet_url",
                component.refdes
            ));
            return;
        };
        let Some(supply) = self.supply(supply_net) else {
            if let Some(reason) = self.pin_supply_issue(supply_net) {
                self.not_covered
                    .push(format!("{}, {reason}", self.describe(id)));
            } else {
                self.todo_nets.insert(supply_net);
                self.not_covered.push(format!(
                    "{}, no declared supply on net {}",
                    self.describe(id),
                    toml_string(&supply_name)
                ));
            }
            return;
        };
        let Some(conditions) = part.operating_conditions.as_ref() else {
            self.not_covered.push(format!(
                "{} ({part_id}), no operating_conditions.forward_current_ma",
                component.refdes
            ));
            return;
        };
        let Some(forward_current_ma) = conditions.forward_current_ma else {
            self.not_covered.push(format!(
                "{} ({part_id}), no operating_conditions.forward_current_ma",
                component.refdes
            ));
            return;
        };
        let Some(forward_voltage_v) = conditions.forward_voltage_v else {
            self.not_covered.push(format!(
                "{} ({part_id}), no operating_conditions.forward_voltage_v",
                component.refdes
            ));
            return;
        };
        let resistance = match self.resistance(resistor) {
            Ok(value) => value,
            Err(reason) => {
                self.not_covered.push(format!(
                    "{}, {reason} for the LED series resistor",
                    self.describe(resistor)
                ));
                return;
            }
        };
        // A pin's output resistance is in series with the design resistor, so
        // the hand-check is I*(R + Rout) + Vf(I) = Vh.
        let output_resistance = supply
            .pin_drive
            .as_ref()
            .and_then(|pin| pin.drive.output_resistance_ohm)
            .unwrap_or(0.0);
        let total_resistance = resistance + output_resistance;
        let expected = if supply.volts <= forward_voltage_v {
            None
        } else {
            crate::diode_forward_current(
                supply.volts,
                total_resistance,
                forward_voltage_v,
                forward_current_ma,
            )
        };
        let Some(expected) = expected else {
            self.not_covered.push(format!(
                "{}, (Vs - Vf)/R gives no forward current: Vs {} V, Vf {} V, R {}",
                self.describe(id),
                number(supply.volts),
                number(forward_voltage_v),
                number(total_resistance)
            ));
            return;
        };
        let band_min = round_sig(expected * 0.95, 3);
        let uncapped_max = round_sig(expected * 1.05, 3);
        let mut min = band_min;
        let mut max = uncapped_max;
        // The datasheet forward current is the rating the Vf is specified at,
        // so it caps the band when the part declares no separate maximum.
        let cap = conditions.max_current_ma.unwrap_or(forward_current_ma);
        let cap_source = if conditions.max_current_ma.is_some() {
            "operating_conditions.max_current_ma"
        } else {
            "operating_conditions.forward_current_ma (no max_current_ma declared)"
        };
        let cap_amps = cap / 1000.0;
        let cap_note = if cap_amps < uncapped_max {
            max = round_sig(cap_amps, 3);
            format!(
                "{} mA from {cap_source} (binds; tightens the upper limit)",
                number(cap)
            )
        } else {
            format!(
                "{} mA from {cap_source} (does not bind; the ±5% band is lower)",
                number(cap)
            )
        };
        let overdrive = expected > cap_amps;
        if overdrive {
            min = round_sig(cap_amps, 3);
            max = min;
            self.warnings.push(format!(
                "overdrive: {} ({part_id}), I*R + Vf(I) = Vs gives {} A, above the {} mA \
                 cap from {cap_source}; the drafted band is pinned to the cap so the \
                 simulation fails",
                component.refdes,
                number(expected),
                number(cap)
            ));
        }
        if !min.is_finite() || !max.is_finite() || min <= 0.0 || min > max {
            self.not_covered.push(format!(
                "{} ({part_id}), operating_conditions give no usable current band",
                component.refdes
            ));
            return;
        }

        let mut comment = String::new();
        let _ = writeln!(
            comment,
            "# measure: {} ({part_id}) forward current, from the design series resistor and the datasheet.",
            component.refdes
        );
        let _ = writeln!(
            comment,
            "#   {} ({}) value {} (design series resistor)",
            self.refdes(resistor),
            self.part_id(resistor),
            self.value(resistor)
        );
        let _ = writeln!(
            comment,
            "#   operating_conditions.forward_voltage_v = {} V (registry Vf)",
            number(forward_voltage_v)
        );
        let _ = writeln!(
            comment,
            "#   operating_conditions.forward_current_ma = {} mA (datasheet rating)",
            number(forward_current_ma)
        );
        let _ = writeln!(comment, "#   current cap: {cap_note}");
        let _ = writeln!(comment, "#   datasheet: {datasheet}");
        if let Some(pin_supply) = &supply.pin_drive {
            let _ = writeln!(
                comment,
                "#   supply: `{}` high_v = {} V{} (datasheet: {})",
                pin_supply.pin,
                number(pin_supply.drive.high_v),
                pin_supply
                    .drive
                    .output_resistance_ohm
                    .map_or_else(String::new, |r| {
                        format!(", output_resistance_ohm = {}", number(r))
                    }),
                pin_supply.datasheet_ref
            );
            let _ = writeln!(
                comment,
                "#   hand-check: I*({} + {}) + Vf(I) = {} V.",
                number(resistance),
                number(output_resistance),
                number(supply.volts)
            );
        }
        let _ = writeln!(
            comment,
            "#   policy: I*R + Vf0 + N*Vt*ln(I/If0) = Vs gives {} A; ±5% rounded to 3 \
             significant digits: {band_min} A..{uncapped_max} A; final band {min} A..{max} A.",
            number(expected)
        );
        if overdrive {
            let _ = writeln!(
                comment,
                "# tripwire: regenerate with --suggest --force after fixing the resistor"
            );
        }
        if reversed {
            let _ = writeln!(
                comment,
                "#   the LED is reversed: its anode is on ground and its cathode on net {}",
                toml_string(&other.name)
            );
        }
        if needs_review(part) {
            let _ = writeln!(
                comment,
                "# UNREVIEWED: the part has no reviewed_by; check the datasheet before use."
            );
        }

        let name = self.unique("iled");
        self.ignore_driver(supply_net, &supply);
        let pin_supply = supply.pin_drive.clone();
        let pin_measure_supply = supply.clone();
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Op,
            name,
            kind: "current",
            node: None,
            element: Some(component.refdes.clone()),
            pin: None,
            min: Some(min),
            max: Some(max),
            at_frequency: None,
            comment,
            supply_net,
            supply,
            sweep: None,
        });

        if let Some(pin_supply) = pin_supply {
            // One current check per pin, even when several shapes share it.
            if self.measures.iter().any(|measure| {
                measure.kind == "pin_current"
                    && measure.pin.as_deref() == Some(pin_supply.pin.as_str())
            }) {
                return;
            }
            let mut comment = String::new();
            let _ = writeln!(
                comment,
                "# measure: `{}` output current against its datasheet drive limit.",
                pin_supply.pin
            );
            let _ = writeln!(
                comment,
                "#   `[pins.drive]` high_v = {} V, max_source_ma = {} mA{}{}.",
                number(pin_supply.drive.high_v),
                number(pin_supply.drive.max_source_ma),
                pin_supply
                    .drive
                    .output_resistance_ohm
                    .map_or_else(String::new, |r| {
                        format!(", output_resistance_ohm = {}", number(r))
                    }),
                pin_supply
                    .drive
                    .output_resistance_ohm
                    .map_or_else(String::new, |r| format!(
                        "; I*({} + {}) + Vf(I) = {} V",
                        number(resistance),
                        number(r),
                        number(pin_supply.drive.high_v)
                    ))
            );
            let _ = writeln!(
                comment,
                "#   datasheet: {}; the limit and drive levels come from the registry.",
                pin_supply.datasheet_ref
            );
            let name = self.unique("pin_current");
            self.measures.push(DraftMeasure {
                analysis: AnalysisKind::Op,
                name,
                kind: "pin_current",
                node: None,
                element: None,
                pin: Some(pin_supply.pin),
                min: None,
                max: None,
                at_frequency: None,
                comment,
                supply_net,
                supply: pin_measure_supply.clone(),
                sweep: None,
            });
        }
    }

    fn detect_dividers(&mut self) {
        let net_ids: Vec<NetId> = self.design.nets.iter().map(|net| net.id).collect();
        for net_id in net_ids {
            self.detect_divider(net_id);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn detect_divider(&mut self, net_id: NetId) {
        let design = self.design;
        if net_id == self.ground {
            return;
        }
        let Some(net) = design.net(net_id) else {
            return;
        };
        let mut resistors = Vec::new();
        let mut others = 0;
        for endpoint in &net.endpoints {
            let Some(component) = design.component(endpoint.component) else {
                return;
            };
            if self.used.contains(&component.id) {
                return;
            }
            if is_kind(component, "resistor") {
                if resistors.contains(&component.id) {
                    return;
                }
                resistors.push(component.id);
            } else {
                others += 1;
            }
        }
        if resistors.len() >= 3 {
            self.report_wide_divider(net, &resistors);
            return;
        }
        if others > 0 {
            let [first, second] = resistors.as_slice() else {
                return;
            };
            // Exactly one leg to ground makes a divider; two pull-downs on
            // one driven net are not a divider and must not be reported as
            // one.
            let first_ground = self.other_net_of(*first, net_id) == Some(self.ground);
            let second_ground = self.other_net_of(*second, net_id) == Some(self.ground);
            if first_ground == second_ground {
                return;
            }
            let mut drivers = Vec::new();
            for endpoint in &net.endpoints {
                let Some(component) = design.component(endpoint.component) else {
                    return;
                };
                if is_kind(component, "resistor") {
                    continue;
                }
                let pin = design
                    .pin(endpoint.component, endpoint.pin)
                    .map_or("", |pin| pin.name.as_str());
                let label = if pin.is_empty() {
                    component.refdes.clone()
                } else {
                    format!("{}.{pin}", component.refdes)
                };
                if !drivers.contains(&label) {
                    drivers.push(label);
                }
            }
            if drivers.is_empty() {
                return;
            }
            self.use_components(&[*first, *second]);
            let mut refs = [self.refdes(*first), self.refdes(*second)];
            refs.sort_unstable();
            self.not_covered.push(format!(
                "divider {}: mid net also drives {}",
                refs.join("/"),
                drivers.join(", ")
            ));
            return;
        }
        let [first, second] = resistors.as_slice() else {
            return;
        };
        let (first, second) = (*first, *second);
        let (Some(first_other), Some(second_other)) = (
            self.other_net_of(first, net_id),
            self.other_net_of(second, net_id),
        ) else {
            return;
        };
        let (top, bottom, supply_net) = if first_other == self.ground {
            (second, first, second_other)
        } else if second_other == self.ground {
            (first, second, first_other)
        } else {
            return;
        };
        if supply_net == self.ground {
            return;
        }
        let Some(supply_name) = design.net(supply_net).map(|net| net.name.clone()) else {
            return;
        };
        if has_line_break(&net.name) || has_line_break(&supply_name) {
            self.use_components(&[top, bottom]);
            self.not_covered.push(format!(
                "{}, a divider net name contains a newline",
                self.describe(top)
            ));
            return;
        }

        self.use_components(&[top, bottom]);
        let Some(supply) = self.supply(supply_net) else {
            // A net driven by another resistor is an internal node of a
            // longer chain, not a missing `power` declaration.
            if self.internal_chain_node(supply_net, top) {
                self.not_covered.push(format!(
                    "the divider {} + {}, net {} is an internal node of a longer \
                     resistor chain; no divider measure is drafted",
                    self.describe(top),
                    self.describe(bottom),
                    toml_string(&supply_name)
                ));
            } else if let Some(reason) = self.pin_supply_issue(supply_net) {
                self.not_covered.push(format!(
                    "the divider {} + {}, {reason}",
                    self.describe(top),
                    self.describe(bottom)
                ));
            } else {
                self.todo_nets.insert(supply_net);
                self.not_covered.push(format!(
                    "the divider {} + {}, no declared supply on net {}",
                    self.describe(top),
                    self.describe(bottom),
                    toml_string(&supply_name)
                ));
            }
            return;
        };
        let (top_ohms, bottom_ohms) = match (self.resistance(top), self.resistance(bottom)) {
            (Ok(top_ohms), Ok(bottom_ohms)) => (top_ohms, bottom_ohms),
            (Err(reason), _) => {
                self.not_covered
                    .push(format!("{}, {reason} for the divider", self.describe(top)));
                return;
            }
            (_, Err(reason)) => {
                self.not_covered.push(format!(
                    "{}, {reason} for the divider",
                    self.describe(bottom)
                ));
                return;
            }
        };
        let expected = supply.volts * bottom_ohms / (top_ohms + bottom_ohms);
        let min = round_sig(expected * 0.95, 3);
        let max = round_sig(expected * 1.05, 3);
        if !min.is_finite() || !max.is_finite() || min > max {
            self.not_covered.push(format!(
                "{}, divider values are unusable",
                self.describe(top)
            ));
            return;
        }

        let (top_value, bottom_value) = (self.value(top), self.value(bottom));
        let mut comment = String::new();
        let _ = writeln!(
            comment,
            "# measure: divider at net {}, V = Vsupply * Rbottom/(Rtop+Rbottom).",
            toml_string(&net.name)
        );
        let _ = writeln!(
            comment,
            "#   derived from this design's values; checks wiring and model, not the value choice."
        );
        let _ = writeln!(
            comment,
            "#   {} ({}) value {} : top, from net {}; datasheet: {}",
            self.refdes(top),
            self.part_id(top),
            top_value,
            toml_string(&supply_name),
            self.datasheet_text(top)
        );
        let _ = writeln!(
            comment,
            "#   {} ({}) value {} : bottom, to ground {}; datasheet: {}",
            self.refdes(bottom),
            self.part_id(bottom),
            bottom_value,
            toml_string(self.ground_name()),
            self.datasheet_text(bottom)
        );
        self.unreviewed_notes(&[top, bottom], &mut comment);
        let _ = writeln!(
            comment,
            "#   expected {} V * {}/({}+{}) = {} V; ±5% rounded to 3 significant digits: {min} V..{max} V.",
            number(supply.volts),
            bottom_value,
            top_value,
            bottom_value,
            number(expected)
        );

        let name = self.unique("vmid");
        self.ignore_driver(supply_net, &supply);
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Op,
            name,
            kind: "node_voltage",
            node: Some(net.name.clone()),
            element: None,
            pin: None,
            min: Some(min),
            max: Some(max),
            at_frequency: None,
            comment,
            supply_net,
            supply,
            sweep: None,
        });
    }

    /// A mid net with three or more resistor legs is divider-like but cannot
    /// be drafted. Name the shape so it does not look like unrelated parts;
    /// never draft a two-leg measure through a parallel bottom.
    fn report_wide_divider(&mut self, net: &Net, resistors: &[ComponentId]) {
        let mut top = None;
        let mut bottoms = Vec::new();
        for &resistor in resistors {
            match self.other_net_of(resistor, net.id) {
                Some(other) if other == self.ground => bottoms.push(resistor),
                Some(_) if top.is_none() => top = Some(resistor),
                _ => {
                    top = None;
                    bottoms.clear();
                    break;
                }
            }
        }
        // Without a leg to ground this is a resistor star, not a divider; the
        // parts stay in the plain "not a recognised shape" report.
        if bottoms.is_empty() {
            return;
        }
        self.use_components(resistors);
        let mut refdes: Vec<&str> = resistors.iter().map(|&id| self.refdes(id)).collect();
        refdes.sort_unstable();
        let label = match top {
            Some(top) if bottoms.len() >= 2 => {
                let mut bottoms: Vec<&str> = bottoms.iter().map(|&id| self.refdes(id)).collect();
                bottoms.sort_unstable();
                format!("{} + {}", self.refdes(top), bottoms.join("/"))
            }
            _ => refdes.join("/"),
        };
        let legs = resistors.len();
        let Some(top) = top else {
            self.not_covered.push(format!(
                "divider {label}: the mid net has {legs} resistor legs; only a two-resistor divider is drafted"
            ));
            return;
        };
        let Some(top_net) = self.other_net_of(top, net.id) else {
            return;
        };
        if self.supply(top_net).is_some() {
            self.not_covered.push(format!(
                "divider {label}: the mid net has {legs} resistor legs; only a two-resistor divider is drafted"
            ));
            return;
        }
        // A net driven by another resistor is an internal chain node, not a
        // missing `power` declaration.
        if !self.internal_chain_node(top_net, top) {
            self.todo_nets.insert(top_net);
        }
        let top_name = self.design.net(top_net).map_or("", |net| net.name.as_str());
        self.not_covered.push(format!(
            "divider {label}: the mid net has {legs} resistor legs; only a two-resistor \
             divider is drafted (top net {} is not a declared supply)",
            toml_string(top_name)
        ));
    }

    fn detect_rcs(&mut self) {
        let net_ids: Vec<NetId> = self.design.nets.iter().map(|net| net.id).collect();
        let op_selected = !self.measures.is_empty();
        for net_id in net_ids {
            let Some((resistor, capacitor)) = self.rc_pair(net_id) else {
                continue;
            };
            if op_selected {
                self.use_components(&[resistor, capacitor]);
                self.not_covered.push(format!(
                    "{} + {}, RC corner needs an ac analysis and cannot share a file with an op measure",
                    self.describe(resistor),
                    self.describe(capacitor)
                ));
            } else {
                self.detect_rc(net_id, resistor, capacitor);
            }
        }
    }

    /// The resistor and capacitor on a two-endpoint net, if it is exactly that.
    fn rc_pair(&self, net_id: NetId) -> Option<(ComponentId, ComponentId)> {
        if net_id == self.ground {
            return None;
        }
        let net = self.design.net(net_id)?;
        if net.endpoints.len() != 2 {
            return None;
        }
        let mut resistor = None;
        let mut capacitor = None;
        for endpoint in &net.endpoints {
            let component = self.design.component(endpoint.component)?;
            if self.used.contains(&component.id) {
                return None;
            }
            if is_kind(component, "resistor") {
                if resistor.replace(component.id).is_some() {
                    return None;
                }
            } else if is_kind(component, "capacitor") {
                if capacitor.replace(component.id).is_some() {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some((resistor?, capacitor?))
    }

    #[allow(clippy::too_many_lines)]
    fn detect_rc(&mut self, net_id: NetId, resistor: ComponentId, capacitor: ComponentId) {
        let design = self.design;
        let Some(net) = design.net(net_id) else {
            return;
        };
        let (Some(input_net), Some(cap_other)) = (
            self.other_net_of(resistor, net_id),
            self.other_net_of(capacitor, net_id),
        ) else {
            return;
        };
        if cap_other != self.ground || input_net == self.ground {
            return;
        }
        let Some(input_name) = design.net(input_net).map(|net| net.name.clone()) else {
            return;
        };
        if has_line_break(&net.name) || has_line_break(&input_name) {
            self.use_components(&[resistor, capacitor]);
            self.not_covered.push(format!(
                "{}, an RC net name contains a newline",
                self.describe(resistor)
            ));
            return;
        }

        self.use_components(&[resistor, capacitor]);
        let Some(supply) = self.supply(input_net) else {
            self.todo_nets.insert(input_net);
            self.not_covered.push(format!(
                "{}, no declared supply on net {}",
                self.describe(resistor),
                toml_string(&input_name)
            ));
            return;
        };
        if supply.pin_drive.is_some() {
            self.not_covered.push(format!(
                "{}, an AC sweep from an IC pin source is not drafted",
                self.describe(resistor)
            ));
            return;
        }
        if supply.volts <= 0.0 {
            self.not_covered.push(format!(
                "{}, no positive declared supply on net {}",
                self.describe(resistor),
                toml_string(&input_name)
            ));
            return;
        }
        let resistance = match self.resistance(resistor) {
            Ok(value) => value,
            Err(reason) => {
                self.not_covered.push(format!(
                    "{}, {reason} for the RC filter",
                    self.describe(resistor)
                ));
                return;
            }
        };
        let capacitance = match self.capacitance(capacitor) {
            Ok(value) => value,
            Err(reason) => {
                self.not_covered.push(format!(
                    "{}, {reason} for the RC filter",
                    self.describe(capacitor)
                ));
                return;
            }
        };
        let corner = 1.0 / (2.0 * std::f64::consts::PI * resistance * capacitance);
        let start = round_sig(corner / 100.0, 3);
        let stop = round_sig(corner * 100.0, 3);
        let min = round_sig(corner * 0.98, 3);
        let max = round_sig(corner * 1.02, 3);
        if ![corner, start, stop, min, max]
            .iter()
            .all(|value| value.is_finite())
            || start <= 0.0
            || stop <= start
            || min > max
        {
            self.not_covered.push(format!(
                "{}, RC values are unusable",
                self.describe(resistor)
            ));
            return;
        }

        let (resistance_value, capacitance_value) = (
            self.value(resistor).to_string(),
            self.value(capacitor).to_string(),
        );
        let mut comment = String::new();
        let _ = writeln!(
            comment,
            "# measure: RC low-pass at net {}, fc = 1/(2*pi*R*C).",
            toml_string(&net.name)
        );
        let _ = writeln!(
            comment,
            "#   derived from this design's values; checks wiring and model, not the value choice."
        );
        let _ = writeln!(
            comment,
            "#   {} ({}) value {}; datasheet: {}",
            self.refdes(resistor),
            self.part_id(resistor),
            resistance_value,
            self.datasheet_text(resistor)
        );
        let _ = writeln!(
            comment,
            "#   {} ({}) value {}; datasheet: {}",
            self.refdes(capacitor),
            self.part_id(capacitor),
            capacitance_value,
            self.datasheet_text(capacitor)
        );
        self.unreviewed_notes(&[resistor, capacitor], &mut comment);
        let _ = writeln!(
            comment,
            "#   1/(2*pi*{resistance_value}*{capacitance_value}) = {} Hz; ≈±2% rounded to 3 significant digits: {min} Hz..{max} Hz.",
            number(corner)
        );

        let name = self.unique("corner");
        self.ignore_driver(input_net, &supply);
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Ac,
            name,
            kind: "relative_db",
            node: Some(net.name.clone()),
            element: None,
            pin: None,
            min: Some(min),
            max: Some(max),
            at_frequency: None,
            comment,
            supply_net: input_net,
            supply: supply.clone(),
            sweep: Some((start, stop)),
        });

        // The corner cannot tell a low-pass from a high-pass with the same
        // R*C, so pin the gain one decade either side of it.
        let passband_frequency = round_sig(corner / 10.0, 3);
        let mut passband_comment = String::new();
        let _ = writeln!(
            passband_comment,
            "# measure: RC low-pass passband at net {}, gain one decade below fc.",
            toml_string(&net.name)
        );
        let _ = writeln!(
            passband_comment,
            "#   a single pole gives 20*log10(1/sqrt(1+0.1^2)) = -0.043 dB at fc/10; a swapped R/C turns this into a high-pass and lands near -20 dB."
        );
        let _ = writeln!(
            passband_comment,
            "#   1/(2*pi*{resistance_value}*{capacitance_value}) = {} Hz; fc/10 = {passband_frequency} Hz rounded to 3 significant digits.",
            number(corner)
        );
        let name = self.unique("passband");
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Ac,
            name,
            kind: "node_voltage",
            node: Some(net.name.clone()),
            element: None,
            pin: None,
            min: Some(-0.5),
            max: Some(0.0),
            at_frequency: Some(passband_frequency),
            comment: passband_comment,
            supply_net: input_net,
            supply: supply.clone(),
            sweep: None,
        });

        let stopband_frequency = round_sig(corner * 10.0, 3);
        let mut stopband_comment = String::new();
        let _ = writeln!(
            stopband_comment,
            "# measure: RC low-pass stopband at net {}, gain one decade above fc.",
            toml_string(&net.name)
        );
        let _ = writeln!(
            stopband_comment,
            "#   a single pole gives 20*log10(1/sqrt(1+10^2)) = -20.04 dB at 10*fc; ±3 dB covers component tolerance."
        );
        let _ = writeln!(
            stopband_comment,
            "#   1/(2*pi*{resistance_value}*{capacitance_value}) = {} Hz; 10*fc = {stopband_frequency} Hz rounded to 3 significant digits.",
            number(corner)
        );
        let name = self.unique("stopband");
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Ac,
            name,
            kind: "node_voltage",
            node: Some(net.name.clone()),
            element: None,
            pin: None,
            min: Some(-23.0),
            max: Some(-17.0),
            at_frequency: Some(stopband_frequency),
            comment: stopband_comment,
            supply_net: input_net,
            supply,
            sweep: None,
        });
    }

    /// A passive left on a net whose other parts are all ignored would be the
    /// only live connection there (`-008 net VIN has 1 connection(s)`).
    /// Ignore it too, with a reason. Nets are judged as a group: two caps on
    /// one dead rail keep each other "live" in a per-part check.
    fn ignore_orphan_passives(&mut self) {
        let candidates: Vec<ComponentId> = self
            .design
            .components
            .iter()
            .filter(|component| {
                !self.used.contains(&component.id)
                    && !self.ignored.contains_key(&component.id)
                    && is_passive(component)
                    // A bad value is a hard -002; ignoring it would hide the typo.
                    && !matches!(
                        crate::model_for(component),
                        Err(crate::ModelError::BadValue(_))
                    )
            })
            .map(|component| component.id)
            .collect();
        let sourced: HashSet<NetId> = self
            .measures
            .iter()
            .map(|measure| measure.supply_net)
            .chain(self.extra_sources.iter().map(|(net, _)| *net))
            .collect();
        // A used part or any unignored non-passive is in the deck and makes
        // its nets live; propagate that until no more candidates are live.
        let mut live: HashSet<ComponentId> = self
            .design
            .components
            .iter()
            .filter(|component| {
                !self.ignored.contains_key(&component.id)
                    && (self.used.contains(&component.id) || !is_passive(component))
            })
            .map(|component| component.id)
            .collect();
        loop {
            let live_nets: HashSet<NetId> = self
                .design
                .nets
                .iter()
                .filter(|net| {
                    net.id == self.ground
                        || sourced.contains(&net.id)
                        || net
                            .endpoints
                            .iter()
                            .any(|endpoint| live.contains(&endpoint.component))
                })
                .map(|net| net.id)
                .collect();
            let newly: Vec<ComponentId> = candidates
                .iter()
                .copied()
                .filter(|id| {
                    !live.contains(id)
                        && self
                            .component_nets(*id)
                            .iter()
                            .any(|net| *net != self.ground && live_nets.contains(net))
                })
                .collect();
            if newly.is_empty() {
                break;
            }
            live.extend(newly);
        }
        let reason = "every net it connects to is driven only by ignored parts".to_string();
        for id in candidates {
            if live.contains(&id) {
                continue;
            }
            let nets = self.component_nets(id);
            let nets: Vec<NetId> = nets.into_iter().filter(|net| *net != self.ground).collect();
            if nets.is_empty() {
                continue;
            }
            let orphan = nets.iter().all(|net_id| {
                self.design.net(*net_id).is_some_and(|net| {
                    // A dangling part has no ignored driver to blame; leave it
                    // in the deck so topology fails loudly.
                    net.endpoints.len() >= 2
                        && net.endpoints.iter().all(|endpoint| {
                            self.ignored.contains_key(&endpoint.component)
                                || !live.contains(&endpoint.component)
                        })
                })
            });
            if orphan {
                self.ignored.entry(id).or_insert_with(|| reason.clone());
            }
        }
    }

    /// A modelled regulator that stays in the deck has only its subcircuit
    /// pins to hold its input node; an undriven input is a singular matrix.
    /// Source it from the declared `power` voltage, or write a TODO.
    ///
    /// ponytail: an AC draft keeps the current behaviour because its sources
    /// all carry the 1 V excitation; handle a bias-only source when needed.
    fn source_regulator_inputs(&mut self) {
        if self.measures.is_empty()
            || self
                .measures
                .iter()
                .any(|measure| measure.analysis == AnalysisKind::Ac)
        {
            return;
        }
        let sourced: HashSet<NetId> = self
            .measures
            .iter()
            .map(|measure| measure.supply_net)
            .collect();
        let mut rail_sources = Vec::new();
        let mut missing = Vec::new();
        for net in &self.design.nets {
            if net.id == self.ground || sourced.contains(&net.id) {
                continue;
            }
            let needed = net.endpoints.iter().any(|endpoint| {
                let Some(component) = self.design.component(endpoint.component) else {
                    return false;
                };
                if self.ignored.contains_key(&component.id) {
                    return false;
                }
                let Some(part) = component.part.as_ref() else {
                    return false;
                };
                part.kind == "regulator"
                    && part.spice.is_some()
                    && self
                        .design
                        .pin(endpoint.component, endpoint.pin)
                        .is_some_and(|pin| pin.electrical_type == ElectricalType::PowerInput)
            });
            if !needed || self.has_retained_power_output(net) {
                continue;
            }
            match self.supply(net.id) {
                Some(mut supply) => {
                    supply.note = format!(
                        "{}; it is the input of a modelled regulator and is sourced so the \
                         model has a DC path",
                        supply.note
                    );
                    rail_sources.push((net.id, supply));
                }
                None => missing.push(net.id),
            }
        }
        self.extra_sources.extend(rail_sources);
        for net_id in missing {
            self.todo_nets.insert(net_id);
        }
    }

    fn component_nets(&self, component: ComponentId) -> Vec<NetId> {
        self.design
            .nets
            .iter()
            .filter(|net| {
                net.endpoints
                    .iter()
                    .any(|endpoint| endpoint.component == component)
            })
            .map(|net| net.id)
            .collect()
    }

    /// A net already held by a part that stays in the deck needs no
    /// testbench source, and a second ideal source would fight it.
    fn has_retained_power_output(&self, net: &Net) -> bool {
        net.endpoints.iter().any(|endpoint| {
            !self.ignored.contains_key(&endpoint.component)
                && self
                    .design
                    .pin(endpoint.component, endpoint.pin)
                    .is_some_and(|pin| pin.electrical_type == ElectricalType::PowerOutput)
        })
    }

    fn report_unused(&mut self) {
        for component in &self.design.components {
            if !self.used.contains(&component.id) {
                // A bad value is not a missing model: ignoring it would hide
                // the typo, so leave it in the deck for -002 to report.
                if crate::model_for(component).is_err_and(|error| error.is_missing_model()) {
                    self.ignored
                        .entry(component.id)
                        .or_insert_with(|| "no built-in SPICE model".to_string());
                }
                self.not_covered.push(format!(
                    "{} ({}), not a recognised shape",
                    component.refdes,
                    component_part_id(component)
                ));
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Option<String> {
        let first = self.measures.first()?;
        let ac = first.analysis == AnalysisKind::Ac;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# Generated by `synth sim --suggest` from {}.",
            comment_text(&self.design.name)
        );
        let _ = writeln!(
            out,
            "# Every value cites its source; review it against the datasheet before use."
        );
        if !self.todo_nets.is_empty() {
            let _ = writeln!(out);
            for net_id in &self.todo_nets {
                let name = self.design.net(*net_id).map_or("", |net| net.name.as_str());
                let name = comment_text(name);
                let _ = writeln!(
                    out,
                    "# TODO: net \"{name}\" has no declared supply voltage; add `power \"{name}\" <voltage>` to the design."
                );
            }
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "ground = {}", toml_string(self.ground_name()));

        let mut ignored: Vec<(&str, &str, &str)> = self
            .ignored
            .iter()
            .filter_map(|(id, reason)| {
                self.design.component(*id).map(|component| {
                    (
                        component.refdes.as_str(),
                        component_part_id(component),
                        reason.as_str(),
                    )
                })
            })
            .collect();
        if !ignored.is_empty() {
            ignored.sort_by(|left, right| left.0.cmp(right.0));
            let _ = writeln!(out);
            for (refdes, part, reason) in &ignored {
                let _ = writeln!(out, "# ignore: {refdes} ({part}): {reason}.");
            }
            let list: Vec<String> = ignored
                .iter()
                .map(|(refdes, _, _)| toml_string(refdes))
                .collect();
            let _ = writeln!(out, "ignore = [{}]", list.join(", "));
        }

        let mut sources: BTreeMap<NetId, &Supply> = BTreeMap::new();
        for measure in &self.measures {
            sources.entry(measure.supply_net).or_insert(&measure.supply);
        }
        for (net_id, supply) in &self.extra_sources {
            sources.entry(*net_id).or_insert(supply);
        }
        for (net_id, supply) in sources {
            let name = self.design.net(net_id).map_or("", |net| net.name.as_str());
            let _ = writeln!(out);
            let _ = writeln!(out, "# source: {}", supply.note);
            let _ = writeln!(out, "[[source]]");
            if let Some(pin_supply) = &supply.pin_drive {
                let _ = writeln!(out, "pin = {}", toml_string(&pin_supply.pin));
                let _ = writeln!(out, "level = \"high\"");
                continue;
            }
            let _ = writeln!(out, "net = {}", toml_string(name));
            let _ = writeln!(out, "kind = \"dc\"");
            if ac {
                let _ = writeln!(out, "#   ac drive: 1 V, so vdb(node) is gain in dB.");
                let _ = writeln!(out, "ac = 1.0");
            } else {
                // The limits use the full supply; a rounded source would move
                // the measurement away from them.
                let _ = writeln!(out, "dc = {:?}", supply.volts);
            }
        }

        let _ = writeln!(out);
        if ac {
            let starts = self
                .measures
                .iter()
                .filter_map(|measure| measure.sweep.map(|sweep| sweep.0));
            let stops = self
                .measures
                .iter()
                .filter_map(|measure| measure.sweep.map(|sweep| sweep.1));
            let start = starts.fold(f64::INFINITY, f64::min);
            let stop = stops.fold(f64::NEG_INFINITY, f64::max);
            let _ = writeln!(
                out,
                "# sweep: fc/100 .. fc*100 of each RC shape, rounded to 3 significant digits."
            );
            let _ = writeln!(out, "[analysis.ac]");
            let _ = writeln!(out, "start = {}", float(start));
            let _ = writeln!(out, "stop = {}", float(stop));
        } else {
            let _ = writeln!(out, "[analysis]");
            let _ = writeln!(out, "op = true");
        }

        for measure in &self.measures {
            let _ = writeln!(out);
            out.push_str(&measure.comment);
            let _ = writeln!(out, "[[measure]]");
            let _ = writeln!(out, "name = {}", toml_string(&measure.name));
            let _ = writeln!(out, "kind = \"{}\"", measure.kind);
            if let Some(node) = &measure.node {
                let _ = writeln!(out, "node = {}", toml_string(node));
            }
            if let Some(element) = &measure.element {
                let _ = writeln!(out, "element = {}", toml_string(element));
            }
            if let Some(pin) = &measure.pin {
                let _ = writeln!(out, "pin = {}", toml_string(pin));
            }
            if let Some(at_frequency) = measure.at_frequency {
                let _ = writeln!(out, "at_frequency = {}", float(at_frequency));
            }
            if let Some(min) = measure.min {
                let _ = writeln!(out, "min = {}", float(min));
            }
            if let Some(max) = measure.max {
                let _ = writeln!(out, "max = {}", float(max));
            }
        }
        Some(out)
    }

    fn unique(&mut self, base: &str) -> String {
        if self.names.insert(base.to_string()) {
            return base.to_string();
        }
        let mut suffix = 2;
        loop {
            let candidate = format!("{base}{suffix}");
            if self.names.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn use_components(&mut self, components: &[ComponentId]) {
        for component in components {
            self.used.insert(*component);
        }
    }

    fn refdes(&self, component: ComponentId) -> &str {
        self.design
            .component(component)
            .map_or("", |component| component.refdes.as_str())
    }

    fn part_id(&self, component: ComponentId) -> &str {
        self.design
            .component(component)
            .map_or("", component_part_id)
    }

    fn describe(&self, component: ComponentId) -> String {
        format!("{} ({})", self.refdes(component), self.part_id(component))
    }

    fn datasheet_text(&self, component: ComponentId) -> &str {
        self.design
            .component(component)
            .and_then(|component| component.part.as_ref())
            .and_then(datasheet_url)
            .unwrap_or("none (design value)")
    }

    /// The ticket's marker for any part in the measure without a reviewer.
    fn unreviewed_notes(&self, components: &[ComponentId], comment: &mut String) {
        for &component in components {
            let unverified = self
                .design
                .component(component)
                .and_then(|component| component.part.as_ref())
                .is_some_and(needs_review);
            if unverified {
                let _ = writeln!(
                    comment,
                    "# UNREVIEWED: {} ({}) has no reviewed_by; check the datasheet before use.",
                    self.refdes(component),
                    self.part_id(component)
                );
            }
        }
    }

    fn ground_name(&self) -> &str {
        self.design
            .net(self.ground)
            .map_or("", |net| net.name.as_str())
    }

    /// The declared or pin-derived supply on a net, in precedence order: the
    /// net's `power` voltage, a `PowerOutput` nominal, then exactly one
    /// output-capable pin carrying `[pins.drive]`.
    #[allow(clippy::too_many_lines)]
    fn supply(&self, net_id: NetId) -> Option<Supply> {
        let design = self.design;
        let net = design.net(net_id)?;
        if let Some(voltage) = net.voltage {
            let volts = voltage.to_v();
            if !volts.is_finite() {
                return None;
            }
            return Some(Supply {
                volts,
                note: format!(
                    "net {} declares a `power` voltage of {} V",
                    toml_string(&net.name),
                    number(volts)
                ),
                drivers: Vec::new(),
                pin_drive: None,
            });
        }

        let mut candidates = Vec::new();
        for endpoint in &net.endpoints {
            let Some(pin) = design.pin(endpoint.component, endpoint.pin) else {
                continue;
            };
            if pin.electrical_type != ElectricalType::PowerOutput {
                continue;
            }
            // Only an explicit nominal is a supply source. `voltage_max_v` is
            // a rating; treating it as a rail would invent a voltage.
            let Some(volts) = pin.voltage_nominal_v else {
                continue;
            };
            if volts.is_finite() {
                candidates.push((volts, endpoint.component, endpoint.pin));
            }
        }
        if let Some(&first) = candidates.first() {
            if candidates.iter().any(|candidate| candidate.0 != first.0) {
                return None;
            }
            let mut drivers = Vec::new();
            for candidate in &candidates {
                if !drivers.contains(&candidate.1) {
                    drivers.push(candidate.1);
                }
            }
            let component = design.component(first.1)?;
            let pin = design.pin(first.1, first.2)?;
            return Some(Supply {
                volts: first.0,
                note: format!(
                    "net {} is driven by `{}.{}`, a power output that declares {} V; \
                     the part is modelled as an ideal source here",
                    toml_string(&net.name),
                    component.refdes,
                    pin.name,
                    number(first.0)
                ),
                drivers,
                pin_drive: None,
            });
        }

        let mut driven = Vec::new();
        let mut undriven_output = false;
        for endpoint in &net.endpoints {
            let Some(component) = design.component(endpoint.component) else {
                continue;
            };
            let Some(pin) = design.pin(endpoint.component, endpoint.pin) else {
                continue;
            };
            if !pin.electrical_type.can_output() {
                continue;
            }
            let Some(drive) = pin.drive.as_ref() else {
                undriven_output = true;
                continue;
            };
            driven.push((
                component.id,
                format!("{}.{}", component.refdes, pin.name),
                drive.clone(),
            ));
        }
        // One driven pin plus another output-capable pin without drive data is
        // ambiguous: that pin could drive the net too, so never guess one.
        if undriven_output {
            return None;
        }
        // Several driven pins on one net are ambiguous; never guess one.
        let (component, pin, drive) = match driven.as_slice() {
            [only] => (only.0, only.1.clone(), only.2.clone()),
            _ => return None,
        };
        let datasheet_ref = drive
            .datasheet_ref
            .as_deref()
            .map(str::trim)
            .filter(|reference| !reference.is_empty())?
            .to_string();
        Some(Supply {
            volts: drive.high_v,
            note: format!(
                "net {} is driven by `{pin}`, an output pin with `[pins.drive]` high_v = {} V{} \
                 (datasheet: {}); the pin is replaced by the testbench source here",
                toml_string(&net.name),
                number(drive.high_v),
                drive
                    .output_resistance_ohm
                    .map_or_else(String::new, |r| format!(
                        " and output_resistance_ohm = {}",
                        number(r)
                    )),
                datasheet_ref
            ),
            drivers: vec![component],
            pin_drive: Some(PinSupply {
                pin,
                drive,
                datasheet_ref,
            }),
        })
    }

    /// Why a supply net that looks pin-driven has no usable supply. The LED
    /// shape reports this instead of writing a TODO, because the fix is
    /// registry drive data, not a `power` declaration.
    fn pin_supply_issue(&self, net_id: NetId) -> Option<String> {
        let design = self.design;
        let net = design.net(net_id)?;
        let mut driven = Vec::new();
        let mut missing_datasheet = Vec::new();
        let mut undriven = Vec::new();
        for endpoint in &net.endpoints {
            let Some(component) = design.component(endpoint.component) else {
                continue;
            };
            let Some(pin) = design.pin(endpoint.component, endpoint.pin) else {
                continue;
            };
            if !pin.electrical_type.can_output() {
                continue;
            }
            let label = format!("{}.{}", component.refdes, pin.name);
            match pin.drive.as_ref() {
                Some(drive)
                    if drive
                        .datasheet_ref
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|reference| !reference.is_empty()) =>
                {
                    driven.push(label);
                }
                Some(_) => missing_datasheet.push(label),
                None => undriven.push(label),
            }
        }
        if driven.len() > 1 {
            return Some(format!(
                "more than one output pin on net {} carries `[pins.drive]` ({}); the supply \
                 is ambiguous",
                toml_string(&net.name),
                driven.join(", ")
            ));
        }
        if let Some(label) = missing_datasheet.first() {
            return Some(format!(
                "pin has no drive data for `{label}` on net {}: its `[pins.drive]` block has \
                 no `datasheet_ref`",
                toml_string(&net.name)
            ));
        }
        if let ([driver], Some(other)) = (driven.as_slice(), undriven.first()) {
            return Some(format!(
                "net {} has two drivers: {driver} has drive data, {other} does not",
                toml_string(&net.name)
            ));
        }
        undriven.first().map(|label| {
            format!(
                "pin has no drive data for `{label}` on net {}; add a `[pins.drive]` block \
                 with the datasheet levels, limits and `datasheet_ref`",
                toml_string(&net.name)
            )
        })
    }

    /// A part whose pin supplied the testbench source is replaced by that
    /// source, so it must be left out of the deck once a measure uses the
    /// supply.
    fn ignore_driver(&mut self, supply_net: NetId, supply: &Supply) {
        let net = self
            .design
            .net(supply_net)
            .map_or("", |net| net.name.as_str());
        let reason = format!(
            "replaced by the testbench source on net {}",
            toml_string(net)
        );
        for &driver in &supply.drivers {
            self.ignored.entry(driver).or_insert_with(|| reason.clone());
        }
    }

    fn pin_net(&self, component: ComponentId, pin: PinId) -> Option<NetId> {
        let mut nets = self.design.nets.iter().filter(|net| {
            net.endpoints
                .iter()
                .any(|endpoint| endpoint.component == component && endpoint.pin == pin)
        });
        let first = nets.next()?.id;
        nets.next().is_none().then_some(first)
    }

    fn has_endpoint(&self, component: ComponentId, net: NetId) -> bool {
        self.design.net(net).is_some_and(|net| {
            net.endpoints
                .iter()
                .any(|endpoint| endpoint.component == component)
        })
    }

    /// True when another resistor connects to `net`; such a net is an internal
    /// node of a longer chain, not a missing `power` declaration. A connector
    /// or an IC pin is not a supply either, but it is not silently assumed to
    /// be one.
    fn internal_chain_node(&self, net: NetId, exclude: ComponentId) -> bool {
        let Some(net) = self.design.net(net) else {
            return false;
        };
        net.endpoints.iter().any(|endpoint| {
            endpoint.component != exclude
                && self
                    .design
                    .component(endpoint.component)
                    .is_some_and(|component| is_kind(component, "resistor"))
        })
    }

    /// The one other net a two-pin part is connected to.
    fn other_net_of(&self, component: ComponentId, net: NetId) -> Option<NetId> {
        let part = self.design.component(component)?.part.as_ref()?;
        if part.pins.len() != 2 {
            return None;
        }
        let mut nets = self
            .design
            .nets
            .iter()
            .filter(|candidate| candidate.id != net)
            .filter(|candidate| {
                candidate
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.component == component)
            });
        let first = nets.next()?.id;
        nets.next().is_none().then_some(first)
    }

    fn value(&self, component: ComponentId) -> &str {
        self.design
            .component(component)
            .and_then(|component| component.value.as_deref())
            .unwrap_or("")
    }

    fn resistance(&self, component: ComponentId) -> Result<f64, &'static str> {
        let raw = self.value(component);
        let Some(value) = parse_resistance(raw) else {
            return Err(if raw.to_ascii_lowercase().contains("meg") {
                "use \"1M\" (`meg` is not Synth syntax)"
            } else {
                "no parseable `value`"
            });
        };
        if !value.is_finite() || value <= 0.0 {
            return Err("the value must be positive and finite");
        }
        Ok(value)
    }

    fn capacitance(&self, component: ComponentId) -> Result<f64, &'static str> {
        let raw = self.value(component);
        let Some(value) = parse_capacitance(raw) else {
            return Err(if raw.to_ascii_lowercase().contains("meg") {
                "use \"1M\" (`meg` is not Synth syntax)"
            } else {
                "no parseable `value`"
            });
        };
        if !value.is_finite() || value <= 0.0 {
            return Err("the value must be positive and finite");
        }
        Ok(value)
    }
}

fn is_passive(component: &Component) -> bool {
    matches!(
        component.kind.as_str(),
        "resistor" | "capacitor" | "inductor"
    )
}

fn is_kind(component: &Component, kind: &str) -> bool {
    component
        .part
        .as_ref()
        .is_some_and(|part| part.kind == kind)
}

fn component_part_id(component: &Component) -> &str {
    component
        .part
        .as_ref()
        .map_or(component.kind.as_str(), |part| part.id.as_str())
}

fn datasheet_url(part: &synth_registry::Part) -> Option<&str> {
    part.provenance
        .as_ref()
        .and_then(|provenance| provenance.datasheet_url.as_deref())
        .filter(|url| !url.trim().is_empty())
}

/// No reviewer means unreviewed, including a part with no `[provenance]` block.
fn needs_review(part: &synth_registry::Part) -> bool {
    part.provenance
        .as_ref()
        .and_then(|provenance| provenance.reviewed_by.as_deref())
        .unwrap_or("")
        .is_empty()
}

/// A net name must not smuggle a new TOML line into the sidecar. The loader
/// rejects these names too, so a shape that uses one is not drafted.
fn has_line_break(value: &str) -> bool {
    value.contains(['\n', '\r'])
}

/// Text that is safe inside a `#` comment.
fn comment_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// TOML basic-string escaping. Control characters are escaped rather than
/// emitted raw, so a name cannot break the generated file.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{:04X}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A number for a comment: 4 significant digits for the reader. Limits keep
/// their own precision through [`float`].
fn number(value: f64) -> String {
    round_sig(value, 4).to_string()
}

/// A TOML float, rounded to four significant digits so a three-digit limit
/// never prints as `15599999.999999998`. `Debug` keeps `6.0` a float.
fn float(value: f64) -> String {
    format!("{:?}", round_sig(value, 4))
}

/// Round to `digits` significant digits through decimal text; arithmetic on
/// powers of ten reintroduces binary noise (`1560 / 1e-4`).
fn round_sig(value: f64, digits: i32) -> f64 {
    if !value.is_finite() || value == 0.0 || digits < 1 {
        return value;
    }
    // `digits >= 1` is guaranteed above, so this conversion cannot fail.
    let Ok(precision) = usize::try_from(digits - 1) else {
        return value;
    };
    format!("{value:.precision$e}").parse().unwrap_or(value)
}
