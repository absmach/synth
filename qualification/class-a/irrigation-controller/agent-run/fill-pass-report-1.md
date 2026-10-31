# REPORT6: irrigation controller, rail-only fill

Final state: `export/` (identical routing input hash to `export_batch_4/`), with `irrigation_controller.synth` and `irrigation_controller.placement.layout.toml` beside it.

## Result
- release_ready true, route_complete true, 43 of 43 nets, KiCad DRC 0 errors, 0 unconnected (163 warnings, up from 129 at start).
- via_in_pad empty, undersized tracks/vias empty, fabrication_ready true.
- Outline unchanged (75.1 x 65.4 mm); no connector or mounting hole moved.
- 26 parts added (72 -> 98), well short of the 40 asked for. I stopped after 4 batches, all of which released; no batch failed, so nothing was dropped or bisected.

## Step A (regrouping): only partly done
One group of six sidecar moves, taken from the placement review's advice, released (`export_step_1`):
C_U5 x=58.0, R_NRST x=39.5, R_FBT x=36.5, R_FBB x=36.5, R_G1 x=46.5, TP_SOIL2 -> (32.0, 46.5) (match_repeated_circuit).

Not done: the `place_beside_neighbour` advice (TP_NRST, SW_RESET, R_NRST, C_NRST, TP_FLOW), the support-part `near:` hints (C_U5 near U3.RESET, C_NRST near Y1.gnd_2), and the test-point hints (TP_NRST, TP_VIN, TP_FLOW, TP_12V). These move parts by 5-20 mm, and I did not try them.
The review's connector-edge warnings (connector "inside the edge") come from a 111 x 94.7 mm board the placer model uses for the review, not from the exported 75.1 x 65.4 mm board; I did not act on them. I did not measure connector gaps on the export.
I did not remeasure the free space after step A; I used the brief's void list.
I did not make the per-group rule report with measured distances that the brief asks for. No rule-by-rule check was made on the exported board.

## Added parts
All parts are generic (no MPN). Positions are board mm.

| Ref | Value | Function | Nets | Position | Batch |
|---|---|---|---|---|---|
| C_N1 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 66.0, 36.0 | 1 |
| C_N2 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 70.0, 36.0 | 1 |
| C_N3 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 66.0, 40.0 | 1 |
| C_N4 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 70.0, 40.0 | 1 |
| C_N5 | 10uF 0805 | +3V3 bulk | +3V3, GND | 66.0, 44.0 | 1 |
| C_N6 | 4.7uF 0805 | +3V3 bulk | +3V3, GND | 70.0, 44.0 | 1 |
| R_N1 | 100k 0603 | bleeder across +3V3 | +3V3, GND | 66.0, 48.0 | 1 |
| R_N2 | 47k 0603 | bleeder across +3V3 | +3V3, GND | 70.0, 48.0 | 1 |
| R_N3 | 2.2k 0603 | series resistor, +12V_VALVE LED | +12V_VALVE, LED_N12_A | 49.5, 63.5 | 2 |
| D_N1 | amber 0603 | +12V_VALVE present LED | LED_N12_A, GND | 53.5, 63.5 | 2 |
| R_N4 | 680 0603 | series resistor, +3V3 LED | +3V3, LED_N33_A | 57.5, 63.5 | 2 |
| D_N2 | green 0603 | +3V3 present LED | LED_N33_A, GND | 61.5, 63.5 | 2 |
| C_N7 | 10uF 25V 0805 | +12V_VALVE decoupling | +12V_VALVE, GND | 65.5, 63.5 | 2 |
| R_N5 | 100k 0603 | bleeder across +12V_VALVE | +12V_VALVE, GND | 69.0, 66.0 | 2 |
| C_N8 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 11.0, 47.0 | 3 |
| C_N9 | 100nF 0603 | +12V_VALVE decoupling | +12V_VALVE, GND | 15.0, 47.0 | 3 |
| C_N10 | 4.7uF 0805 | +3V3 bulk | +3V3, GND | 19.0, 47.0 | 3 |
| R_N6 | 100k 0603 | bleeder across +12V_VALVE | +12V_VALVE, GND | 11.0, 51.0 | 3 |
| D_N3 | d_tvs_5v | TVS on flow pulse (clamp to GND) | FLOW_PULSE, GND | 15.5, 51.0 | 3 |
| D_N4 | d_tvs_5v | TVS on +3V3 rail | +3V3, GND | 20.0, 51.0 | 3 |
| C_N11 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 73.5, 36.0 | 4 |
| C_N12 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 73.5, 40.0 | 4 |
| C_N13 | 100nF 0603 | +3V3 decoupling | +3V3, GND | 73.5, 44.0 | 4 |
| C_N14 | 4.7uF 0805 | +3V3 bulk | +3V3, GND | 73.5, 48.0 | 4 |
| TP_N1 | test pad | +3V3 test point | +3V3 | 66.0, 52.5 | 4 |
| TP_N2 | test pad | GND test point | GND | 70.0, 52.5 | 4 |

## Caveats on these parts
- Most of the +3V3 capacitors (batch 1, 4) sit in the right-hand void, 15-30 mm from the MCU supply pin and the other 3V3 loads. They add rail capacitance but are not local decoupling for any IC pin; the router connects them with long tracks. The brief's "next to a rail track or IC supply pin" is not met for them.
- D_N3 (flow-pulse TVS, batch 3) is about 45 mm from J_FLOW's pulse pin region at the bottom edge, not next to the connector as intended; C_N8..C_N10, D_N4 likewise sit away from the nets' other loads.
- The +12V_VALVE LED/capacitor/bleeder (batch 2) lie in void 3 away from the valve connectors.
- The 3V3 5 V TVS (D_N4) is a 5 V part; its leakage on a 3.3 V rail was not checked against a datasheet. Bleeder values (47k/100k) were not checked for bulk-cap discharge time.
- The 12 V LED resistor (2.2k) gives about 4 mA at 12 V; the 680 R gives about 1.8 mA at 3.3 V (dim). Neither was checked against LED datasheets.
- Parts are generic registry entries, not reviewed ones.

## Not tried from the brief's list
Valve-output snubbers, per-valve TVS/flyback additions, ferrite bead on the sensor supply, varistor, second input TVS, connector-pin TVS/ESD diodes, and test points on connector pins. The existing board already has flyback diodes on both valve outputs.

## Not checked
No independent `kicad-cli` DRC of the exported files (the only DRC figures are those the Synth export reports); no connector-to-edge measurement; no placement review run after the additions; no fresh-directory repeat beyond `export/` matching `export_batch_4/` by input hash.

## Dropped parts
None.
