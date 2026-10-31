# Irrigation controller: fill run report

Final state: `export/` (with `irrigation_controller.synth` and `irrigation_controller.placement.layout.toml` beside it). Start state: `export_start/`.

## Parts added (18, in 4 kept batches)

Positions are the sidecar centres in board mm. Every part has rotation 0.

| Ref | Value | Function | Nets | Position | Batch |
|---|---|---|---|---|---|
| R_B1 | 100k | bleeder across C_BULK | +12V_VALVE / GND | 42.3, 5.0 | 1 |
| C_B1 | 100nF | HF bypass, 12 V rail beside C_BULK | +12V_VALVE / GND | 42.3, 7.6 | 1 |
| R_SN1 | 100 | snubber resistor, valve 1 coil | +12V_VALVE / SNUB1 | 42.3, 10.2 | 1 |
| C_SN1 | 10nF | snubber capacitor, valve 1 coil | SNUB1 / VALVE1_SW | 42.3, 12.8 | 1 |
| TP_V1 | test pad | probe VALVE1_SW | VALVE1_SW | 42.3, 15.0 | 1 |
| R_SN2 | 100 | snubber resistor, valve 2 coil | +12V_VALVE / SNUB2 | 62.0, 25.2 | 2 |
| C_SN2 | 10nF | snubber capacitor, valve 2 coil | SNUB2 / VALVE2_SW | 65.2, 25.2 | 2 |
| TP_V2 | test pad | probe VALVE2_SW | VALVE2_SW | 67.6, 25.5 | 2 |
| C_B3 | 100nF | HF bypass, 12 V rail at valve 2 driver | +12V_VALVE / GND | 62.0, 28.8 | 2 |
| R_B3 | 47k | 12 V rail bleeder | +12V_VALVE / GND | 65.2, 28.8 | 2 |
| C_B2 | 100nF | +3V3 decoupling | +3V3 / GND | 54.5, 32.0 | 3 |
| C_B4 | 100nF | +3V3 decoupling | +3V3 / GND | 54.5, 34.5 | 3 |
| R_B2 | 100k | +3V3 bleeder | +3V3 / GND | 54.5, 37.0 | 3 |
| TP_SDA | test pad | probe I2C_SDA | I2C_SDA | 54.5, 39.5 | 3 |
| C_B5 | 4.7uF 0805 | +3V3 bulk | +3V3 / GND | 60.5, 32.2 | 3 |
| R_B4 | 47k | +3V3 bleeder | +3V3 / GND | 60.5, 35.2 | 3 |
| TP_SCL | test pad | probe I2C_SCL | I2C_SCL | 60.5, 38.2 | 3 |
| C_B8 | 4.7uF 0805 | +3V3 bulk | +3V3 / GND | 20.0, 42.5 | 4b |

The 0603 parts use `r_generic_0603` / `c_generic_0603`; the 0805 parts use `c_generic_0805`. All values are from the permitted list. No part was authored.

## Dropped

- **C_B6, C_B7** (100nF, +3V3, void 3 at 18.0, 39.5 and 22.5, 39.5), batch 4. Batch 4 (C_B6, C_B7, C_B8) did not release: `validation_failed`, 20 vias in pads, 9 signal vias with no ground return. The router picked `--ordering bus` for that run. Trimming the batch to C_B8 alone released. I did not test C_B6 or C_B7 individually, so I do not know whether one of them would release on its own.
- Not attempted:
  - Rail LEDs: the board already has two rail LEDs on 680 ohm and 2.2k resistors.
  - A Schottky across each valve output: D_FLY1 and D_FLY2 already exist.
  - Pull-ups and pull-downs on enable, reset and boot pins: R_EN, R_NRST and R_PD1/2 already exist.
  - Extra 100nF at IC supply pins: U1, U2, U4 and U5 already have decoupling. I did not add more beside them because the routing batch that tried (batch 4) failed.

## Measured from the exports

- **Release:** `export/` reports `release_ready: true`, `route_complete: true`, `fabrication_ready: true`, 0 open nets, 0 vias in pads, no blocking reasons.
- **KiCad DRC (as reported by the export):** 0 errors, 0 unconnected, 128 warnings.
- **Part count:** 126 (108 at start, plus 18).
- **Batches:** batches 1, 2, 3 and 4b all released on their first fresh-directory export. `export/` has the same router input hash as `export_batch_4b/`.
- **Router:** KiCadRoutingTools, `inside_out` ordering. The start state also routed with `inside_out`.

## Not done or not verified

- **Courtyard fill is not measured.** The only tools here were the Synth MCP and Read/Write/Edit, so I could not run `layout_metrics.py` or `kicad-cli`. My estimate is that the 18 parts add roughly 70 mm² of courtyard. That is about 1.4 % of the 4926 mm² outline, so fill would be about 31 %. This is a calculation, not a measurement, and it sits close to the 31 % target. Please run `layout_metrics.py` on `export/` before relying on it. If it falls short, the free space left is in voids 2, 4, 7 and 9.
- **Outline and connector gaps are not independently measured.** The start export and every kept export report an outline present and a passing release gate, and the sidecar positions of existing parts and connectors are unchanged. I did not read the Edge.Cuts coordinates or run `edge.py`.
- **`synth_place_with_hints` review was unusable.** It reported a 151 x 113 mm outline and flagged connectors as far from the edge, which does not match the 75 x 65 mm export. It also moved some existing parts, such as C_BULK, in its own preview. I treated the export as the evidence.
- **KiCad version:** these exports ran with KiCad 10.0.5 (reported by the tool), not the 10.0.7 named in the server notes.
- **Not run:** KiCad DRC with real library tables, three-export hash comparison, fabrication package, JLCPCB DFM.
- **Schematic:** the new groups ("12V BULK AND SNUBBER", "VALVE 2 SNUBBER", "3V3 EXTRA DECOUPLING") add some readability warnings (net span over 100 mm on the 12 V and GND nets, low sheet fill). I did not run `synth_review_schematic` or fix them.
- **Snubber placement:** the valve 1 snubber sits about 7 mm from J_VALVE1 and the valve 2 snubber about 17 mm from J_VALVE2, because the free voids are not next to the connectors.
