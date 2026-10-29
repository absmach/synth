// SPDX-License-Identifier: Apache-2.0

//! The Class A irrigation controller's parts carry a reviewer, a datasheet and
//! the electrical data the power checks read, not only a KiCad symbol.

use std::path::Path;

const IDS: [&str; 7] = [
    "stm32g030k8_lqfp32",
    "sht30_dis",
    "apds9301_real",
    "xtal_8mhz_3225_4p",
    "ao3400a_nmos",
    "header_1x5",
    "24lc256_soic8",
];

fn part(id: &str) -> synth_registry::Part {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/parts");
    synth_registry::load_dir(&dir)
        .expect("seed registry must load")
        .iter()
        .find(|(k, _)| k.as_str() == id)
        .unwrap_or_else(|| panic!("{id} is missing"))
        .1
        .clone()
}

#[test]
fn every_irrigation_part_has_a_reviewer_a_date_a_datasheet_and_qualifies_without_findings() {
    use synth_registry::qualify::{qualify_part, NoFacts};
    for id in IDS {
        let p = part(id);
        let prov = p
            .provenance
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no provenance"));
        assert!(prov.reviewed_by.as_deref().is_some_and(|v| !v.is_empty()));
        assert!(prov.reviewed_at.as_deref().is_some_and(|v| !v.is_empty()));
        assert!(
            prov.datasheet_url
                .as_deref()
                .is_some_and(|u| u.starts_with("http")),
            "{id}: datasheet_url under [provenance]"
        );
        let q = qualify_part(&p, &NoFacts, &NoFacts);
        let found: Vec<&str> = q.findings().map(|f| f.code.as_str()).collect();
        assert!(found.is_empty(), "{id}: qualification findings {found:?}");
    }
}

#[test]
fn the_ics_declare_their_supply_range_and_decoupling() {
    for (id, lo, hi, net) in [
        ("stm32g030k8_lqfp32", 2.0, 3.6, "VDD"),
        ("sht30_dis", 2.15, 5.5, "VDD"),
        ("apds9301_real", 2.4, 3.6, "VDD"),
    ] {
        let p = part(id);
        let oc = p.operating_conditions.as_ref().expect("a range");
        assert_eq!((oc.min_voltage_v, oc.max_voltage_v), (Some(lo), Some(hi)));
        assert!(
            p.required_decoupling.iter().any(|d| d.net == net),
            "{id}: no decoupling on {net}"
        );
    }
}

#[test]
fn the_light_sensor_interrupt_is_open_drain() {
    let p = part("apds9301_real");
    let int = p.pins.iter().find(|x| x.name == "INT").unwrap();
    assert_eq!(
        int.electrical_type,
        synth_registry::ElectricalType::OpenDrainLow
    );
}

#[test]
fn the_crystal_is_an_exact_part_with_a_stated_load() {
    let p = part("xtal_8mhz_3225_4p");
    assert_eq!(p.mpn.as_deref(), Some("NX3225GD-8MHZ-STD-CRA-3"));
    let d = p.description.unwrap();
    assert!(d.contains("load capacitance 8 pF") && !d.contains("assum"));
}

fn caps(p: &synth_registry::Part, pin: &str) -> String {
    format!(
        "{:?}",
        p.pins
            .iter()
            .find(|x| x.name == pin)
            .unwrap_or_else(|| panic!("{}: no pin {pin}", p.id))
            .capabilities
    )
}

#[test]
fn the_mcu_pins_carry_the_bus_functions_of_the_datasheets_alternate_function_tables() {
    // DS12991 Rev 6, tables 13 and 14: I2C1 on PB6/PB7 and PB8/PB9, USART1 on PB6/PB7,
    // SPI1 on PA5/PA6/PA7 (SCK/MISO/MOSI). The LQFP32 package has no PB10.
    let p = part("stm32g030k8_lqfp32");
    for (pin, wanted) in [
        ("PB6", ["Gpio", "I2cScl", "UartTx"].as_slice()),
        ("PB7", ["Gpio", "I2cSda", "UartRx"].as_slice()),
        ("PB8", ["Gpio", "I2cScl"].as_slice()),
        ("PB9", ["Gpio", "I2cSda"].as_slice()),
        ("PA5", ["SpiSck"].as_slice()),
        ("PA6", ["SpiMiso"].as_slice()),
        ("PA7", ["SpiMosi"].as_slice()),
    ] {
        let c = caps(&p, pin);
        for cap in wanted {
            assert!(c.contains(cap), "{pin} should be {cap}: {c}");
        }
    }
    assert!(
        p.find_pin("PB10").is_none(),
        "the LQFP32 package has no PB10 pin"
    );
    // A pin with no I2C function in the tables must not claim one.
    assert!(!caps(&p, "PA5").contains("I2c"));
    assert!(caps(&p, "NRST").contains("Reset"));
}

#[test]
fn the_i2c_parts_declare_their_bus_pins() {
    for (id, sda, scl) in [("sht30_dis", "SDA", "SCL"), ("apds9301_real", "SDA", "SCL")] {
        let p = part(id);
        assert!(caps(&p, sda).contains("I2cSda"), "{id}: {sda}");
        assert!(caps(&p, scl).contains("I2cScl"), "{id}: {scl}");
    }
}

#[test]
fn the_humidity_alert_is_open_drain() {
    let p = part("sht30_dis");
    let alert = p.pins.iter().find(|x| x.name == "ALERT").unwrap();
    assert_eq!(
        alert.electrical_type,
        synth_registry::ElectricalType::OpenDrainLow,
        "ALERT pulls low and needs a pull-up"
    );
}

#[test]
fn the_mosfet_carries_its_datasheet_ratings() {
    // AOS AO3400A: VDS 30 V, ID 5.7 A at 25 C, PD 1.4 W, VGS +/-12 V, RthJA 125 C/W steady-state.
    let p = part("ao3400a_nmos");
    let oc = p.operating_conditions.as_ref().expect("ratings");
    assert_eq!(oc.max_voltage_v, Some(30.0));
    assert_eq!(oc.max_current_ma, Some(5700.0));
    assert_eq!(oc.max_power_mw, Some(1400.0));
    assert_eq!(oc.thermal_resistance_c_per_w, Some(125.0));
    let gate = p.pins.iter().find(|x| x.name == "g").unwrap();
    assert_eq!(gate.voltage_max_v, Some(12.0), "the gate takes +/-12 V");
}

#[test]
fn the_canonical_mp2307_limits_are_the_datasheets() {
    // MPS MP2307 datasheet: "3A, 23V, 340kHz"; recommended input 4.75 V to 23 V.
    // The description and the operating limits must agree with each other.
    let p = part("mp2307_buck");
    let description = p.description.as_deref().unwrap_or_default();
    assert!(description.contains("3A"), "{description}");
    let limits = p.operating_conditions.as_ref().expect("operating limits");
    assert_eq!(limits.max_current_ma, Some(3000.0));
    assert_eq!(limits.min_voltage_v, Some(4.75));
    assert_eq!(limits.max_voltage_v, Some(23.0));
}
