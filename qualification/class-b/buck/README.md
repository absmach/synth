# 24 V to 3.3 V buck with protected input (Class B, SYN-67)

A 24 V in, 3.3 V out step-down board built around the TI LMR38010 (1 A), with a PTC fuse, a reverse-blocking Schottky and a TVS on the input. It is intended for 24 V +10% in and 1 A out, on four copper layers, 70.99 x 33.99 mm.

It answers ticket #67 under #38 (Class B qualification): a switching regulator with input and output filtering, feedback, power-good and thermal copper. The negative variant `buck_neg_fb_sw.synth` is the same board with `R_FB_T` and `C7` taken from SW instead of the output node. `synth validate` must reject it with E-SYNTH-POWER-011.

The board is designed, placed and routed. It is not fabricated, not powered and not tested on hardware. No surge test was run and no IEC 61000-4-5 rating is claimed.

Authored by nyagamunene. The registry parts `lmr38010` and `ltst_c190kgkt` were reviewed by Steve Munene on 2026-10-29; no independent board review is recorded here.

No prompt was recorded for this board; see [`PROMPT.md`](PROMPT.md).

## The board

| Group | Parts |
|---|---|
| Input | `J1` JST B2B-XH-A; `F1` Bourns MF-MSMF110/33X PTC (1.1 A hold at 23 C, 0.63 A at 70 C, 2.2 A trip, 33 V max); `D1` Vishay SS36 Schottky, reverse block (60 V, 3 A); `D2` Vishay SMBJ30A TVS (30 V standoff, 48.4 V clamp at 12.4 A); `C1` 47 uF 50 V electrolytic (Honor RVT1H470M0810), `C2` 10 uF, `C3` 220 nF at the VIN pin |
| Converter | `U1` TI LMR38010SDDAR, HSOIC-8 with exposed pad (4.2 to 80 V operating, 85 V absolute maximum); `C4` 100 nF boot; `R_RT` 33k sets 800 kHz; UVLO divider `R_UV_T` 1.37M and `R_UV_B` 100k |
| Output | `L1` Bourns SRN6045TA-6R8M, 6.8 uH (Isat 5.7 A); `C5`, `C6`, `C9` 22 uF 25 V X7R 1206; `J2` JST B4B-XH-A |
| Feedback | `R_FB_T` 100k from the output node (L1 pad 2 side), `R_FB_B` 43.2k, `C7` 56 pF feed-forward across `R_FB_T` |
| Power-good and LEDs | `R_PG` 100k pull-up to +3V3, PG on `J2` pin 3; `D4` green "PG FAULT" with `R_PGLED` 470 ohm, lit while PG is low; `D3` green "PWR" with `R_LED` 470 ohm (both Lite-On LTST-C190KGKT) |
| Mechanical | `H1`, `H2` M2.5 non-plated holes in opposite corners |

- Output: Vout = 1.0 x (1 + 100k/43.2k) = 3.315 V typical, 3.26 to 3.36 V over the 0.985 to 1.015 V reference (ideal resistors).
- UVLO (datasheet SNVSBM3B, Eq 13 and 14): start 18.4 V typical, stop 16.2 V typical. EN is 1.90 V at 28 V in.
- Inductor ripple at 26.4 V in is 0.53 A peak to peak. The peak at 1 A out is about 1.27 A, against the 5.7 A Isat.
- Converter choice: the first revision used the LMR33640 (38 V absolute maximum), which sits below the 48.4 V TVS clamp. The LMR38010 (85 V) sits above it.
- Thermal: the exposed pad sits on the F.Cu GND pour with six 0.3 mm GND vias in the pad, declared as `[[vias]]` in `buck.placement.layout.toml`. The source declares `manufacturer "jlc-filled-via"` and `requires via_in_pad`.
- Placement: `buck.placement.layout.toml` pins all 27 parts with hard priority. In `kicad/buck.kicad_pcb`, all 97 track segments are on F.Cu, and the 21 vias are all GND, 0.3 mm drill.
- Registry review records: [`lmr38010`](../../../registry/parts/regulators/lmr38010.synth.toml) and [`ltst_c190kgkt`](../../../registry/parts/diodes/ltst_c190kgkt.synth.toml). The KiCad LED pads are 0.15 mm wider in span than the datasheet land.
- The board's row is in [`qualification/matrix.toml`](../../matrix.toml).

## Not verified by Synth

- **Release:** the route, DRC, ERC and release verdicts are not checked in this folder. The earlier reports are no longer in the tree, and the matrix row is skipped until they are re-established.
- **Fabrication:** the Gerbers were not opened in a fab's DFM tool. The via-in-pad process was not confirmed with a fab.
- **Thermal:** TI asks for at least a 4 x 3 via array (9.5.1.1); the board has six. The 0.58 W loss assumes 85% efficiency; the junction temperature is not estimated.
- **Surge:** a sustained over-voltage is limited only by `F1` (33 V) and the 50 V capacitors. TVS energy, PTC behaviour and inrush are not analysed.
- **Stability:** DC-bias derating of the 22 uF and 10 uF ceramics is unknown. Datasheet Table 9-1 has no 3.3 V row.
- **Power-good:** Synth has no power-good rule. Thresholds, delay and fault behaviour are not checked.
- **Layout:** loop area, return paths and EMI are not analysed. Routing used FreeRouting defaults with no length constraints.
- **Silkscreen:** the "PG FAULT" and "PWR" values are on F.Fab only. The board prints just `D3` and `D4`.
- **Parts:** availability and LCSC numbers are not checked. The generic passives have no MPN.
