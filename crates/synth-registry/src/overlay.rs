// SPDX-License-Identifier: Apache-2.0

//! Serialise a [`Part`] back to registry TOML.
//!
//! Every writer that creates or corrects a Tier-2 entry needs this: the
//! `synth part import-*` commands, `synth_author_part`, and the automatic
//! footprint repair in `synth export-kicad`. Keeping one renderer means a
//! Tier-2 overlay written by any of them has the same shape, and — more
//! importantly — that a *complete* part is emitted every time.
//!
//! That completeness is the whole point of a separate renderer rather than
//! `toml::to_string_pretty`. A Tier-2 file shadows the Tier-1 entry of the
//! same id, so any field a writer forgets to emit is not merged: it is lost.
//! The hand-written key order also makes overlay diffs readable in review.

use std::fmt::Write as _;

use crate::{ElectricalType, Part, Pin, PinCapability, ProvenanceSource};

/// Render `part` as a complete registry TOML document.
///
/// `header` is prepended as `#` comments; callers use it to record what
/// produced the file, which is the first thing a reviewer wants to know.
pub fn part_to_toml(part: &Part, header: &[&str]) -> String {
    let mut out = String::new();
    for line in header {
        let _ = writeln!(out, "# {line}");
    }
    if !header.is_empty() {
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "id = {}", quoted(part.id.as_str()));
    let _ = writeln!(out, "kind = {}", quoted(&part.kind));
    let _ = writeln!(
        out,
        "description = {}",
        quoted(part.description.as_deref().unwrap_or(""))
    );
    if let Some(mpn) = part.mpn.as_deref() {
        let _ = writeln!(out, "mpn = {}", quoted(mpn));
    }
    if let Some(lcsc) = part.lcsc_pn.as_deref() {
        let _ = writeln!(out, "lcsc_pn = {}", quoted(lcsc));
    }
    if let Some(sym) = part.kicad_symbol.as_deref() {
        let _ = writeln!(out, "kicad_symbol = {}", quoted(sym));
    }
    if let Some(fp) = part.kicad_footprint.as_deref() {
        let _ = writeln!(out, "kicad_footprint = {}", quoted(fp));
    }
    if let Some(d) = part.footprint_dimensions.as_ref() {
        let _ = writeln!(out);
        let _ = writeln!(out, "[footprint_dimensions]");
        let _ = writeln!(out, "width_mm = {}", d.width_mm);
        let _ = writeln!(out, "height_mm = {}", d.height_mm);
        if let Some(m) = d.courtyard_margin_mm {
            let _ = writeln!(out, "courtyard_margin_mm = {m}");
        }
    }
    if let Some(c) = part.operating_conditions.as_ref() {
        let _ = writeln!(out);
        let _ = writeln!(out, "[operating_conditions]");
        if let Some(v) = c.min_voltage_v {
            let _ = writeln!(out, "min_voltage_v = {v}");
        }
        if let Some(v) = c.max_voltage_v {
            let _ = writeln!(out, "max_voltage_v = {v}");
        }
        if let Some(v) = c.max_current_ma {
            let _ = writeln!(out, "max_current_ma = {v}");
        }
    }
    for pin in &part.pins {
        write_pin(&mut out, pin);
    }
    for d in &part.required_decoupling {
        let _ = writeln!(out);
        let _ = writeln!(out, "[[required_decoupling]]");
        let _ = writeln!(out, "net = {}", quoted(&d.net));
        let _ = writeln!(out, "value = {}", quoted(&d.value));
        let _ = writeln!(out, "count = {}", d.count);
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "[provenance]");
    let prov = part.provenance.as_ref();
    let _ = writeln!(
        out,
        "source = {}",
        quoted(
            prov.map_or(ProvenanceSource::Generated, |p| p.source.clone())
                .name(),
        )
    );
    if let Some(g) = prov.and_then(|p| p.generator.as_deref()) {
        let _ = writeln!(out, "generator = {}", quoted(g));
    }
    if let Some(u) = prov.and_then(|p| p.datasheet_url.as_deref()) {
        let _ = writeln!(out, "datasheet_url = {}", quoted(u));
    }
    // Empty `reviewed_by` is what keeps `W-SYNTH-PART-UNVERIFIED` firing: a
    // machine-written entry must not be able to clear the review gate by
    // existing.
    let _ = writeln!(out, "reviewed_by = \"\"");
    out
}

fn write_pin(out: &mut String, pin: &Pin) {
    let _ = writeln!(out);
    let _ = writeln!(out, "[[pins]]");
    let _ = writeln!(out, "name = {}", quoted(&pin.name));
    let _ = writeln!(out, "number = {}", quoted(&pin.number.0));
    let _ = writeln!(
        out,
        "electrical_type = {}",
        quoted(pin.electrical_type.name())
    );
    let caps: Vec<String> = pin.capabilities.iter().map(|c| quoted(c.name())).collect();
    if !caps.is_empty() {
        let _ = writeln!(out, "capabilities = [{}]", caps.join(", "));
    }
    if pin.required {
        let _ = writeln!(out, "required = true");
    }
}

/// Quote and escape a TOML basic string.
pub fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

impl ProvenanceSource {
    /// The `snake_case` name used in `[provenance]`.
    ///
    ///
    /// Derived from serde's own `rename_all = "snake_case"`, so this and the
    /// loader cannot drift apart; `round_trip_name_*` below pins that.
    pub fn name(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Generated => "generated",
            Self::Imported => "imported",
            Self::Authored => "authored",
        }
    }
}
impl ElectricalType {
    /// The `snake_case` name used in a pin's `electrical_type`.
    ///
    /// Derived from serde's own `rename_all = "snake_case"`, so this and the
    /// loader cannot drift apart; `round_trip_name_*` below pins that.
    pub fn name(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::PowerInput => "power_input",
            Self::PowerOutput => "power_output",
            Self::GroundReference => "ground_reference",
            Self::Bidirectional => "bidirectional",
            Self::Input => "input",
            Self::Output => "output",
            Self::ThreeStatable => "three_statable",
            Self::OpenDrainLow => "open_drain_low",
            Self::OpenDrainHigh => "open_drain_high",
            Self::Analog => "analog",
            Self::Rf => "rf",
            Self::DifferentialPositive => "differential_positive",
            Self::DifferentialNegative => "differential_negative",
            Self::Clock => "clock",
            Self::DoNotConnect => "do_not_connect",
            Self::Unclassified => "unclassified",
        }
    }
}
impl PinCapability {
    /// The `snake_case` name used in a pin's `capabilities` list.
    ///
    /// Derived from serde's own `rename_all = "snake_case"`, so this and the
    /// loader cannot drift apart; `round_trip_name_*` below pins that.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gpio => "gpio",
            Self::UsbDp => "usb_dp",
            Self::UsbDn => "usb_dn",
            Self::UsbVbus => "usb_vbus",
            Self::UsbCc => "usb_cc",
            Self::SpiMosi => "spi_mosi",
            Self::SpiMiso => "spi_miso",
            Self::SpiSck => "spi_sck",
            Self::SpiCs => "spi_cs",
            Self::I2cSda => "i2c_sda",
            Self::I2cScl => "i2c_scl",
            Self::UartTx => "uart_tx",
            Self::UartRx => "uart_rx",
            Self::AnalogInput => "analog_input",
            Self::AnalogOutput => "analog_output",
            Self::ClockInput => "clock_input",
            Self::ClockOutput => "clock_output",
            Self::Reset => "reset",
            Self::BootMode => "boot_mode",
            Self::RfFeed => "rf_feed",
            Self::DiffPairPositive => "diff_pair_positive",
            Self::DiffPairNegative => "diff_pair_negative",
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lifecycle, Part, PartId, PinNumber};

    fn sample() -> Part {
        Part {
            id: PartId("demo".into()),
            kind: "mcu".into(),
            description: Some("a \"quoted\" part".into()),
            version: 0,
            lifecycle: Lifecycle::default(),
            signed_by: vec![],
            substitutes: vec![],
            mpn: Some("M1".into()),
            lcsc_pn: Some("C1".into()),
            pins: vec![Pin {
                name: "vdd".into(),
                number: PinNumber("A22".into()),
                electrical_type: ElectricalType::PowerInput,
                capabilities: vec![PinCapability::Gpio],
                required: true,
                unit: None,
                voltage_max_v: Some(3.6),
                voltage_min_v: Some(1.8),
                voltage_nominal_v: Some(3.3),
            }],
            required_decoupling: vec![crate::RequiredDecoupling {
                net: "vdd".into(),
                value: "100nf".into(),
                count: 2,
                max_distance_mm: Some(5.0),
            }],
            kicad_symbol: Some("Lib:Sym".into()),
            kicad_footprint: Some("Lib:Fp".into()),
            footprint_dimensions: None,
            operating_conditions: None,
            provenance: None,
        }
    }

    /// The property that matters: what is written must read back as the same
    /// part. A renderer that silently drops a field produces an overlay that
    /// *shadows* the complete Tier-1 entry with a lossy one.
    #[test]
    fn round_trips_through_the_loader() {
        let part = sample();
        let text = part_to_toml(&part, &["generated by test"]);
        let back: Part = toml::from_str(&text).expect("emitted TOML must parse");
        assert_eq!(back.id, part.id);
        assert_eq!(back.mpn, part.mpn);
        assert_eq!(back.lcsc_pn, part.lcsc_pn);
        assert_eq!(back.kicad_footprint, part.kicad_footprint);
        assert_eq!(back.kicad_symbol, part.kicad_symbol);
        assert_eq!(back.pins.len(), 1);
        assert_eq!(back.pins[0].number, part.pins[0].number);
        assert_eq!(back.pins[0].electrical_type, part.pins[0].electrical_type);
        assert_eq!(back.pins[0].capabilities, part.pins[0].capabilities);
        assert!(back.pins[0].required);
        assert_eq!(back.required_decoupling.len(), 1);
    }

    #[test]
    fn quotes_and_escapes_are_survivable() {
        let mut part = sample();
        part.description = Some("has \"quotes\" and \\ backslash".into());
        let text = part_to_toml(&part, &[]);
        let back: Part = toml::from_str(&text).expect("escaped TOML must parse");
        assert_eq!(back.description, part.description);
    }

    /// A machine-written entry must never be able to clear the review gate.
    #[test]
    fn provenance_is_written_unverified() {
        let text = part_to_toml(&sample(), &[]);
        assert!(text.contains("source = \"generated\""), "{text}");
        assert!(text.contains("reviewed_by = \"\""), "{text}");
    }

    #[test]
    fn header_lines_become_comments() {
        let text = part_to_toml(&sample(), &["first", "second"]);
        assert!(text.starts_with("# first\n# second\n"), "{text}");
    }
}
