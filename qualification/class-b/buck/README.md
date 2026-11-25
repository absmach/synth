# 24 V to 3.3 V buck with protected input (Class B, SYN-67)

A 24 V in, 3.3 V out step-down board built around the TI LMR38010 (1 A), with a PTC fuse, a reverse-blocking Schottky and a TVS on the input. It is intended for 24 V +10% in and 1 A out, on four copper layers, 71.0 x 34.0 mm.

It answers ticket #67 under #38 (Class B qualification): a switching regulator with input and output filtering, feedback, power-good and thermal copper. The board is designed, placed and routed. It is not fabricated, not powered and not tested on hardware, and no surge rating is claimed.

No prompt was recorded for this board; see [`PROMPT.md`](PROMPT.md).

## The board

### Parts

| Group | Parts |
|---|---|
| Input | `J1` JST B2B-XH-A; `F1` Bourns MF-MSMF110/33X PTC (1.1 A hold at 23 C, 0.63 A at 70 C, 2.2 A trip, 33 V max); `D1` Vishay SS36 Schottky, reverse block (60 V, 3 A); `D2` Vishay SMBJ30A TVS (30 V standoff, 48.4 V clamp at 12.4 A); `C1` 47 uF 50 V electrolytic (Honor RVT1H470M0810), `C2` 10 uF, `C3` 220 nF at the VIN pin |
| Converter | `U1` TI LMR38010SDDAR, HSOIC-8 with exposed pad (4.2 to 80 V operating, 85 V absolute maximum); `C4` 100 nF boot; `R_RT` 33k sets 800 kHz; UVLO divider `R_UV_T` 1.37M and `R_UV_B` 100k |
| Output | `L1` Bourns SRN6045TA-6R8M, 6.8 uH (Isat 5.7 A); `C5`, `C6`, `C9` 22 uF 25 V X7R 1206; `J2` JST B4B-XH-A |
| Feedback | `R_FB_T` 100k from the output node (L1 pad 2 side), `R_FB_B` 43.2k, `C7` 56 pF feed-forward across `R_FB_T` |
| Power-good and LEDs | `R_PG` 100k pull-up to +3V3, PG on `J2` pin 3; `D4` green "PG FAULT" with `R_PGLED` 470 ohm, lit while PG is low; `D3` green "PWR" with `R_LED` 470 ohm (both Lite-On LTST-C190KGKT) |
| Mechanical | `H1`, `H2` M2.5 non-plated holes in opposite corners |
| Negative variant | `buck_neg_fb_sw.synth` is the same board with `R_FB_T` and `C7` taken from SW instead of the output node. `synth validate` must reject it with E-SYNTH-POWER-011. |

### Layout

| Item | Value |
|---|---|
| Outline | 71.0 x 34.0 mm |
| Copper layers | 4 |
| Stackup | Default Synth four-layer stackup, 1.6 mm board |
| Parts in the BOM | 27 |
| Parts placed in the CPL | 27 |
| Nets | 13 named nets in `kicad/buck.kicad_pcb` |
| Track segments | 97, all on F.Cu |
| Vias | 21, all GND, 0.3 mm drill |
| Track length | 200.1 mm |
| Zones and planes | One unsplit GND zone on each of the four copper layers; no analog or power ground split. Keepout zones at the `H1` and `H2` screw heads. No keepout under the switch node or the inductor. |
| Placement | `buck.placement.layout.toml` pins all 27 parts with hard priority. |
| Fab package | `fab/gerbers.zip`, `fab/bom.csv`, `fab/cpl.csv`, `models/buck.step` and `SHA256SUMS` |
| Export pin | The committed KiCad files were exported with Synth commit `a79570cd`. An export from another Synth revision does not match `SHA256SUMS`. |

### Checks

| Check | Result | Notes |
|---|---|---|
| KiCad DRC (10.0.8, --refill-zones) | 0 errors, 0 warnings, 0 unconnected | Measured on the committed `kicad/buck.kicad_pcb`. |
| KiCad ERC (10.0.8) | 0 errors, 8 warnings | All 8 warnings are `lib_symbol_issues` (library lookups; the stock libraries were not configured in that run). |
| Schematic parity (KiCad 10.0.8) | Clean | Run on the committed files on 2026-11-25. |
| Return vias (`W-SYNTH-DRC-016`) | 0 | On the committed copper, with a 0.6377 mm window from the default four-layer stackup. Every layer change is a GND via and no signal net leaves F.Cu. This is a proximity test, not a return-path analysis. |
| `synth registry qualify` | All nine registry parts passed on 2026-10-29 | `lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`, `mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`, `ltst_c190kgkt`. |
| Registry QUAL warning | `mf_msmf110_33x` reports `E-SYNTH-QUAL-016` | Its 3D model is not installed. |
| Schematic quality (`tools/schematic_quality.py`) | 7 texts with a wire through them, 1 wire crossing | The sheet is readable, not tidy. |
| Qualification matrix | Row `buck` in [`qualification/matrix.toml`](../../matrix.toml) | The row is skipped until the route and release verdicts are re-established. |

### Design figures

| Area | Item | Value | Basis |
|---|---|---|---|
| Power | Output voltage | Vout = 1.0 x (1 + 100k/43.2k) = 3.315 V typical; 3.26 to 3.36 V over the 0.985 to 1.015 V reference (ideal resistors) | Calculated; TI SNVSBM3B |
| Power | UVLO thresholds | Start 18.4 V typical, stop 16.2 V typical | TI SNVSBM3B, Eq 13 and 14 |
| Power | UVLO spread | Start 16.2 to 20.6 V, stop 14.0 to 17.9 V | Calculated; TI SNVSBM3B, Eq 13 and 14 |
| Power | EN voltage | 1.90 V at 28 V in | Calculated |
| Power | Inductor ripple | 0.53 A peak to peak at 26.4 V in | Calculated |
| Power | Inductor peak | About 1.27 A at 1 A out, against the 5.7 A Isat | Calculated; Bourns SRN6045TA |
| Power | Minimum inductance | 1.03 uH (Eq 11); 6.8 uH fitted | TI SNVSBM3B, Eq 11 |
| Power | Input capacitance | Meets the 4.7 uF plus 100 to 220 nF minimum | TI SNVSBM3B |
| Power | Switching frequency | `R_RT` 33k sets 800 kHz | TI SNVSBM3B, RT table under Figure 8-4 |
| Power | Converter choice | The first revision used the LMR33640 (38 V absolute maximum), below the 48.4 V TVS clamp. The LMR38010 (85 V) sits above it. | TI SNVSBM3B; Vishay SMBJ series |
| Protection | TVS breakdown and clamp | SMBJ30A breaks down at 33.3 to 36.8 V and clamps at 48.4 V; 600 W at 10/1000 us | Vishay SMBJ series datasheet |
| Protection | Surge margin to converter | 48.4 V clamp is below the LMR38010 85 V VIN and SW maximum | TI SNVSBM3B; Vishay SMBJ series |
| Protection | Surge margin to capacitors | `C1`, `C2` and `C3` are 50 V parts, 1.6 V above the clamp | Calculated |
| Protection | PTC load | `F1` carries 0.162 A at 24 V and 0.240 A at the UVLO stop voltage, against 0.63 A hold at 70 C | Calculated; Bourns MF-MSMF110/33X |
| Power-good | Thresholds | Rising 110/112/114 %, falling 90/92/94 % | TI SNVSBM3B |
| Power-good | Timing | 40 us glitch filter, 4 ms start-up delay | TI SNVSBM3B |
| Power-good | PG pin rating | 20 V | TI SNVSBM3B |
| LEDs | LED current | 1.9 to 3.0 mA through each 470 ohm resistor, against 30 mA maximum | Calculated; Lite-On DS22-2000-074 Revision O |
| LEDs | `D4` current | About 2.55 mA while PG is low | Calculated |
| Thermal | Exposed pad | 2.41 x 3.1 mm against the 2.40 x 3.10 mm DDA land pattern | Measured on the board file; TI SNVSBM3B DDA land pattern |
| Thermal | Thermal vias | Exposed pad on the F.Cu GND pour with six 0.3 mm GND vias in the pad, declared as `[[vias]]` in `buck.placement.layout.toml` | Measured on the board file |
| Thermal | Via-in-pad process | The source declares `manufacturer "jlc-filled-via"` and `requires via_in_pad` | `buck.synth` |
| Thermal | RthetaJA | The datasheet value (42.9 C/W) is marked not valid for design and is not used | TI SNVSBM3B |
| Assembly | `C3` / `C2` to U1 VIN | 0.82 / 1.46 mm pad-edge gap | Measured on the board file |
| Assembly | `C3` / `C2` ground pad to U1 GND | 2.69 / 5.35 mm | Measured on the board file |
| Assembly | `C4` to U1 BOOT / SW | 0.62 / 2.18 mm | Measured on the board file |
| Assembly | `L1` pad 1 to U1 SW | 3.80 mm | Measured on the board file |
| Assembly | `C5`, `C6`, `C9` to `L1` pad 2 | 0.83 / 2.29 / 0.70 mm | Measured on the board file |
| Assembly | `D2` to U1 VIN | 7.82 mm | Measured on the board file |
| Assembly | `R_FB_T` / `R_FB_B` to U1 FB | 3.55 / 3.79 mm | Measured on the board file |
| Assembly | `R_FB_B` ground pad to U1 GND | 11.31 mm | Measured on the board file |
| Assembly | `R_FB_T` FB pad to `L1` pad 1 (SW) | 2.58 mm | Measured on the board file |
| Assembly | `R_PG` to U1 PG | 22.63 mm | Measured on the board file |
| Footprints | LED land | The KiCad LED pads are 0.15 mm wider in span than the datasheet land | Lite-On DS22-2000-074 Revision O |

### Sourcing

| Item | Status | Notes |
|---|---|---|
| Registry review records | All nine registry parts carry a review record | See [`lmr38010`](../../../registry/parts/regulators/lmr38010.synth.toml) and [`ltst_c190kgkt`](../../../registry/parts/diodes/ltst_c190kgkt.synth.toml). |
| Datasheets | Cited | TI SNVSBM3B (LMR38010), Lite-On DS22-2000-074 Revision O (LTST-C190KGKT), Vishay SMBJ series. |
| Availability and LCSC numbers | Not checked | The generic passives have no MPN. |

## Not verified by Synth

| Area | What is not verified |
|---|---|
| Release | The route and release verdicts are not checked in this folder. The earlier reports are no longer in the tree. |
| Router mismatch | `synth check --fab` routes with KiCadRoutingTools, not FreeRouting. At KiCadRoutingTools `61bde44` that route left 11 signal vias without a ground return via within 0.64 mm, so the manufacturing stage failed. |
| Board review | No independent board review is recorded. |
| Fabrication | The Gerbers were not opened in a fab's DFM tool. The via-in-pad process was not confirmed with a fab. |
| Thermal | TI asks for at least a 4 x 3 via array (SNVSBM3B 9.5.1.1); the board has six. The 0.58 W loss assumes 85% efficiency. The junction temperature is not estimated. |
| Surge | No surge test was run and no IEC 61000-4-5 rating is claimed. A sustained over-voltage is limited only by `F1` (33 V) and the 50 V capacitors. TVS energy, PTC behaviour and inrush are not analysed. |
| Stability | DC-bias derating of the 22 uF and 10 uF ceramics is unknown. Datasheet Table 9-1 has no 3.3 V row. `C1` has no stated ESR; its ripple rating is 120 mA at 120 Hz. Crossover, phase margin and load-transient response are not computed. |
| Power-good | Synth has no power-good rule. Thresholds, delay and fault behaviour are not checked. |
| Layout | Loop area, return paths and EMI are not analysed. Routing used FreeRouting defaults with no length constraints. The input capacitors have a longer ground leg than VIN leg. |
| Feedback placement | `R_FB_B` returns to U1 GND through the plane, 11.31 mm away, and the `R_FB_T` FB pad is 2.58 mm from the SW pad. Neither is judged. |
| Footprints | The SS36 and SMBJ30A pads sit wider apart than Vishay's land. The SRN6045TA pads are 0.55 mm longer inward. The MF-MSMF110/33X pads are 1.125 mm long against Bourns' 1.68 mm. |
| Polarity and pin 1 | `C1` pin 1 as + follows the KiCad convention. The JST pin 1 is not stated in the catalogue. |
| Silkscreen | The "PG FAULT" and "PWR" values are on F.Fab only. The board prints just `D3` and `D4`. |
| Hardware | The board is not fabricated, not powered and not tested on hardware. |
