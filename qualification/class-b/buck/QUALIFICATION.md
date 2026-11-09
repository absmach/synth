# Qualification matrix: 24 V to 3.3 V buck board (SYN-67)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the
evidence. Section B lists what no tool here proves and an SI/PI/RF or power expert has to sign
off. No expert has reviewed this board. Every part on the board that carries a registry review record is signed (Steve Munene); the 0603 passives and the mounting hole have none.

**Scope: artifact generation only.** What is claimed as passed is what Section A lists: the design compiles, routes, passes
Synth's source/DRC gate and KiCad DRC and ERC with 0 errors, and its files are hashed. Not claimed, and unverified: input
and switch-node loop area and coupling, EMI, input and output ripple, loop stability with real capacitor derating and ESR,
junction temperature at 1 A, fabrication acceptance of the filled vias in the pad, and independent mixed-signal/RF
sign-off (#38). Also open: the `synth check --fab` return-path analysis did not pass (A18 gives the reason). The board
has not been fabricated, powered or tested.

This is one board in Class B (#38) and it carries **case 1 only**, the switching regulator:
24 V to 3.3 V with input and output filtering, feedback, power-good and thermal copper. The analog
front end, the USB/Ethernet peripheral and the other negative cases belong to other boards.
Section C maps every `#38` checklist item to a row here, including the ones that do not apply.

## Reproduce

Run from the repository root with the Synth code of this branch. Base: `git merge-base HEAD upstream/main`
is `2390cc9` (upstream `main` when this branch was rebased on 2026-11-06). The Synth changes this
board needs are the commits `e0c8ecd` (sidecar vias, return-via window matched to the DRC rule, rails flagged
on generated symbols, `SYNTH_SOURCE_REVISION`), `bb78235` (the LMR38010 part, SS36 drawn as `Device:D_Schottky`)
`32dcd43` (the board itself) and `a9c7546` (the corrected notes in the source) on top of that base. Use an empty `XDG_DATA_HOME` so a user registry cannot
shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
scripts/setup-routing-engines.sh --freerouting-only
B=qualification/class-b/buck
synth validate $B/buck.synth
synth validate $B/buck_neg_fb_sw.synth                     # must exit 1 with E-SYNTH-POWER-011
SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad $B/buck.synth --out out/buck \
  --allow-unverified-parts --validate-erc --router free-routing --pnp --gerbers --drill   # reports/release-fab.json
SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad $B/buck.synth --out out/buck-nofab \
  --allow-unverified-parts --validate-erc --router free-routing                           # reports/release.json
kicad-cli pcb drc --severity-all --refill-zones --format json \
  -o $B/reports/buck.kicad-drc.json $B/kicad/buck.kicad_pcb
kicad-cli sch erc --severity-all --format json \
  -o $B/reports/buck.kicad-erc.json $B/kicad/buck.kicad_sch
kicad-cli sch export netlist --format kicadsexpr -o /tmp/buck.net $B/kicad/buck.kicad_sch
(cd $B && grep -v '^#' SHA256SUMS | sha256sum -c -)
```

There is no `--place-route-iterate`: the sidecar `buck.placement.layout.toml` decides placement.
Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.4.1 (from `reports/buck.routing.json`),
Synth 0.1.2, on 2026-11-09. `SYNTH_SOURCE_REVISION` makes `buck.routing.json` record `source_revision`: it holds
`b4cbe5abcdf0f32f0cc0d8f8de86b2e84c1bb902` (`b4cbe5ab`), the head the reports were regenerated on; the evidence commit is its child, so the recorded revision is the parent of the commit that carries these files. `release.json` and `release-fab.json` do not record a revision; they were generated in the same two runs, from the same revision. Re-exporting from the committed sidecar gives a byte-identical `kicad/buck.kicad_pcb` (95 segments, 25 vias, six of them thermal vias in the U1 pad). The JSON reports were generated with a relative board path and the
repository root prefix was stripped with `sed`; `out/` is a scratch directory and is not committed. A different
router version can give different copper. `reports/registry-qualify.txt` was regenerated for the 11 registry parts on the board
with `synth registry qualify --part <id>` (`lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`,
`mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`,
`ltst_c190kgkt`, `c_samsung_cl31b106kbhnnne`, `c_samsung_cl31a226kahnnne`): all eleven qualify (`lmr38010` and `ltst_c190kgkt` were reviewed on 2026-10-29, the two capacitors on 2026-11-06).
`python3 -I footprint_compare.py kicad/buck.kicad_pcb > reports/footprint-compare.txt` (run from this folder; needs KiCad's `pcbnew` module) compares every footprint with its library copy.
The pictures in `images/` (and their hashes) are from the board before C10 and C11 were added; they were not regenerated.

## Artifact hashes

`SHA256SUMS` lists every design artifact (source, sidecar, KiCad project, Gerbers, drill files,
BOM, pick-and-place, pictures, routing, DRC and ERC reports, footprint comparison, `pad_gaps.py`, `footprint_compare.py`). It leaves out `README.md`,
`QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `buck.synth` | `c8d516f8c67d7f8cb2b3fbcf8b82568090c049fded81622285673113df88a0b2` |
| `buck_neg_fb_sw.synth` | `e7c4bd4b409c0c67d996ec4e9310fa5f70f122a5d8ecf10d273f470d1fdf2863` |
| `buck.placement.layout.toml` | `f5266a861777b904b185442ffc7d8afd5fe56591c597d65c86197cab4da77843` |
| `kicad/buck.kicad_pcb` | `fa7d6414c10f79c2fe3b21756b218511b7f59a87ddae39aac66d1f441d07b447` |
| `kicad/buck.kicad_sch` | `902d5e03cc6d9abd627aa06d5e50dbc3cbff8a8bac768cda9babd1e5507c57a9` |
| `kicad/bom.csv` | `41b77b5c34930618e888d7f02bb1633abd911ed2011d965bd58cbb71a9c2eda2` |
| `reports/release.json` | `47431c5485ff1f790d45f19a1c9c9fa97744f890f62c8217c4574000a8629b93` |
| `reports/release-fab.json` | `30a4afa1d250f625efb9476ddc581a41858fc4323e3d54b7009e8f0369c5a42f` |
| `reports/buck.routing.json` | `cbd32cdf0ed3f9d75eeb142e3f2ec06cbe02e14e9cf8494ac50b5b8e0c43d782` |
| `reports/buck.kicad-drc.json` | `869eb58e1f8064c5fef6f47c9d07608afc4d9dc8a69bd299042fcf2ce0be604e` |
| `reports/buck.kicad-erc.json` | `ca017d14fbab532984dafb6175931c79ed0d645a9f15df5df8bcf10ab73fc956` |

Datasheet values used for the part values are in `README.md` ("Parts and datasheets"). The
datasheets are not committed, so the figures taken from them cannot be checked from this
repository.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate buck.synth` | Exit 0, 0 errors, 13 warnings (11 before C10 and C11 were added): 5 `E-SYNTH-SCHEM-003`, 1 `E-SYNTH-SCHEM-004`, 4 `E-SYNTH-SCHEM-005`, 1 `W-SYNTH-ANOMALY-001`, 2 `W-SYNTH-SUPPLY-002` |
| A2 | Regulator feedback is not taken from the switch node | `E-SYNTH-POWER-011` on `buck_neg_fb_sw.synth` | The twin exits 1 with the error and the same 13 warnings (C10 and C11 are in the twin too, as generic 1206 parts like its C2, so it differs from the good board only by the feedback fault and its generic capacitors); the good board does not raise it. The rule follows one series resistor only, so a longer feedback path is not checked |
| A3 | Every net is connected end to end | `reports/buck.connectivity.json`, `reports/buck.routing.json` | 13 nets, 0 open, 0 unconnected pads, 95 segments (92 F.Cu, 3 B.Cu on `PG`), 25 vias in the routing statistics and in the PCB file (the engine itself claimed 24 imported vias), 192.60 mm of copper (179.77 mm before C10 and C11). The only via-in-pad are the six declared thermal vias (`via_in_pad` list in the connectivity report) |
| A4 | No KiCad DRC rule violation on the delivered board | `kicad-cli pcb drc --refill-zones`, `reports/buck.kicad-drc.json` | 0 errors, 0 unconnected, 25 `lib_footprint_mismatch` warnings. `footprint_compare.py` (`reports/footprint-compare.txt`) diffed all 29 footprints against the KiCad library: pads (number, type, shape, position, size, drill, offset, layers, roundrect ratio, mask and paste margins) and the courtyard, Fab, Mask and Paste graphics are identical for all 29; only silkscreen differs, on 27 footprints (the 2 mounting holes are identical). Silkscreen text, 3D models and footprint properties are not compared. The 27 differ in silkscreen stroke width (0.18 mm against the library's 0.12 mm) and, on the 1206 capacitors, in line length. KiCad lists 25 of the 27 (not `J1` and `J2`); why it skips those two was not investigated |
| A5 | The route passes Synth's source/DRC gate | `reports/release.json` (`synth export-kicad`, no fab flags) | `release_ready` true, `production_status` production, state `routed`, 0 signal-integrity findings (blocking 0, warnings 0) |
| A6 | The package passes Synth's release gate | `reports/release-fab.json` (`--gerbers --drill`) | `release_ready` true, `production_status` production, no overrides, with the board routed by FreeRouting as in the commands above. `lmr38010` and `ltst_c190kgkt` were reviewed on 2026-10-29 (B8). `synth check --fab` does not agree: see A18 |
| A7 | Schematic electrical rules | `kicad-cli sch erc`, `reports/buck.kicad-erc.json` | 0 violations. The earlier `lib_symbol_mismatch` for SS36 is gone: SS36 is now drawn with `Device:D_Schottky`. The two `power_pin_not_driven` errors that the new part would have caused are fixed by a Synth change that lets a generated symbol anchor the PWR_FLAG |
| A8 | The schematic netlist matches the source | `kicad-cli sch export netlist` | 13 nets, node for node the source's 7 `net` blocks plus the 6 further names its 10 `connect` statements create: GND 18 nodes, `+3V3` 10, VIN 9, FB 4, PG 4, EN 3, SW 3, BOOT 2, RT 2, VIN_FUSED 2, VIN_RAW 2, LED_PWR 2, LED_PG 2. 29 components |
| A9 | A layer change without a nearby ground return via is reported | `W-SYNTH-DRC-016` in both release files | 0 findings. A Synth change made the return-via pass use the same window as the rule (the thickest height the rule derives from the default four-layer stackup, 0.6377 mm, where the pass used 1.53 mm) and lets it try the window's diagonal corners. The layer changes are the 24 GND vias and one `PG` via; no power net leaves F.Cu. `synth capability list` still declares return-path analysis unsupported, so this is a proximity test, not a return-path analysis |
| A10 | Manufacturing exports exist and are complete | `kicad/gerbers/` (22 Gerbers plus the `.gbrjob`), `kicad/drill/` | 4 copper layers (F, In1, In2, B), mask, paste, silkscreen and Edge.Cuts present, Edge.Cuts traces the 64.99 x 24.99 mm outline (unchanged by C10 and C11); PTH drill 31 holes (25 x 0.3 mm vias, 4 x 0.95 mm and 2 x 1.0 mm connector pins), NPTH 2 x 2.5 mm mounting holes. Not fabricated |
| A11 | The exported project is readable by KiCad | KiCad 10.0.7 headless | `pcb drc`, `sch erc` and `sch export netlist` load the committed files and succeed. Not opened in the KiCad GUI |
| A12 | Ground pours are stored filled | `grep -c filled_polygon kicad/buck.kicad_pcb` | 5 filled polygons for the 4 GND zones: 2 on F.Cu (the pour is cut into 2 islands by the routing) and 1 each on In1, In2 and B.Cu; the other 2 zones are rule areas and carry no fill. `kicad-cli pcb drc` reports 0 unconnected items |
| A13 | The placement is the hand-pinned sidecar and it held | `buck.placement.layout.toml`, `kicad/buck.kicad_pcb`, `pad_gaps.py` | All 29 parts are pinned with hard priority (C10 and C11 are the two added; the sidecar holds the exported positions, including C2 at 28.5 / 19.9, C3 at 27.5 / 14.14 and H2 at 61.5 / 21.5, and re-exporting from it reproduces the committed board byte for byte); the only other entries are one label offset for R_FB_T and the six thermal vias. Reason: the unassisted auto placement left C3 19.8 mm from VIN, the output capacitors 12.9 to 19.3 mm from L1 and 8 `W-SYNTH-DRC-016` warnings (reviewer's finding). Gaps in the final file are in the table below |
| A14 | The placement passes Synth's structural review | MCP `synth_describe_placement` | Not rerun on the hand-pinned placement. The earlier score 100 was for the auto placement and no longer applies |
| A15 | Power-good reaches the connector | `kicad-cli sch export netlist` (A8) | Net `PG` has exactly four nodes: `U1.pg` (pin 6, open drain), `R_PG.p1` (100k to `+3V3`), `J2.p3` and `D4.cathode` (A17). The connection and the pull-up are proven; the behaviour is B3 |
| A16 | Thermal copper and vias exist under the converter | `kicad/buck.kicad_pcb`, `reports/buck.connectivity.json` | U1 pin 9 is a 2.41 x 3.1 mm exposed pad in the `GND` net, soldered to the F.Cu pour, with 6 GND vias inside it (2 x 3, 0.6 mm pad, 0.3 mm drill). GND vias within 1 / 2 / 3 / 4 mm of the pad centre: 2 / 6 / 6 / 7 (unchanged by C10 and C11). The router no longer moves these vias: it leaves a via in a pad alone when the board approves via-in-pad (`requires via_in_pad`); before that the post-route via mover pushed four of the six out of the pad. The board has 25 vias: 24 GND and one on `PG`. The board is `jlc-filled-via` with `requires via_in_pad`. The copper is proven; the temperature is B2 |
| A17 | The power-good fault LED is wired as designed | `kicad-cli sch export netlist` (A8), `kicad/bom.csv`, `kicad/buck.kicad_pcb` | `D4` (`ltst_c190kgkt`, value "PG FAULT") has its anode on net `LED_PG` and its cathode on `PG`; `R_PGLED` (470 R) joins `+3V3` to `LED_PG`. Both are in the BOM. The LED current is a hand figure: (3.3 - 2.1) / 470 = 2.55 mA, taking Vf 2.1 V from the 1.9 to 2.4 V range, with `PG` held low by U1's 92 ohm typ on-resistance, so about 0.2 V. "PG FAULT" is the value field on F.Fab and is not printed on the silkscreen; only the reference D4 is. That the LED lights on a fault is B3 |
| A18 | `synth check` verdict from the matrix row | `synth check <board> --json` and `--fab --json` (KiCadRoutingTools `61bde44`) | Release: pass. `--fab`: source, DRC and native KiCad verification pass; manufacturing `fail` because the KiCadRoutingTools copper has 11 vias without a ground return via within 0.64 mm. Matches `expect = "pass"`, `expect_fab = "fail"` |

Pad-edge gaps (`pad_gaps.py`, final routed file), against the first auto placement in brackets where it was recorded:

| Pair | Gap mm |
| --- | --- |
| C3 to U1 VIN | 0.82 (19.80) |
| C2 to U1 VIN | 4.07 (10.86) |
| C10 / C11 to U1 VIN | 3.73 / 3.49 |
| C3 / C2 ground pad to U1 GND | 2.69 / 6.89 (21.22 / 13.75) |
| C10 / C11 ground pad to U1 GND (pin 1) | 0.90 / 3.66 |
| C4 to U1 BOOT / SW | 0.62 / 2.18 (6.49 / 4.63) |
| R_RT to U1 RT/SYNC | 1.22 |
| L1 pad 1 to U1 SW | 3.80 (3.33) |
| C5, C6, C9 to L1 pad 2 | 0.70 / 2.10 / 0.70 (14.35 / 12.85 / 19.33) |
| D2 TVS to U1 VIN | 11.95 (10.00) |
| R_UV_T to U1 EN | 1.87 (2.33) |
| R_PG to U1 PG | 16.65 (24.13) |
| R_FB_T / R_FB_B to U1 FB | 3.55 / 3.79 (3.39 / 13.77) |
| C7 to R_FB_T | 0.55 (8.80) |
| R_FB_B ground pad to U1 GND | 11.31 (5.86) |
| R_FB_T FB pad to L1 pad 1 (SW) | 3.58 (8.98) |

## Placement and return-path review (final 29-part board)

My own measurement on the final files:
- Placement: 29 components, all pinned. C10 and C11 sit in the free band above U1 (y 10.0 mm, x 29.0 and 33.8 mm, same orientation, VIN pad left, ground pad right); no other part was moved. Their ground pads are over U1's pin 1 and exposed pad (C10 ground pad 0.90 mm from pin 1, C10 / C11 1.72 / 1.63 mm from the exposed pad). Input chain J1 -> F1 -> D1 below U1, so VIN does not cross the EN divider. J2 is rotated 270 degrees so PG (pin 3) and +3V3 (pin 1) do not cross. C5, C6 and C9 form a short column right of L1. R_PG and the PG LED chain sit beside J2.
- Numbers: C3 0.82 mm, C11 3.49 mm, C10 3.73 mm and C2 4.07 mm from the VIN pad, C5/C6/C9 0.70 / 2.10 / 0.70 mm from L1 pad 2, L1 pad 1 3.80 mm from SW (table above). R_PG is 16.65 mm from the PG pin and R_FB_B's ground is 11.31 mm from U1 GND, both left as they are.
- `W-SYNTH-DRC-016`: 0 in `release.json` and `release-fab.json`.
- Layer changes: 25 vias, 24 on GND (6 in U1's pad, 18 from the router and the return-via pass, not split in the report) and one on `PG`, a signal net that dips to B.Cu for 3 segments with a GND return via beside it (0 `W-SYNTH-DRC-016`); no power net changes layer, 92 of 95 segments are on F.Cu.
- Not judged: current-loop area, stackup impedance, EMI, ripple, and whether 18 router-placed GND vias are in the right places. Those are for the expert (B5, B12, B13).

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | Survives a surge or a sustained overvoltage | The converter is rated 4.2-80 V operating and 85 V absolute maximum (SW 85 V). SMBJ30A has VBR 33.3-36.8 V and VC 48.4 V at 12.4 A (Vishay smbj.pdf, 600 W 10/1000 us), below the 85 V maximum, so a surge the TVS can absorb leaves the regulator inside its absolute maximum. A SUSTAINED overvoltage is limited by the PTC F1 (Vmax 33 V) and the 50 V capacitors C1, C2, C10, C11 and C3. No surge test was run and no IEC 61000-4-5 rating is claimed. For the expert: the TVS energy, the PTC under surge, and the capacitors at the 48.4 V clamp (50 V parts, 1.6 V margin). Datasheet numbers only | Surge clamp below the converter maximum; nothing tested |
| B2 | Thermal copper, thermal vias, thermal relief on pours and junction temperature | `synth capability list` declares `thermal_relief_analysis` unsupported: zone relief gap and spoke width are fixed export defaults. Six vias sit in the pad where TI asks for at least a 4 x 3 array of 10 mil vias (9.5.1.1). The datasheet's `RthetaJA` of 42.9 C/W is not valid for design and no copper-area curve exists in it, so no junction temperature is estimated; the loss at 1 A and an assumed 85% efficiency is 0.58 W. Via-in-pad needs filled and capped vias, not confirmed with a fab | Copper present; temperature unverified |
| B3 | Power-good behaviour | The connection and the LED wiring are proven (A15, A17); the LED is an indicator, not a check on the thresholds. The datasheet gives rising 110/112/114% and falling 90/92/94% thresholds, a 40 us glitch filter and a 4 ms start-up delay, but nothing declares those limits and `synth capability list` marks `power_sequencing_and_power_good` unsupported. Nothing was measured | Unsupported by Synth; unverified by an expert |
| B4 | Input and output filtering, loop stability, capacitor derating | Input: at least 4.7 uF plus 100-220 nF (C2, C10 and C11 10 uF each, C3 220 nF, C1 47 uF). Output: 3 x 22 uF (Samsung CL31A226KAHNNNE, 25 V X5R, +/-15 % over -55 to 85 C); the datasheet Table 9-1 has no 3.3 V row. L1 6.8 uH gives 0.53 A pp at 26.4 V in and 800 kHz; the Eq 11 minimum is 1.03 uH. DC-bias derating, read off the Samsung library data-sheet plots on 2026-11-06 (approximate, plot-reading error about 5 points): C2, C10 and C11 (10 uF, 50 V X7R) lose about 80 % of their capacitance at 24 V, so about 2 uF effective each and about 6 uF for the three in parallel; C5, C6 and C9 (22 uF, 25 V X5R) lose about 25 % at 3.3 V, so about 16 to 17 uF each, about 50 uF of the nominal 66 uF. The input stage therefore meets the datasheet's 4.7 uF minimum on paper with ceramics alone at 24 V (about 6 uF effective from the three 10 uF parts, approximate, a plot reading; C3 220 nF 0603 is also derated, not read, and is not counted); C1 47 uF electrolytic, ESR unknown, adds bulk. The margin over 4.7 uF is about 1.3 uF and is as good as the plot reading (about 5 points per part). ESR from the same plots is of the order of 10 to 30 mOhm near 800 kHz (log-scale plot, rough). The X5R +/-15 % band is temperature only. C1 ESR is not given; its ripple rating is 120 mA at 120 Hz. Crossover, phase margin and load-transient response were not computed or measured | Requires expert review |
| B5 | Switch-node loop area, return paths, EMI and ripple | Only pad-edge gaps were measured (table above). No loop area, inductance, ripple or EMI result. The input capacitors C3, C11, C10 and C2 are 0.82, 3.49, 3.73 and 4.07 mm from the VIN pad; their ground pads are 2.69, 3.66, 0.90 and 6.89 mm from the GND pin 1 pad (C10 and C11 are 1.72 and 1.63 mm from the exposed GND pad, with the six thermal vias between). The C2 and C3 ground leg is still longer than the VIN leg. Not analysed further | Requires expert review |
| B6 | Analog placement separation and grounding | `synth capability list` declares `split_ground_planes` unsupported: each ground net gets one unsplit zone. R_FB_T is 3.55 mm and R_FB_B 3.79 mm from U1 pin 5; R_FB_B's ground is 11.31 mm from U1 pin 1 and reaches it through the plane. R_FB_T's FB pad is 2.58 mm from L1 pad 1 (SW). Whether that is acceptable is for a reviewer | Requires expert review |
| B7 | Reverse-polarity block, PTC margin, UVLO thresholds, output voltage | All by hand calculation from datasheet numbers (README): PTC 0.162 A at 24 V and 0.240 A at the UVLO stop voltage against Ihold 0.63 A at 70 C; UVLO start 18.4 V (16.2 to 20.6) and stop 16.2 V (14.0 to 17.9); Vout 3.315 V. No bench measurement | Requires expert review |
| B8 | Part pins and footprints against the manufacturer drawings | `synth registry qualify --part <id>` (`reports/registry-qualify.txt`): all 11 registry parts qualify (`lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`, `mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`, `ltst_c190kgkt`, `c_samsung_cl31b106kbhnnne`, `c_samsung_cl31a226kahnnne`; reviewer Steve Munene, 2026-10-28; the two capacitors 2026-11-06), each compared with its manufacturer datasheet. `lmr38010` was reviewed on 2026-10-29 and now qualifies too. Five of the seven were compared for pin function, polarity and land pattern against the stock KiCad footprint; the KiCad pads differ from the datasheet land patterns in places (the SS36 and SMBJ30A pads sit wider apart than Vishay's reference, the SRN6045TA pads are 0.55 mm longer inward, the MF-MSMF110/33X pads are 1.125 mm long against Bourns' 1.68 mm). The electrolytic (Honor Elec RVT1H470M0810, JLCPCB C2895739) was checked for value, voltage, size and part number; its datasheet does not state the polarity marking or a land pattern, so pin 1 as + follows the KiCad `CP_Elec_8x10` convention and is not verified. `mf_msmf110_33x` also reports `E-SYNTH-QUAL-016` as a review finding (its footprint names a 3D model that is not installed). The two JST headers were checked for pitch, hole diameter, pad diameter and the 3 A / 250 V rating; pin-1 polarity is not stated in the catalogue. The two Samsung capacitors were checked for value, voltage, dielectric, size and part number against their datasheets (see the review records below); no pin or land-pattern drawing was compared, and the stock `C_1206_3216Metric` land pattern is not the Samsung recommended one. The 0603 passives have no MPN. `ltst_c190kgkt` is now included in that report (qualified), with its review record signed on 2026-10-29 | Requires expert review |
| B9 | Compactness | Outline 64.99 x 24.99 mm (1624 mm2); the previous layout on the current exporter was 73.99 x 36.99 mm (2737 mm2), so this one is 41 % smaller. The earlier auto placement of the LMR33640 version was 60.75 x 31.855 mm (1935 mm2); before that 70.99 x 31.125 mm (2210 mm2, a hand layout) and 58.0 x 32.6 mm (1891 mm2, the SOT-23-6 version). The outline follows the pinned part positions and the corner holes. H1 sits in the empty top middle and H2 below J2, C1 moved to the top left and the input chain is one row, which removed the empty bands; the cost is a longer C2 / D2 distance to U1 VIN (4.07 / 11.95 mm), not analysed (B5, B12). Adding C10 and C11 did not change the outline: they sit in the free band above U1, so it is still 64.99 x 24.99 mm (1624 mm2), 41 % smaller than the 73.99 x 36.99 mm (2737 mm2) layout, and 0 % different from the board before C10 and C11 | Not a defect; not compared |
| B10 | Keepouts and shielding | The language has keepouts; none was declared under the switch node or the inductor in this design, and none was checked | Unsupported here |
| B11 | Power sequencing, soft-start and inrush | The converter has internal soft start and a 4 ms power-good delay. Inrush into C1 and the output capacitors, and the sequencing of the PG signal against the output, were not analysed or measured | Unverified |
| B12 | Decoupling placement | Gaps are in the table above. They are short for C3, C10, C11, C2, C4, C5, C6, C9 and C7, and long for R_PG (16.65 mm, a pull-up) and R_FB_B's ground (11.31 mm). Loop inductance was not computed | Unverified |
| B13 | Stackup and impedance assumptions | The board uses the default 4-layer stackup with inner ground planes. No stackup block was declared, no controlled-impedance net exists, and the capability descriptor marks inner-layer impedance unsupported. The return-via window of 0.6377 mm comes from that default stackup | Not applicable to a single-ended power board |
| B14 | What the Gerbers, drill files and BOM say about orderability | The files in `kicad/` were generated and hashed (A10), but nothing was fabricated, no fab accepted them, the BOM carries LCSC numbers for C1, C2, C10, C11, C5, C6 and C9 (`lcsc_pn` column) and none for the 0603 passives, which also have no MPN. Availability not checked. Whether a fab takes the six filled vias in the pad is not confirmed | Not done |
| B15 | Schematic correctness and readability | ERC has 0 violations (A7) and the netlist matches the source (A8). The sheet was scored with `tools/schematic_quality.py`: 0 overlapping text pairs, 7 texts with a wire through them, 1 wire crossing, 1313.2 mm of wire, mean passive-to-pin distance 66.4 mm (max 172.8) | Readable, not tidy |
| B16 | The other negative cases from #38 (missing pair constraints, invalid stackup assumptions) | This board has no differential pair and no stackup claim, so those cases do not apply here; the USB board (#103) carries them. Only the unsafe-feedback case is built here | Not applicable here |
| B17 | Comparison with a human reference design | Only TI's datasheet equations were used. No reference layout was compared (#68) | Not done |
| B18 | Function on real hardware | Nothing was powered, programmed or measured. Two M2.5 mounting holes exist, so the board can be fixed down | Not done |
| B19 | Independent mixed-signal / RF sign-off (#38) | No reviewer | Not done |

### Review record: D3, D4 Lite-On LTST-C190KGKT

Source: https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF, Spec No. DS22-2000-074, Revision O, effective 09/04/2021, read 2026-10-29. Registry file `registry/parts/diodes/ltst_c190kgkt.synth.toml`.

| Item | Datasheet | Registry / board | Agrees |
| --- | --- | --- | --- |
| Body | 1.6 x 0.8 mm, 0.8 mm tall, 0.4 mm terminals | KiCad `LED_0603_1608Metric` | yes |
| Polarity | cathode marked on the package | pin 1 cathode, pin 2 anode; Device:LED K=1, A=2; footprint pad 1 cathode, pad 2 anode | yes |
| DC forward current | 30 mA max (power 75 mW, reverse 5 V) | `max_current_ma` 30 | yes |
| VF | 1.9-2.4 V at 20 mA | description "Vf 1.9 to 2.4V at 20mA" | yes |
| Optical | IV 18-71 mcd at 20 mA, dominant wavelength 567.5-576.5 nm (green AlInGaP), IR 10 uA at 5 V | green in description | yes |
| Pads | recommended 0.8 x 0.8 mm each, 2.3 mm outer span | KiCad pads 0.875 x 0.95 mm at +-0.7875 mm, outer span 2.45 mm | differs: 0.075 mm wider, 0.15 mm longer in Y, 0.15 mm larger span |
| Width 2.96 mm | not a datasheet figure | footprint courtyard width | n/a |

Circuit check: D3 from +3V3 through R_LED 470 ohm carries (3.3 - 2.4 to 1.9) / 470 = 1.9 to 3.0 mA, far below 30 mA. D4 through R_PGLED likewise.

Reviewed by Steve Munene on 2026-10-29; `reviewed_by` and `reviewed_at` are set in the registry file.

### Review record: C2, C10, C11, C5, C6, C9 Samsung MLCCs

Registry files `registry/parts/passives/c_samsung_cl31b106kbhnnne.synth.toml` (C2, C10, C11) and `c_samsung_cl31a226kahnnne.synth.toml` (C5, C6, C9), read 2026-11-06. What the data sheets state is entered; DC-bias and ESR values are read off the plots of the Samsung library data-sheet PDFs (approximate).

| Item | C2, C10, C11 CL31B106KBHNNNE (LCSC C89632) | C5, C6, C9 CL31A226KAHNNNE (LCSC C12891) |
| --- | --- | --- |
| Source | [LCSC PDF](https://www.lcsc.com/datasheet/lcsc_datasheet_1810221111_Samsung-Electro-Mechanics-CL31B106KBHNNNE_C89632.pdf) was **not readable** here; the Samsung [component library page](https://weblib.samsungsem.com/mlcc/mlcc-ec-data-sheet.do?partNumber=CL31B106KBHNNN) for CL31B106KBHNNN was read | [LCSC PDF](https://www.lcsc.com/datasheet/lcsc_datasheet_2304140030_Samsung-Electro-Mechanics-CL31A226KAHNNNE_C12891.pdf) read |
| Capacitance, tolerance | 10 uF, +/-10 % (library page) | 22 uF, +/-10 % |
| Rated voltage | 50 Vdc (library page) | 25 V |
| Dielectric | X7R (library page) | X5R |
| Temperature | -55 to 125 C per a distributor listing (Zouser, [CL31B106KBHNNNE](https://www.zouser.com/datasheet/cl31b106kbhnnne)); the manufacturer PDF could not be read; consistent with the X7R code. The Samsung library page does not print a range | -55 to 85 C, +/-15 % over temperature |
| Size | 1206 (3216 mm); L 3.20 +/-0.20, W 1.60 +/-0.20, T 1.60 +/-0.20 mm (Samsung library page; the LCSC PDF was not read). The Zouser listing gives L 3.20 x W 1.60 mm and thickness 1.80 mm max; the two thickness figures disagree and are not reconciled here | L 3.20 +/-0.30, W 1.60 +/-0.30, T 1.60 max mm; terminations Ni/Cu/Ni/Sn |
| Footprint | KiCad `C_1206_3216Metric`; registry `footprint_dimensions` 4.6 x 2.3 mm | same |
| DC bias | about -80 % at 24 V (about 2 uF); -60 % near 10 V; -90 % at 50 V (plot, 1 kHz 1 Vrms) | about -25 % at 3.3 V (about 16.5 uF); -67 % at 10 V; -90 % at 25 V (plot, 0.12 kHz 0.5 Vrms) |
| ESR near 800 kHz | order of 10 to 30 mOhm (log plot, rough) | order of 10 mOhm (log plot, rough) |
| Data-sheet PDFs | Samsung Component Library "DataSheet_CL31B106KBHNNN", created 2026-11-06 21:02 KST, printed by Steve Munene | "DataSheet_CL31A226KAHNNN", created 2026-11-06 20:48 KST, printed by Steve Munene |

The 5 V rows of the LMR38010 table ask for 22 uF at the output, and no 22 uF 25 V X7R 1206 part was found, so C5, C6 and C9 changed from X7R to X5R. Open items: (1) the DC-bias and ESR values above are plot readings, not tabulated data; a reviewer should confirm them from the Samsung Component Library or measure. (2) The input filter decision is made and applied: C10 and C11, the same part as C2, were added in parallel, so about 6 uF effective at 24 V (3 x about 2 uF) against the datasheet minimum of 4.7 uF; this still rests on the plot reading above, not on tabulated data or a measurement, and C3 (220 nF 0603) is not read. (3) Stability with the derated output capacitance (about 50 uF at 3.3 V) is not computed. (4) The CL31B106KBHNNNE manufacturer or LCSC PDF still has to be read to confirm the temperature range (only a distributor listing gives it) and the package drawing and thickness (1.60 +/-0.20 mm on the Samsung library page, 1.80 mm max on the distributor listing). (5) Availability and price at LCSC were not checked. Status unchanged: requires expert review; artifact generation only.

### Review record: U1 TI LMR38010SDDAR

Source: https://www.ti.com/lit/ds/symlink/lmr38010.pdf, SNVSBM3B (text copy), read 2026-10-29. Registry file `registry/parts/regulators/lmr38010.synth.toml`.

- Pins: gnd 1, en 2, vin 3, rt_sync 4, fb 5, pg 6, boot 7, sw 8, pad 9; the datasheet agrees.
- Ratings: operating 4.2 to 80 V (registry min 4.2, max 80, 1000 mA); absolute maximum VIN 85 V, SW 85 V, EN VIN + 0.3 V, PG 20 V.
- Footprint: exposed pad 2.41 x 3.1 mm against the datasheet DDA land pattern 2.40 x 3.10 mm (0.01 mm, within drawing tolerance). Signal pads 1.6 x 0.6 mm at 1.27 mm pitch were not compared line by line.
- Application: Cin at least 4.7 uF plus 100-220 nF (met); Eq 11 inductance minimum 1.03 uH (met by 6.8 uH); R_RT 33k for 800 kHz from the RT table under Figure 8-4; VREF 1.0 V unchanged so the feedback divider is unchanged.
- `RthetaJA` 42.9 C/W is not valid for design (datasheet text); not used.

Reviewed by Steve Munene on 2026-10-29; `reviewed_by` and `reviewed_at` are set in the registry file.

Rows A2, A9 and B1 to B8 are the ones that decide whether this board meets a Class B bar (#38
treats missing SI/PI/RF evidence as a failure). They are open or bounded, not closed.

## C. #38 traceability for case 1

Every line of `#38`'s "Check exactly" list, against this board.

| `#38` item | Verdict here | Where |
| --- | --- | --- |
| Differential-pair membership and geometry, length constraints | Not applicable: no differential pair on this board | B16 |
| Return paths and layer transitions | 0 `W-SYNTH-DRC-016`; layer changes are 24 GND vias and one `PG` via with a GND return via beside it; no return-current analysis, and the capability descriptor says so | A9, B5 |
| Impedance assumptions | Not applicable: no controlled-impedance net, no stackup block | B13 |
| Keepouts | Unsupported here: the language has them, none was declared or checked | B10 |
| Analog placement separation | R_FB_T 3.55 mm and R_FB_B 3.79 mm from the FB pin; divider ground 11.31 mm from U1 GND; not judged | B6 |
| Grounding | One unsplit GND zone per layer; split planes unsupported | B6, A9 |
| Decoupling | Boot and decoupling capacitors 0.55 to 3.80 mm from the pin they serve; loop inductance not computed | B12 |
| Thermal relief | Fixed export defaults, not analysed; six vias in the pad; the temperature is not proven | A16, B2 |
| Connector ESD paths | D2 SMBJ30A on VIN is the transient clamp; J1 and J2 are 2- and 4-pin power connectors with no signal pins | B1, README |
| Power sequencing | The power-good signal exists and is pulled up (A15); its thresholds, delay and fault behaviour are not checked or measured | A15, A17, B3, B11 |
| ERC and DRC | 0 violations on ERC, 0 errors and 25 `lib_footprint_mismatch` warnings on DRC | A4, A7 |
| Schematic correctness | Netlist equals the source net for net; readability scored, not tidy | A8, B15 |
| BOM correctness | 29 components, one BOM row each (C2, C10 and C11 are three rows of the same part, not grouped), MPNs on C1, C2, C10, C11, C5, C6, C9 and the ICs, none on the 0603 passives; `lcsc_pn` set on C1, C2, C10, C11, C5, C6, C9 only | A6, B14 |
| Footprint correctness | 11 of 11 registry parts qualify; the LED footprint differs from the datasheet pad recommendation | B8 |
| Manufacturing exports | Gerbers, drill, BOM and pick-and-place generated and hashed; not fabricated | A10, B14 |
| KiCad reopenability | Loaded headlessly by KiCad 10.0.7 for every export used here; not opened in the GUI | A11 |
| Which claims Synth proves | Section A, and only section A | A1-A18 |
| Which claims need SI/PI/RF expert analysis | Section B | B1-B19 |
| Comparison against a human reference | Only datasheet equations; no layout comparison | B17 |

All four features #38's first case names are carried by the board: input and output filtering
(A3, B4, B12), feedback (A2, A8), power-good (A15, A17, B3) and thermal copper (A16, B2). The last two
are present and connected but unverified.

## D. Matrix row

The row for this board is in `qualification/matrix.toml` in this branch, in the schema of open PR #247
(`id = "buck"`, `class = "B"`, `status = "experimental"`, `expect = "pass"`, `expect_fab = "fail"`, with a
rationale from the measurements above). That file holds only this row. The harness
(`crates/synth-cli/tests/qualification_matrix.rs`) and the full matrix are in #247, which is not on `main`,
so when #247 lands `qualification/matrix.toml` will conflict as an add/add; keep the buck row as it is.
Nothing in this branch runs the row yet, so the board is not discoverable by the qualification harness until
then.

How the values were chosen, from the #247 harness: `expect` is compared with the source and DRC stages
together (A1, A4: pass); `expect_fab` is compared with the manufacturing stage under `--fab`.

`synth check` was run on 2026-10-29 against this board with KiCadRoutingTools `61bde44` (`SYNTH_KRT_PYTHON` set
to a venv holding scipy and shapely) and the DRC-stage budget raised from 30 s to 120 s (the 30 s budget made
the DRC stage report `unknown` on a loaded machine). Results: without `--fab`, source `pass`, DRC `pass`,
overall `pass`, as `expect = "pass"` says. With `--fab`, the DRC stage passes and the native verification
passes (kicad_fab, kicad_erc and kicad_drc, 0 violations each), but the manufacturing stage is `fail`: that
stage routes with KiCadRoutingTools, not FreeRouting, and its copper leaves 11 signal vias without a ground
return via within 0.64 mm (`W-SYNTH-DRC-016`), so the release manifest is `validation_failed` and `untrusted`.
The committed copper (FreeRouting) has none of them (A9). `expect_fab` therefore stays `fail`; the reason is
the router, no longer the part reviews. Closing it needs the return-via pass to work on KiCadRoutingTools
output, or the matrix row to route with FreeRouting.
