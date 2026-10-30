# RTD Analog Front End — Qualification Record

## Board: RTD Analog Front End (2-layer)
## Issue: #365 (Class B qualification)

## DRC Evidence
- KiCad DRC: 4 violations (3 silk_overlap + 1 silk_over_copper, all silkscreen warnings)
- 0 unconnected pads (all 7 nets connected)
- KiCad version: 10.0.7
- Command: `kicad-cli pcb drc --refill-zones <board>.kicad_pcb`

## ERC Evidence
- Synth ERC: 0 errors, 4 warnings (supply identity, decoupling separation, junction fan-out)
- See reports/rtd_front_end.erc.json

## Analog Design
- LM358 non-inverting gain stage: gain = 1 + 30k/10k = 4x
- PT100: 100-212Ω over 0-300°C
- Excitation: 3.3V / 1kΩ = 3.3mA
- Divider output: 0.30-0.58V
- Amplified output: 1.2-2.3V (fits LM358 2.5V max on 3.3V supply)
- RC low-pass filter: fc ~160 Hz (10kΩ + 100nF)
- LM358 is NOT rail-to-rail; output limited to ~2.5V on 3.3V supply

## Class-B Scope
- Parent requirement: 4-layer, ADS124S08, REF5025, RP2350, 4-wire RTD, AGND/DGND single-point tie
- Current implementation: 2-layer, LM358, 2-wire RTD, no precision ADC/reference
- **This board does NOT satisfy the Class-B requirement**
- Moved to separate non-qualification example or implement full Class-B board

## Per-Part Datasheet Review
| Part | MPN | Footprint | Status |
|------|-----|-----------|--------|
| LM358 | LM358 | SOIC-8 | Dual opamp, not rail-to-rail |
| Generic resistors | - | 0603 | Various values |
| Generic capacitors | - | 0603/0805 | Various values |

## Status: NOT QUALIFIED
