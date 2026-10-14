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
  fpga/                 # Field-programmable gate arrays
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

The `capabilities` list is the _semantic_ layer. A capability like
`usb_dp` may require a specific electrical type (see
[`PinCapability::required_electrical_type`](../crates/synth-registry/src/capability.rs));
the loader rejects parts that violate this.

## Seed corpus

The V1 seed corpus is intentionally small — three parts covering the
three principal categories (active digital, secure peripheral, passive).

## Qualification

`synth registry qualify` checks every part against the KiCad footprint and
symbol it names, rather than only checking that both exist. A part can load
cleanly, carry provenance and reference a real footprint while still having a
wrong pin map, and that mismatch is invisible until fabrication.

```bash
synth registry qualify                       # human summary, non-zero if not clean
synth registry qualify --part rp2350         # one part
synth registry qualify --report review.json  # the review artifact
```

Eight checks run per part:

| Check | Catches |
| --- | --- |
| `pin_numbering` | duplicate pad numbers, duplicate pin names, blank numbers |
| `pin_pad_coverage` | a pin claiming a pad the footprint does not have |
| `pad_pin_coverage` | an exposed or thermal pad missing from the pin map |
| `footprint_side` | a footprint whose copper is entirely on B.Cu (mirrored) |
| `symbol_pin_coverage` | a pin number absent from the referenced KiCad symbol |
| `package_dimensions` | gross size errors, naming mil/mm confusion or a lost decimal |
| `pin_classification` | a no-connect pin marked required; unclassified pins |
| `provenance` | no reviewer, no datasheet, a missing or garbled review date |

Each check reports `pass`, `fail`, `unknown` or `not_applicable`. The
distinction matters: `not_applicable` means the part declares nothing to
compare against, such as a part with no `kicad_symbol` whose symbol the
exporter synthesizes, while `unknown` means a check that should have run could
not — usually because KiCad's libraries are not installed. An `unknown` part is
not fabrication-safe, because nothing was proven about it.

Findings are `blocking` or `review`. Manufacturing export refuses a board whose
parts carry **structural** blocking findings — a pin map that disagrees with the
named footprint — with no override, since that is simply wrong rather than a
judgement call. Review gaps stay under the existing
`--allow-unverified-parts` gate.

### What qualification cannot prove

A permutation of pin numbers among pads that all exist is not detectable from
the footprint: if `vdd` and `gnd` are swapped between two real pads, every
automated check still passes. Only a reviewer comparing against the datasheet
catches that, which is what `[provenance].reviewed_by` records. Qualification
narrows what a human has to check; it does not replace the check.

The mutation corpus in `fixtures/registry-qualify/` pins each detectable
failure mode to the check that catches it.
