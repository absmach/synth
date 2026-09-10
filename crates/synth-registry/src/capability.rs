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
}
