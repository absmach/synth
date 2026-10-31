# REPORT6: irrigation controller, outline repair and partial regrouping

Final state: `export/` (same routing input hash as `export_step_3/`), with `irrigation_controller.synth` and `irrigation_controller.placement.layout.toml` beside it.

## Result (from the Synth export report, not independently measured)
- release_ready true, route_complete true, 43 of 43 nets, KiCad DRC 0 errors, 0 unconnected (166 warnings, up from 163 at start).
- via_in_pad empty, undersized tracks/vias empty, fabrication_ready true.
- No part was added. The .synth is unchanged; 163 -> 166 warnings come from the moves below.

## Moves kept (sidecar only)
| Step | Part | From | To | Rule |
|---|---|---|---|---|
| 1 | R_N5 | 69.0, 66.0 | 69.0, 65.5 | `move_off_edge_rim`, the advice's own coordinates |
| 3 | C_U5 | 58.0, 45.5 | 58.0, 34.0 | support part to U3's reset pin area (between C_U3 and C_U4) |
| 3 | C_NRST | 21.5, 38.5 | 27.0, 41.0 | `place_beside_neighbour` toward U2 |

## Tried and dropped
- Step 2 added SW_RESET (49,38 -> 45,40.5) and TP_NRST (60.5,50 -> 41.5,42.5) to the two moves above. It failed: net NRST left SW_RESET.1 connected to 3 of 7 pads, and the router put a via inside C_N2.2. Both parts were put back at their old coordinates; step 3 with C_U5 and C_NRST alone released.

## Not done, and not checked
- I did not measure the exported outline or any connector gap. I have no shell and did not read the .kicad_pcb. The R_N5 move is the placement review's own fix for the 0.5 mm outline excess and 0.68 mm connector gaps, so it should clear them, but that is unconfirmed. Check the outline (cap 75.1 x 65.4 mm) and the connector gaps (limit 0.3 mm) on `export/`.
- I did not re-run the placement review after the moves, so I cannot say which warnings are cleared. C_U5 and C_NRST were placed by estimate, not on the pad coordinates; the `near: part.pin` hints were not used.
- Not done: reset group (SW_RESET, R_NRST, TP_NRST), test points TP_VIN, TP_FLOW, TP_12V, D_N3 beside R_FS, `match_repeated_circuit` for D_N2 and R_G2, `align_like_parts`, and the power, processor, valve-driver and sensor regrouping.
- Not done: the far +3V3 capacitors (C_N1 to C_N6, C_N11 to C_N14) remain 15 to 30 mm from the loads, and the flow-pulse TVS D_N3 is still far from J_FLOW.
- Not done: no parts added, no remeasure of free space, no per-group rule report with measured distances, no independent `kicad-cli` DRC.
- No `allow_incomplete`, and `synth_compact_layout` was not called.

## Dropped parts
None.
