# Netlist: buck rev A

Taken from the `net` blocks and `connect` statements in `../buck.synth`. `QUALIFICATION.md` A8 records that `kicad-cli sch export netlist` gave 13 nets, node for node the same (7 `net` blocks plus 6 more names from 10 `connect` statements), 27 components. The 13 net names are also the net names in `../kicad/buck.kicad_pcb`. This file was not regenerated.

| Net | Class | Nodes | Count |
|---|---|---|---|
| VIN_RAW | PWR | J1.p1, F1.p1 | 2 |
| VIN_FUSED | PWR | F1.p2, D1.anode | 2 |
| VIN | PWR | D1.cathode, D2.cathode, C1.p1, C2.p1, C3.p1, U1.vin (3), R_UV_T.p1 | 7 |
| GND | PWR | J1.p2, D2.anode, C1.p2, C2.p2, C3.p2, U1.gnd (1), U1.pad (9), R_UV_B.p2, C5.p2, C6.p2, C9.p2, R_FB_B.p2, R_RT.p2, J2.p2, J2.p4, D3.cathode | 16 |
| SW | PWR | U1.sw (8), L1.p1, C4.p2 | 3 |
| +3V3 | PWR | L1.p2, C5.p1, C6.p1, C9.p1, R_FB_T.p1, J2.p1, R_LED.p1, R_PG.p2, R_PGLED.p1, C7.p2 | 10 |
| PG | default | U1.pg (6), R_PG.p1, J2.p3, D4.cathode | 4 |
| EN | default | U1.en (2), R_UV_T.p2, R_UV_B.p1 | 3 |
| FB | default | U1.fb (5), R_FB_T.p2, R_FB_B.p1, C7.p1 | 4 |
| RT | default | U1.rt_sync (4), R_RT.p1 | 2 |
| BOOT | default | U1.boot (7), C4.p1 | 2 |
| LED_PWR | default | R_LED.p2, D3.anode | 2 |
| LED_PG | default | R_PGLED.p2, D4.anode | 2 |

Net classes: `PWR` (trace 0.5 mm, clearance 0.2 mm) on the first six nets; the rest have no class in the source. Power rails declared: `VIN` 24 V, `+3V3` 3.3 V.

Routed result (`../reports/buck.connectivity.json`, `../reports/buck.routing.json`): 13 nets connected, 0 open nets, 0 unconnected pads, 97 segments (all F.Cu), 21 vias (all GND), 200.08 mm of copper.

Negative twin `../buck_neg_fb_sw.synth`: identical except R_FB_T and C7 hang from SW; `synth validate` must exit 1 with E-SYNTH-POWER-011 (recorded in README/QUALIFICATION A2; not rerun here).
