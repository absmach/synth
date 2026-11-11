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
use synth_registry::ElectricalType;
use synth_validate::{parse_capacitance, parse_resistance};

/// A drafted `<design>.sim.toml`, plus everything the draft could not cover.
#[derive(Debug, Clone)]
pub struct TestbenchSuggestion {
    /// `<design>.sim.toml` text, or `None` when no measure could be derived.
    pub toml: Option<String>,
    /// Human-readable reasons parts/shapes were left out.
    pub not_covered: Vec<String>,
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
        };
    }
    let ground = grounds[0];
    if has_line_break(&ground.name) {
        return TestbenchSuggestion {
            toml: None,
            not_covered: vec!["the ground net name contains a newline".to_string()],
        };
    }

    let mut draft = Draft::new(design, ground.id);
    draft.detect_leds();
    draft.detect_dividers();
    draft.detect_rcs();
    draft.report_unused();
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
}

#[derive(Debug)]
struct DraftMeasure {
    analysis: AnalysisKind,
    name: String,
    kind: &'static str,
    node: Option<String>,
    element: Option<String>,
    min: f64,
    max: f64,
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
    todo_nets: BTreeSet<NetId>,
    names: HashSet<String>,
    measures: Vec<DraftMeasure>,
}

impl<'a> Draft<'a> {
    fn new(design: &'a Board, ground: NetId) -> Self {
        Self {
            design,
            ground,
            used: HashSet::new(),
            not_covered: Vec::new(),
            todo_nets: BTreeSet::new(),
            names: HashSet::new(),
            measures: Vec::new(),
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
        let Some(supply) = self.supply(supply_net) else {
            self.todo_nets.insert(supply_net);
            self.not_covered.push(format!(
                "{}, no declared supply on net {}",
                self.describe(id),
                toml_string(&supply_name)
            ));
            return;
        };
        let part_id = part.id.as_str();
        let Some(datasheet) = datasheet_url(part) else {
            self.not_covered.push(format!(
                "{} ({part_id}), no datasheet_url",
                component.refdes
            ));
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
        let nominal = forward_current_ma / 1000.0;
        let min = round_sig(nominal * 0.95, 3);
        let uncapped_max = round_sig(nominal * 1.05, 3);
        let mut max = uncapped_max;
        let cap_note = conditions.max_current_ma.map_or_else(
            || "not declared (no cap applied)".to_string(),
            |cap| {
                let cap_amps = round_sig(cap / 1000.0, 3);
                if cap_amps < uncapped_max {
                    max = cap_amps;
                    format!("{} mA (binds; tightens the upper limit)", number(cap))
                } else {
                    format!("{} mA (does not bind; the ±5% band is lower)", number(cap))
                }
            },
        );
        if !min.is_finite() || !max.is_finite() || min <= 0.0 || min > max {
            self.not_covered.push(format!(
                "{} ({part_id}), operating_conditions give no usable current band",
                component.refdes
            ));
            return;
        }

        let forward_voltage = conditions
            .forward_voltage_v
            .map_or_else(|| "not declared".to_string(), number);
        let mut comment = String::new();
        let _ = writeln!(
            comment,
            "# measure: {} ({part_id}) forward current, derived only from the datasheet.",
            component.refdes
        );
        let _ = writeln!(
            comment,
            "#   operating_conditions.forward_voltage_v = {forward_voltage} V"
        );
        let _ = writeln!(
            comment,
            "#   operating_conditions.forward_current_ma = {} mA (nominal)",
            number(forward_current_ma)
        );
        let _ = writeln!(
            comment,
            "#   operating_conditions.max_current_ma = {cap_note}"
        );
        let _ = writeln!(comment, "#   datasheet: {datasheet}");
        let _ = writeln!(
            comment,
            "#   policy: forward_current_ma ±5%, rounded to 3 significant digits: \
             {min} A..{uncapped_max} A; final band {min} A..{max} A."
        );
        if reversed {
            let _ = writeln!(
                comment,
                "#   the LED is reversed: its anode is on ground and its cathode on net {}",
                toml_string(&other.name)
            );
        }
        if part.is_unverified() {
            let _ = writeln!(
                comment,
                "# UNREVIEWED: the part has no reviewed_by; check the datasheet before use."
            );
        }

        let name = self.unique("iled");
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Op,
            name,
            kind: "current",
            node: None,
            element: Some(component.refdes.clone()),
            min,
            max,
            comment,
            supply_net,
            supply,
            sweep: None,
        });
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
        if net.endpoints.len() != 2 {
            return;
        }
        let mut resistors = Vec::new();
        for endpoint in &net.endpoints {
            let Some(component) = design.component(endpoint.component) else {
                return;
            };
            if !is_kind(component, "resistor")
                || self.used.contains(&component.id)
                || resistors.contains(&component.id)
            {
                return;
            }
            resistors.push(component.id);
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
            self.todo_nets.insert(supply_net);
            self.not_covered.push(format!(
                "{}, no declared supply on net {}",
                self.describe(top),
                toml_string(&supply_name)
            ));
            return;
        };
        let (Some(top_ohms), Some(bottom_ohms)) = (self.resistance(top), self.resistance(bottom))
        else {
            self.not_covered.push(format!(
                "{}, no parseable `value` for the divider",
                self.describe(top)
            ));
            return;
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
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Op,
            name,
            kind: "node_voltage",
            node: Some(net.name.clone()),
            element: None,
            min,
            max,
            comment,
            supply_net,
            supply,
            sweep: None,
        });
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
        if supply.volts <= 0.0 {
            self.not_covered.push(format!(
                "{}, no positive declared supply on net {}",
                self.describe(resistor),
                toml_string(&input_name)
            ));
            return;
        }
        let (Some(resistance), Some(capacitance)) =
            (self.resistance(resistor), self.capacitance(capacitor))
        else {
            self.not_covered.push(format!(
                "{}, no parseable `value` for the RC filter",
                self.describe(resistor)
            ));
            return;
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

        let (resistance_value, capacitance_value) = (self.value(resistor), self.value(capacitor));
        let mut comment = String::new();
        let _ = writeln!(
            comment,
            "# measure: RC low-pass at net {}, fc = 1/(2*pi*R*C).",
            toml_string(&net.name)
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
            "#   1/(2*pi*{resistance_value}*{capacitance_value}) = {corner:.2} Hz; ≈±2% rounded to 3 significant digits: {min} Hz..{max} Hz."
        );

        let name = self.unique("corner");
        self.measures.push(DraftMeasure {
            analysis: AnalysisKind::Ac,
            name,
            kind: "relative_db",
            node: Some(net.name.clone()),
            element: None,
            min,
            max,
            comment,
            supply_net: input_net,
            supply,
            sweep: Some((start, stop)),
        });
    }

    fn report_unused(&mut self) {
        for component in &self.design.components {
            if !self.used.contains(&component.id) {
                self.not_covered.push(format!(
                    "{} ({}), not a recognised shape",
                    component.refdes,
                    component_part_id(component)
                ));
            }
        }
    }

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

        let mut sources: BTreeMap<NetId, &Supply> = BTreeMap::new();
        for measure in &self.measures {
            sources.entry(measure.supply_net).or_insert(&measure.supply);
        }
        for (net_id, supply) in sources {
            let name = self.design.net(net_id).map_or("", |net| net.name.as_str());
            let _ = writeln!(out);
            let _ = writeln!(out, "# source: {}", supply.note);
            let _ = writeln!(out, "[[source]]");
            let _ = writeln!(out, "net = {}", toml_string(name));
            let _ = writeln!(out, "kind = \"dc\"");
            if ac {
                let _ = writeln!(out, "ac = {}", float(supply.volts));
            } else {
                let _ = writeln!(out, "dc = {}", float(supply.volts));
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
            let _ = writeln!(out, "min = {}", float(measure.min));
            let _ = writeln!(out, "max = {}", float(measure.max));
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
                .is_some_and(synth_registry::Part::is_unverified);
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
        let first = *candidates.first()?;
        if candidates.iter().any(|candidate| candidate.0 != first.0) {
            return None;
        }
        let component = design.component(first.1)?;
        let pin = design.pin(first.1, first.2)?;
        Some(Supply {
            volts: first.0,
            note: format!(
                "net {} is driven by `{}.{}`, a power output that declares {} V; \
                 the part is modelled as an ideal source here",
                toml_string(&net.name),
                component.refdes,
                pin.name,
                number(first.0)
            ),
        })
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

    fn resistance(&self, component: ComponentId) -> Option<f64> {
        let value = parse_resistance(self.value(component))?;
        (value.is_finite() && value > 0.0).then_some(value)
    }

    fn capacitance(&self, component: ComponentId) -> Option<f64> {
        let value = parse_capacitance(self.value(component))?;
        (value.is_finite() && value > 0.0).then_some(value)
    }
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

fn number(value: f64) -> String {
    value.to_string()
}

/// A TOML float. `Debug` keeps `6.0` a float and never emits a bare integer.
fn float(value: f64) -> String {
    format!("{value:?}")
}

/// Round to `digits` significant digits so a generated limit matches the
/// hand-written testbench a human would write.
fn round_sig(value: f64, digits: i32) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let magnitude = value.abs().log10().floor() as i32;
    let factor = 10f64.powi(digits - 1 - magnitude);
    if !factor.is_finite() {
        return value;
    }
    (value * factor).round() / factor
}
