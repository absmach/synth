# Synth component registry

Clean-slate, hand-authored part definitions. Every byte that flows
through the Synth compiler is owned and understood by Synth — no
foreign-library imports at compile time.

## Layout

```
registry/parts/
  connectors/           # Pin headers, USB-C, JST connectors
  crystals/             # Oscillators and crystals
  diodes/               # Diodes, LEDs, TVS, Zeners
  ic/                   # Logic, shift registers, multiplexers
  mcus/                 # Microcontrollers (RP2040, STM32, ESP32, etc.)
  memory/               # Flash, EEPROM, SRAM
  opamps/               # Operational amplifiers and comparators
  passives/             # Generic 0402/0603/0805 R, C, L
  protection/           # ESD and overvoltage protection
  regulators/           # LDOs and switching buck/boost converters
  rf/                   # Modems, sub-GHz, Bluetooth/Wi-Fi modules
  sensors/              # Environmental, IMU, temperature sensors
  switches/             # Push buttons, tactile, DIP switches
  transistors/          # MOSFETs, BJTs
```

Each `<id>.synth.toml` file defines exactly one part. The filename stem
must match the `id` field; the loader enforces this.

### User & Custom Components (Runtime Tiers)
At runtime, custom or user-imported parts are loaded from:
- **Project-local parts:** `./parts/<id>.synth.toml` (versioned with your board)
- **User cache:** `~/.local/share/synth/registry/parts/` (XDG Tier-2 directory)

## Schema (V1)

```toml
id = "rp2350"
kind = "mcu"
description = "..."

[[pins]]
name = "gp0"                   # logical pin name used in SynthSpec
number = "3"                   # physical pin number from datasheet
electrical_type = "bidirectional"
capabilities = ["gpio", "spi_mosi", "uart_tx"]
required = false               # connection mandatory for the part to function

[[required_decoupling]]
net = "vdd_io"
value = "100nf"
count = 4
```

The `electrical_type` values mirror the standard EDA categorization
(passive, power_input, power_output, bidirectional, input, output,
open_drain, analog, rf, differential_positive, differential_negative,
no_connect).

The `capabilities` list is the *semantic* layer. A capability like
`usb_dp` may require a specific electrical type (see
[`PinCapability::required_electrical_type`](../crates/synth-registry/src/capability.rs));
the loader rejects parts that violate this.

## Seed corpus

The V1 seed corpus is intentionally small — three parts covering the
three principal categories (active digital, secure peripheral, passive).
