// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Electrical type of a pin. Expanded for more precision
/// for better ERC rule accuracy.
///
/// This is the *electrical* model: what direction current flows, what
/// drive strength to assume, etc. The *semantic* model — what protocol
/// the pin participates in — lives in [`PinCapability`].
///
/// Variants are ordered by specificity for ERC rule matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ElectricalType {
    /// Passive component terminal (resistor, capacitor, inductor).
    Passive,
    /// Power supply input pin (consumes power).
    PowerInput,
    /// Power supply output pin (provides power, e.g., regulator output).
    PowerOutput,
    /// Ground reference pin — distinct from generic PowerInput for
    /// precise ERC rules (ground bounce, star grounding, etc.).
    GroundReference,
    /// Bidirectional I/O (can be input or output depending on config).
    Bidirectional,
    /// Digital input only.
    Input,
    /// Digital output only (push-pull).
    Output,
    /// Tri-statable output — can be high-Z (e.g., bus transceiver).
    ThreeStatable,
    /// Open-drain / open-collector, active-low output.
    OpenDrainLow,
    /// Open-drain / open-emitter, active-high output.
    OpenDrainHigh,
    /// Analog signal (continuous voltage).
    Analog,
    /// RF signal (high-frequency).
    Rf,
    /// Differential pair positive member.
    DifferentialPositive,
    /// Differential pair negative member.
    DifferentialNegative,
    /// Clock signal — modeled as electrical type for drive-strength ERC.
    Clock,
    /// Manufacturer-marked no-connect (must remain floating).
    DoNotConnect,
    /// Author has not classified the pin yet (default for new pins).
    #[default]
    Unclassified,
}

impl ElectricalType {
    /// Returns true if this type represents a power-related pin.
    pub fn is_power(&self) -> bool {
        matches!(
            self,
            ElectricalType::PowerInput
                | ElectricalType::PowerOutput
                | ElectricalType::GroundReference
        )
    }

    /// Returns true if this type represents a digital output driver.
    pub fn is_output_driver(&self) -> bool {
        matches!(
            self,
            ElectricalType::Output
                | ElectricalType::ThreeStatable
                | ElectricalType::OpenDrainLow
                | ElectricalType::OpenDrainHigh
        )
    }

    /// Returns true if this type can sink current (output or open-drain).
    pub fn can_sink(&self) -> bool {
        matches!(
            self,
            ElectricalType::Output
                | ElectricalType::ThreeStatable
                | ElectricalType::OpenDrainLow
                | ElectricalType::OpenDrainHigh
        )
    }

    /// Returns true if this type can source current (output or three-state high).
    pub fn can_source(&self) -> bool {
        matches!(
            self,
            ElectricalType::Output | ElectricalType::ThreeStatable | ElectricalType::OpenDrainHigh
        )
    }

    /// Returns true if this type is high-impedance (input, passive, analog).
    pub fn is_high_z(&self) -> bool {
        matches!(
            self,
            ElectricalType::Input
                | ElectricalType::Passive
                | ElectricalType::Analog
                | ElectricalType::Rf
                | ElectricalType::DifferentialPositive
                | ElectricalType::DifferentialNegative
        )
    }

    /// Returns true if this type represents a differential signal.
    pub fn is_differential(&self) -> bool {
        matches!(
            self,
            ElectricalType::DifferentialPositive | ElectricalType::DifferentialNegative
        )
    }

    /// Returns true if this type is a clock signal.
    pub fn is_clock(&self) -> bool {
        matches!(self, ElectricalType::Clock)
    }

    /// Returns true if this pin must not be connected.
    pub fn is_no_connect(&self) -> bool {
        matches!(self, ElectricalType::DoNotConnect)
    }

    /// Convert to legacy OpenDrain for backward compatibility.
    /// Maps both OpenDrainLow and OpenDrainHigh to OpenDrainLow.
    pub fn to_legacy_open_drain(self) -> Option<ElectricalType> {
        match self {
            ElectricalType::OpenDrainLow | ElectricalType::OpenDrainHigh => {
                Some(ElectricalType::OpenDrainLow)
            }
            _ => None,
        }
    }
}

/// Semantic capability tag. One pin may have many capabilities (a GPIO
/// can typically be muxed to SPI MOSI, I²C SDA, UART TX, etc.). ERC
/// rules consume these tags to validate protocol-level correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PinCapability {
    Gpio,

    // USB
    UsbDp,
    UsbDn,
    UsbVbus,
    UsbCc,

    // SPI
    SpiMosi,
    SpiMiso,
    SpiSck,
    SpiCs,

    // I²C
    I2cSda,
    I2cScl,

    // UART
    UartTx,
    UartRx,

    // Analog
    AnalogInput,
    AnalogOutput,

    // Clocking / control
    ClockInput,
    ClockOutput,
    Reset,
    BootMode,

    // RF
    RfFeed,

    // Differential
    DiffPairPositive,
    DiffPairNegative,
}

impl PinCapability {
    /// Returns the electrical type a pin *must* have in order to be
    /// allowed to declare this capability. Returns `None` if any
    /// electrical type is compatible (e.g., `Gpio` can be on a
    /// bidirectional, input, output, or open-drain pin).
    pub fn required_electrical_type(self) -> Option<ElectricalType> {
        match self {
            PinCapability::AnalogInput | PinCapability::AnalogOutput => {
                Some(ElectricalType::Analog)
            }
            PinCapability::ClockInput | PinCapability::ClockOutput => Some(ElectricalType::Clock),
            PinCapability::DiffPairPositive | PinCapability::UsbDp => {
                Some(ElectricalType::DifferentialPositive)
            }
            PinCapability::DiffPairNegative | PinCapability::UsbDn => {
                Some(ElectricalType::DifferentialNegative)
            }
            PinCapability::RfFeed => Some(ElectricalType::Rf),
            // Everything else is compatible with multiple electrical types.
            _ => None,
        }
    }

    /// Check if this capability is compatible with the given electrical type.
    pub fn compatible_with(self, et: ElectricalType) -> bool {
        match self.required_electrical_type() {
            Some(required) => required == et,
            None => true, // No strict requirement
        }
    }

    /// The canonical, datasheet-style name for this function, used as
    /// the KiCad *pin alternate* label (e.g. `I2C_SDA`, `SPI_MOSI`).
    /// Uppercase ASCII with `_` separators so it is a legal KiCad pin
    /// name and reads like a peripheral function table.
    pub fn canonical_name(self) -> &'static str {
        match self {
            PinCapability::Gpio => "GPIO",
            PinCapability::UsbDp => "USB_DP",
            PinCapability::UsbDn => "USB_DN",
            PinCapability::UsbVbus => "USB_VBUS",
            PinCapability::UsbCc => "USB_CC",
            PinCapability::SpiMosi => "SPI_MOSI",
            PinCapability::SpiMiso => "SPI_MISO",
            PinCapability::SpiSck => "SPI_SCK",
            PinCapability::SpiCs => "SPI_CS",
            PinCapability::I2cSda => "I2C_SDA",
            PinCapability::I2cScl => "I2C_SCL",
            PinCapability::UartTx => "UART_TX",
            PinCapability::UartRx => "UART_RX",
            PinCapability::AnalogInput => "ANALOG_IN",
            PinCapability::AnalogOutput => "ANALOG_OUT",
            PinCapability::ClockInput => "CLK_IN",
            PinCapability::ClockOutput => "CLK_OUT",
            PinCapability::Reset => "RESET",
            PinCapability::BootMode => "BOOT",
            PinCapability::RfFeed => "RF",
            PinCapability::DiffPairPositive => "DIFF_P",
            PinCapability::DiffPairNegative => "DIFF_N",
        }
    }

    /// Infer the function a **net name** denotes, e.g. `I2C1_SCL` →
    /// [`PinCapability::I2cScl`], `UART0_TX` → `UartTx`,
    /// `USB_DP` → `UsbDp`, `SPI1_MOSI` → `SpiMosi`.
    ///
    /// Deliberately conservative, in the codebase's "decline rather
    /// than guess" style: `SCL`/`SDA`/`MOSI`/`MISO`/`SCK` are unique to
    /// one protocol and stand alone, but `TX`/`RX`/`CS`/`DP` are far
    /// too common, so they only resolve when the name also carries a
    /// protocol word (`UART`, `SPI`, `USB`). Anything else returns
    /// `None` — an unrecognised name must not invent a requirement.
    #[must_use]
    pub fn from_net_name(name: &str) -> Option<PinCapability> {
        let upper = name.to_ascii_uppercase();
        // `D+` / `D-` are the one role KiCad spells with punctuation,
        // so check the raw string before the token split eats it.
        let has_token = |needle: &str| {
            upper
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|t| t == needle)
        };
        let has_prefix = |prefix: &str| {
            upper
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|t| t.starts_with(prefix))
        };

        let is_spi = has_prefix("SPI");
        let is_uart = has_prefix("UART") || has_prefix("USART") || has_token("SERIAL");
        let is_usb = has_prefix("USB");

        // Unambiguous single-protocol roles.
        if has_token("SCL") {
            return Some(PinCapability::I2cScl);
        }
        if has_token("SDA") {
            return Some(PinCapability::I2cSda);
        }
        if has_token("MOSI") {
            return Some(PinCapability::SpiMosi);
        }
        if has_token("MISO") {
            return Some(PinCapability::SpiMiso);
        }
        if has_token("SCK") || has_token("SCLK") {
            return Some(PinCapability::SpiSck);
        }
        // Context-dependent roles.
        if has_token("NSS") || (is_spi && (has_token("CS") || has_token("SS"))) {
            return Some(PinCapability::SpiCs);
        }
        if is_uart && (has_token("TX") || has_token("TXD")) {
            return Some(PinCapability::UartTx);
        }
        if is_uart && (has_token("RX") || has_token("RXD")) {
            return Some(PinCapability::UartRx);
        }
        if is_usb && has_token("VBUS") {
            return Some(PinCapability::UsbVbus);
        }
        if upper.contains("D+") || (is_usb && has_token("DP")) {
            return Some(PinCapability::UsbDp);
        }
        if upper.contains("D-") || (is_usb && has_token("DM")) {
            return Some(PinCapability::UsbDn);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electrical_type_properties() {
        assert!(ElectricalType::PowerOutput.is_power());
        assert!(ElectricalType::GroundReference.is_power());
        assert!(!ElectricalType::Output.is_power());

        assert!(ElectricalType::Output.is_output_driver());
        assert!(ElectricalType::OpenDrainLow.is_output_driver());
        assert!(!ElectricalType::Input.is_output_driver());

        assert!(ElectricalType::OpenDrainLow.can_sink());
        assert!(!ElectricalType::OpenDrainLow.can_source());
        assert!(ElectricalType::OpenDrainHigh.can_source());
        assert!(ElectricalType::OpenDrainHigh.can_sink());

        assert!(ElectricalType::DifferentialPositive.is_differential());
        assert!(ElectricalType::Clock.is_clock());
        assert!(ElectricalType::DoNotConnect.is_no_connect());
    }

    #[test]
    fn pin_capability_compatibility() {
        assert!(PinCapability::ClockOutput.compatible_with(ElectricalType::Clock));
        assert!(!PinCapability::ClockOutput.compatible_with(ElectricalType::Output));
        assert!(PinCapability::Gpio.compatible_with(ElectricalType::Bidirectional));
        assert!(PinCapability::Gpio.compatible_with(ElectricalType::Output));
        assert!(
            PinCapability::DiffPairPositive.compatible_with(ElectricalType::DifferentialPositive)
        );
        assert!(!PinCapability::DiffPairPositive.compatible_with(ElectricalType::Output));
    }

    #[test]
    fn canonical_names_are_uppercase_identifiers() {
        assert_eq!(PinCapability::I2cScl.canonical_name(), "I2C_SCL");
        assert_eq!(PinCapability::SpiMosi.canonical_name(), "SPI_MOSI");
        assert_eq!(PinCapability::UsbDp.canonical_name(), "USB_DP");
        assert_eq!(PinCapability::UartTx.canonical_name(), "UART_TX");
    }

    #[test]
    fn net_name_function_inference() {
        // Unambiguous single-protocol roles, with and without prefix.
        assert_eq!(
            PinCapability::from_net_name("I2C1_SCL"),
            Some(PinCapability::I2cScl)
        );
        assert_eq!(
            PinCapability::from_net_name("scl"),
            Some(PinCapability::I2cScl)
        );
        assert_eq!(
            PinCapability::from_net_name("SDA"),
            Some(PinCapability::I2cSda)
        );
        assert_eq!(
            PinCapability::from_net_name("I2C0.sda"),
            Some(PinCapability::I2cSda)
        );
        assert_eq!(
            PinCapability::from_net_name("SPI1_MOSI"),
            Some(PinCapability::SpiMosi)
        );
        assert_eq!(
            PinCapability::from_net_name("SPI_SCK"),
            Some(PinCapability::SpiSck)
        );
        assert_eq!(
            PinCapability::from_net_name("USB_DP"),
            Some(PinCapability::UsbDp)
        );
        assert_eq!(
            PinCapability::from_net_name("USB_D-"),
            Some(PinCapability::UsbDn)
        );
        assert_eq!(
            PinCapability::from_net_name("USB_VBUS"),
            Some(PinCapability::UsbVbus)
        );
        // Context-dependent roles need their protocol word.
        assert_eq!(
            PinCapability::from_net_name("UART0_TX"),
            Some(PinCapability::UartTx)
        );
        assert_eq!(
            PinCapability::from_net_name("UART0_RXD"),
            Some(PinCapability::UartRx)
        );
        assert_eq!(
            PinCapability::from_net_name("SPI1_CS"),
            Some(PinCapability::SpiCs)
        );
        // Bare TX/RX/CS/DP must not invent a requirement.
        assert_eq!(PinCapability::from_net_name("TX"), None);
        assert_eq!(PinCapability::from_net_name("RX"), None);
        assert_eq!(PinCapability::from_net_name("CS"), None);
        assert_eq!(PinCapability::from_net_name("DP"), None);
        // Unrelated names stay unrecognised.
        assert_eq!(PinCapability::from_net_name("3V3"), None);
        assert_eq!(PinCapability::from_net_name("RESET"), None);
        assert_eq!(PinCapability::from_net_name("net_4"), None);
        assert_eq!(PinCapability::from_net_name("ALERT_1"), None);
    }
}
