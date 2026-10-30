# BLE Environment Node — Qualification Record

## Board: BLE Environment Node (4-layer)
## Issue: #365 (Class B qualification)

## DRC Evidence
- KiCad DRC: 6 violations (2 silk_over_copper warnings, 2 hole_clearance, 2 clearance)
- 0 unconnected pads (all 16 nets connected)
- KiCad version: 10.0.7
- Command: `kicad-cli pcb drc --refill-zones <board>.kicad_pcb`

## ERC Evidence
- Synth ERC: 0 errors, 17 warnings (name convention, supply identity, ESD, decoupling)
- See reports/ble_node.erc.json

## Routing Evidence
- 14/16 nets autorouted by kicad-routing-tools
- 2 nets manually routed via resin-filled via-in-pad (jlc-filled-via manufacturer)
- Total vias: 24+, total trace length: ~434mm

## Per-Part Datasheet Review
| Part | MPN | Footprint | Status |
|------|-----|-----------|--------|
| nRF52840 | NRF52840-QIAA-R | Nordic_AQFN-73-1EP_7x7mm_P0.5mm | I2C caps added to p0_06/p0_08 |
| LP5907-3.3 | LP5907MFX-3.3 | SOT-23-5 | Low-noise LDO for RF |
| USB-C receptacle | TYPE-C-31-M-12 | USB_C_Receptacle_HRO_TYPE-C-31-M-12 | 3D model imported |
| BME280 | BME280 | Bosch_LGA-8_2.5x2.5mm | Authored part (no BME280 in registry) |
| Balun | 2450BL15B100 | Balun_Johanson_0896BM15A0001 | Footprint corrected |
| Chip antenna | 2450AT43F0100E | Johanson_2450AT43F0100 | 2.4GHz, 1dBi |
| 32MHz crystal | Generic | Crystal_SMD_3225-4Pin_3.2x2.5mm | 12pF load caps |

## RF Review
- 50 Ω feed: 0.145mm trace width (microstrip, 0.1mm FR4, er=4.3)
- Exported as RF50 net class with trace_width 0.145mm
- Antenna keepout: 8mm radius
- Matching network: 3.9nH / 1.0pF (Nordic reference design values)
- **Independent RF review: NOT DONE**

## USB SI Review
- USB 2.0 Full Speed (12 Mbps)
- Separate ESD protection on D+/D- channels
- No controlled impedance on USB pair

## Fabrication Review
- via-in-pad at U1 (aQFN-73) requires resin-filled vias (jlc-filled-via manufacturer)
- JLCPCB confirmation: PENDING

## Component Review
- All parts need independent review
- BME280 substituted for BME680 (BME280 not in registry)

## Status: NOT QUALIFIED
