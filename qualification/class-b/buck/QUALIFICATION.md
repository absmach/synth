# Qualification matrix: 24 V to 3.3 V buck board (SYN-67)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the
evidence. Section B lists what no tool here proves and an SI/PI/RF or power expert has to sign
off. No expert has reviewed this board. Every part on the board that carries a registry review record is signed (Steve Munene); the generic passives and the mounting hole have none.

This is one board in Class B (#38) and it carries **case 1 only**, the switching regulator:
24 V to 3.3 V with input and output filtering, feedback, power-good and thermal copper. The analog
front end, the USB/Ethernet peripheral and the other negative cases belong to other boards.
Section C maps every `#38` checklist item to a row here, including the ones that do not apply.

## Reproduce

Run from the repository root with the Synth code of this branch. Base: `git merge-base HEAD upstream/main`
is `c5caf90` (upstream `main` when this branch last merged it; `main` has moved since). The Synth changes this
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
Synth 0.1.1, on 2026-10-29. `SYNTH_SOURCE_REVISION` makes `buck.routing.json` record `source_revision`: it holds
`a79570cd84d7c9dcc67931593a42fc3836b76065`, the commit the reports were regenerated from (the copper is byte-identical to the previous run, whose revision was `a9c7546`). The JSON reports were generated with a relative board path and the
repository root prefix was stripped with `sed`; `out/` is a scratch directory and is not committed. A different
router version can give different copper. `reports/registry-qualify.txt` was regenerated for the 9 registry parts on the board
with `synth registry qualify --part <id>` (`lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`,
`mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`): all eight qualify (`lmr38010` was reviewed on 2026-10-29).

## Artifact hashes

`SHA256SUMS` lists every design artifact (source, sidecar, KiCad project, Gerbers, drill files,
BOM, pick-and-place, pictures, routing, DRC and ERC reports, `pad_gaps.py`). It leaves out `README.md`,
`QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `buck.synth` | `2b22a64fcf7e5247f787aa08f763d336fbd5a44b16989445bf39f929d6d6cb49` |
| `buck_neg_fb_sw.synth` | `c0f7ef273bf90689ad805b09ec6e736099cadc59943eaeec53015f51edbe38f8` |
| `buck.placement.layout.toml` | `fd6298e3eca76d955f53b93abe8b3e1528f66276411a4156d1b05a65770794c9` |
| `kicad/buck.kicad_pcb` | `93d7b760e2caa0c09cb27fd1a429857efeed4dde174b62e6ff59a3fcab9f34ea` |
| `kicad/buck.kicad_sch` | `0c4bf60be83892bc186e555484b0b65c9c6ab9aa0d2b5c99a2e6fcc7bb77bc45` |
| `kicad/bom.csv` | `2a4faa316306140a1e6178fcf388840e6913b05edd23a92547fcd7f0b80dadce` |
| `reports/release.json` | `b84d5f6f9a1bc4905c96b6ca5358ca62c09a73e180c5af22b85f4e2cb90bc0e4` |
| `reports/release-fab.json` | `a88da875adeac3b2dfbcdb5465b8881e2ea3c1f50e5aa2d547524611f56655aa` |
| `reports/buck.routing.json` | `0bd40e39186408234da897f39debfc993b0222570c4294f22e25ee7c6bfef9e2` |
| `reports/buck.kicad-drc.json` | `1d8f117b95a1728dda9771a4140e416021027857e71325fdcf7d167396a400dc` |
| `reports/buck.kicad-erc.json` | `ad9091137e7be2f3a8cbe14d1a9b4f066f2f5c44ea276658b288c9d3b23c2726` |

Datasheet values used for the part values are in `README.md` ("Parts and datasheets"). The
datasheets are not committed, so the figures taken from them cannot be checked from this
repository.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate buck.synth` | Exit 0, 0 errors, 11 warnings: 3 `E-SYNTH-SCHEM-003`, 1 `E-SYNTH-SCHEM-004`, 4 `E-SYNTH-SCHEM-005`, 1 `W-SYNTH-ANOMALY-001`, 2 `W-SYNTH-SUPPLY-002` |
| A2 | Regulator feedback is not taken from the switch node | `E-SYNTH-POWER-011` on `buck_neg_fb_sw.synth` | The twin exits 1 with the error and the same 11 warnings; the good board does not raise it. The rule follows one series resistor only, so a longer feedback path is not checked |
| A3 | Every net is connected end to end | `reports/buck.connectivity.json`, `reports/buck.routing.json` | 13 nets, 0 open, 0 unconnected pads, 97 segments (all F.Cu), 21 vias in the routing statistics and in the PCB file, 200.08 mm of copper. The only via-in-pad are the six declared thermal vias (`via_in_pad` list in the connectivity report) |
| A4 | No KiCad DRC rule violation on the delivered board | `kicad-cli pcb drc --refill-zones`, `reports/buck.kicad-drc.json` | 0 errors, 0 unconnected, 0 warnings |
| A5 | The route passes Synth's source/DRC gate | `reports/release.json` (`synth export-kicad`, no fab flags) | `release_ready` true, `production_status` production, state `routed`, 0 signal-integrity findings (blocking 0, warnings 0) |
| A6 | The package passes Synth's release gate | `reports/release-fab.json` (`--gerbers --drill`) | `release_ready` true, `production_status` production, no overrides, with the board routed by FreeRouting as in the commands above. `lmr38010` and `ltst_c190kgkt` were reviewed on 2026-10-29 (B8). `synth check --fab` does not agree: see A18 |
| A7 | Schematic electrical rules | `kicad-cli sch erc`, `reports/buck.kicad-erc.json` | 0 violations. The earlier `lib_symbol_mismatch` for SS36 is gone: SS36 is now drawn with `Device:D_Schottky`. The two `power_pin_not_driven` errors that the new part would have caused are fixed by a Synth change that lets a generated symbol anchor the PWR_FLAG |
| A8 | The schematic netlist matches the source | `kicad-cli sch export netlist` | 13 nets, node for node the source's 7 `net` blocks plus the 6 further names its 10 `connect` statements create: GND 16 nodes, `+3V3` 10, VIN 7, FB 4, PG 4, EN 3, SW 3, BOOT 2, RT 2, VIN_FUSED 2, VIN_RAW 2, LED_PWR 2, LED_PG 2. 27 components |
| A9 | A layer change without a nearby ground return via is reported | `W-SYNTH-DRC-016` in both release files | 0 findings. A Synth change made the return-via pass use the same window as the rule (the thickest height the rule derives from the default four-layer stackup, 0.6377 mm, where the pass used 1.53 mm) and lets it try the window's diagonal corners. The only layer changes are the 21 GND vias; no signal net leaves F.Cu. `synth capability list` still declares return-path analysis unsupported, so this is a proximity test, not a return-path analysis |
| A10 | Manufacturing exports exist and are complete | `kicad/gerbers/` (22 Gerbers plus the `.gbrjob`), `kicad/drill/` | 4 copper layers (F, In1, In2, B), mask, paste, silkscreen and Edge.Cuts present, Edge.Cuts traces the 70.99 x 33.99 mm outline; PTH drill 27 holes (21 x 0.3 mm vias, 4 x 0.95 mm and 2 x 1.0 mm connector pins), NPTH 2 x 2.5 mm mounting holes. Not fabricated |
| A11 | The exported project is readable by KiCad | KiCad 10.0.7 headless | `pcb drc`, `sch erc` and `sch export netlist` load the committed files and succeed. Not opened in the KiCad GUI |
| A12 | Ground pours are stored filled | `grep -c filled_polygon kicad/buck.kicad_pcb` | 5 filled polygons for the 4 GND zones: 2 on F.Cu (the pour is cut into 2 islands by the routing) and 1 each on In1, In2 and B.Cu; the other 2 zones are rule areas and carry no fill. `kicad-cli pcb drc` reports 0 unconnected items |
| A13 | The placement is the hand-pinned sidecar and it held | `buck.placement.layout.toml`, `kicad/buck.kicad_pcb`, `pad_gaps.py` | All 27 parts are pinned with hard priority; the only other entries are one label offset for R_FB_T and the six thermal vias. Reason: the unassisted auto placement left C3 19.8 mm from VIN, the output capacitors 12.9 to 19.3 mm from L1 and 8 `W-SYNTH-DRC-016` warnings (reviewer's finding). Gaps in the final file are in the table below |
| A14 | The placement passes Synth's structural review | MCP `synth_describe_placement` | Not rerun on the hand-pinned placement. The earlier score 100 was for the auto placement and no longer applies |
| A15 | Power-good reaches the connector | `kicad-cli sch export netlist` (A8) | Net `PG` has exactly four nodes: `U1.pg` (pin 6, open drain), `R_PG.p1` (100k to `+3V3`), `J2.p3` and `D4.cathode` (A17). The connection and the pull-up are proven; the behaviour is B3 |
| A16 | Thermal copper and vias exist under the converter | `kicad/buck.kicad_pcb`, `reports/buck.connectivity.json` | U1 pin 9 is a 2.41 x 3.1 mm exposed pad in the `GND` net, soldered to the F.Cu pour, with 6 GND vias inside it (2 x 3, 0.6 mm pad, 0.3 mm drill). GND vias within 1 / 2 / 3 / 4 mm of the pad centre: 2 / 6 / 6 / 7. The board has 21 vias, all GND. The board is `jlc-filled-via` with `requires via_in_pad`. The copper is proven; the temperature is B2 |
| A17 | The power-good fault LED is wired as designed | `kicad-cli sch export netlist` (A8), `kicad/bom.csv`, `kicad/buck.kicad_pcb` | `D4` (`ltst_c190kgkt`, value "PG FAULT") has its anode on net `LED_PG` and its cathode on `PG`; `R_PGLED` (470 R) joins `+3V3` to `LED_PG`. Both are in the BOM. The LED current is a hand figure: (3.3 - 2.1) / 470 = 2.55 mA, taking Vf 2.1 V from the 1.9 to 2.4 V range, with `PG` held low by U1's 92 ohm typ on-resistance, so about 0.2 V. "PG FAULT" is the value field on F.Fab and is not printed on the silkscreen; only the reference D4 is. That the LED lights on a fault is B3 |
| A18 | `synth check` verdict from the matrix row | `synth check <board> --json` and `--fab --json` (KiCadRoutingTools `61bde44`) | Release: pass. `--fab`: source, DRC and native KiCad verification pass; manufacturing `fail` because the KiCadRoutingTools copper has 11 vias without a ground return via within 0.64 mm. Matches `expect = "pass"`, `expect_fab = "fail"` |

Pad-edge gaps (`pad_gaps.py`, final routed file), against the first auto placement in brackets where it was recorded:

| Pair | Gap mm |
| --- | --- |
| C3 to U1 VIN | 0.82 (19.80) |
| C2 to U1 VIN | 1.46 (10.86) |
| C3 / C2 ground pad to U1 GND | 2.69 / 5.35 (21.22 / 13.75) |
| C4 to U1 BOOT / SW | 0.62 / 2.18 (6.49 / 4.63) |
| R_RT to U1 RT/SYNC | 1.22 |
| L1 pad 1 to U1 SW | 3.80 (3.33) |
| C5, C6, C9 to L1 pad 2 | 0.83 / 2.29 / 0.70 (14.35 / 12.85 / 19.33) |
| D2 TVS to U1 VIN | 7.82 (10.00) |
| R_UV_T to U1 EN | 3.35 (2.33) |
| R_PG to U1 PG | 22.63 (24.13) |
| R_FB_T / R_FB_B to U1 FB | 3.55 / 3.79 (3.39 / 13.77) |
| C7 to R_FB_T | 0.55 (8.80) |
| R_FB_B ground pad to U1 GND | 11.31 (5.86) |
| R_FB_T FB pad to L1 pad 1 (SW) | 2.58 (8.98) |

## Placement and return-path review (final 27-part board)

My own measurement on the final files:
- Placement: 27 components, all pinned. Input chain J1 -> F1 -> D1 below U1, so VIN does not cross the EN divider. J2 is rotated 270 degrees so PG (pin 3) and +3V3 (pin 1) do not cross. C5, C6 and C9 form a short column right of L1. R_PG and the PG LED chain sit beside J2.
- Numbers: C3 0.82 mm and C2 1.46 mm from the VIN pad, C5/C6/C9 0.83 / 2.29 / 0.70 mm from L1 pad 2, L1 pad 1 3.80 mm from SW (table above). R_PG is 22.63 mm from the PG pin and R_FB_B's ground is 11.31 mm from U1 GND, both left as they are.
- `W-SYNTH-DRC-016`: 0 in `release.json` and `release-fab.json`.
- Layer changes: 21 vias, all on GND (6 in U1's pad, 15 from the router); no signal or power net changes layer, all 97 segments are on F.Cu.
- Not judged: current-loop area, stackup impedance, EMI, ripple, and whether 15 router-placed GND vias are in the right places. Those are for the expert (B5, B12, B13).

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | Survives a surge or a sustained overvoltage | The converter is rated 4.2-80 V operating and 85 V absolute maximum (SW 85 V). SMBJ30A has VBR 33.3-36.8 V and VC 48.4 V at 12.4 A (Vishay smbj.pdf, 600 W 10/1000 us), below the 85 V maximum, so a surge the TVS can absorb leaves the regulator inside its absolute maximum. A SUSTAINED overvoltage is limited by the PTC F1 (Vmax 33 V) and the 50 V capacitors C1, C2 and C3. No surge test was run and no IEC 61000-4-5 rating is claimed. For the expert: the TVS energy, the PTC under surge, and the capacitors at the 48.4 V clamp (50 V parts, 1.6 V margin). Datasheet numbers only | Surge clamp below the converter maximum; nothing tested |
| B2 | Thermal copper, thermal vias, thermal relief on pours and junction temperature | `synth capability list` declares `thermal_relief_analysis` unsupported: zone relief gap and spoke width are fixed export defaults. Six vias sit in the pad where TI asks for at least a 4 x 3 array of 10 mil vias (9.5.1.1). The datasheet's `RthetaJA` of 42.9 C/W is not valid for design and no copper-area curve exists in it, so no junction temperature is estimated; the loss at 1 A and an assumed 85% efficiency is 0.58 W. Via-in-pad needs filled and capped vias, not confirmed with a fab | Copper present; temperature unverified |
| B3 | Power-good behaviour | The connection and the LED wiring are proven (A15, A17); the LED is an indicator, not a check on the thresholds. The datasheet gives rising 110/112/114% and falling 90/92/94% thresholds, a 40 us glitch filter and a 4 ms start-up delay, but nothing declares those limits and `synth capability list` marks `power_sequencing_and_power_good` unsupported. Nothing was measured | Unsupported by Synth; unverified by an expert |
| B4 | Input and output filtering, loop stability, capacitor derating | Input: at least 4.7 uF plus 100-220 nF (C2 10 uF, C3 220 nF, C1 47 uF). Output: 3 x 22 uF; the datasheet Table 9-1 has no 3.3 V row. L1 6.8 uH gives 0.53 A pp at 26.4 V in and 800 kHz; the Eq 11 minimum is 1.03 uH. DC-bias derating of the ceramics is unknown (no vendor data). C1 ESR is not given; its ripple rating is 120 mA at 120 Hz. Crossover, phase margin and load-transient response were not computed or measured | Requires expert review |
| B5 | Switch-node loop area, return paths, EMI and ripple | Only pad-edge gaps were measured (table above). No loop area, inductance, ripple or EMI result. The input capacitors are 0.82 and 1.46 mm from the VIN pad but their ground pads are 2.69 and 5.35 mm from the GND pad, a longer ground leg than VIN leg. Not analysed further | Requires expert review |
| B6 | Analog placement separation and grounding | `synth capability list` declares `split_ground_planes` unsupported: each ground net gets one unsplit zone. R_FB_T is 3.55 mm and R_FB_B 3.79 mm from U1 pin 5; R_FB_B's ground is 11.31 mm from U1 pin 1 and reaches it through the plane. R_FB_T's FB pad is 2.58 mm from L1 pad 1 (SW). Whether that is acceptable is for a reviewer | Requires expert review |
| B7 | Reverse-polarity block, PTC margin, UVLO thresholds, output voltage | All by hand calculation from datasheet numbers (README): PTC 0.162 A at 24 V and 0.240 A at the UVLO stop voltage against Ihold 0.63 A at 70 C; UVLO start 18.4 V (16.2 to 20.6) and stop 16.2 V (14.0 to 17.9); Vout 3.315 V. No bench measurement | Requires expert review |
| B8 | Part pins and footprints against the manufacturer drawings | `synth registry qualify --part <id>` (`reports/registry-qualify.txt`): all 9 registry parts qualify (`lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`, `mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`, `ltst_c190kgkt`; reviewer Steve Munene, 2026-10-28), each compared with its manufacturer datasheet. `lmr38010` was reviewed on 2026-10-29 and now qualifies too. Five of the seven were compared for pin function, polarity and land pattern against the stock KiCad footprint; the KiCad pads differ from the datasheet land patterns in places (the SS36 and SMBJ30A pads sit wider apart than Vishay's reference, the SRN6045TA pads are 0.55 mm longer inward, the MF-MSMF110/33X pads are 1.125 mm long against Bourns' 1.68 mm). The electrolytic (Honor Elec RVT1H470M0810, JLCPCB C2895739) was checked for value, voltage, size and part number; its datasheet does not state the polarity marking or a land pattern, so pin 1 as + follows the KiCad `CP_Elec_8x10` convention and is not verified. `mf_msmf110_33x` also reports `E-SYNTH-QUAL-016` as a review finding (its footprint names a 3D model that is not installed). The two JST headers were checked for pitch, hole diameter, pad diameter and the 3 A / 250 V rating; pin-1 polarity is not stated in the catalogue. The generic passives have no MPN. `ltst_c190kgkt` is now included in that report (qualified), with its review record signed on 2026-10-29 | Requires expert review |
| B9 | Compactness | Outline 70.99 x 33.99 mm (2413 mm2). The earlier auto placement of the LMR33640 version was 60.75 x 31.855 mm (1935 mm2); before that 70.99 x 31.125 mm (2210 mm2, a hand layout) and 58.0 x 32.6 mm (1891 mm2, the SOT-23-6 version). The outline follows the pinned part positions and the corner holes. I did not isolate the cause of the growth from 1935 mm2 | Not a defect; not compared |
| B10 | Keepouts and shielding | The language has keepouts; none was declared under the switch node or the inductor in this design, and none was checked | Unsupported here |
| B11 | Power sequencing, soft-start and inrush | The converter has internal soft start and a 4 ms power-good delay. Inrush into C1 and the output capacitors, and the sequencing of the PG signal against the output, were not analysed or measured | Unverified |
| B12 | Decoupling placement | Gaps are in the table above. They are short for C3, C2, C4, C5, C6, C9 and C7, and long for R_PG (22.63 mm, a pull-up) and R_FB_B's ground (11.31 mm). Loop inductance was not computed | Unverified |
| B13 | Stackup and impedance assumptions | The board uses the default 4-layer stackup with inner ground planes. No stackup block was declared, no controlled-impedance net exists, and the capability descriptor marks inner-layer impedance unsupported. The return-via window of 0.6377 mm comes from that default stackup | Not applicable to a single-ended power board |
| B14 | What the Gerbers, drill files and BOM say about orderability | The files in `kicad/` were generated and hashed (A10), but nothing was fabricated, no fab accepted them, no part carries an LCSC number, and every generic passive has no MPN. Availability not checked. Whether a fab takes the six filled vias in the pad is not confirmed | Not done |
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
| Return paths and layer transitions | 0 `W-SYNTH-DRC-016`; layer changes are GND vias only; no return-current analysis, and the capability descriptor says so | A9, B5 |
| Impedance assumptions | Not applicable: no controlled-impedance net, no stackup block | B13 |
| Keepouts | Unsupported here: the language has them, none was declared or checked | B10 |
| Analog placement separation | R_FB_T 3.55 mm and R_FB_B 3.79 mm from the FB pin; divider ground 11.31 mm from U1 GND; not judged | B6 |
| Grounding | One unsplit GND zone per layer; split planes unsupported | B6, A9 |
| Decoupling | Boot and decoupling capacitors 0.55 to 3.80 mm from the pin they serve; loop inductance not computed | B12 |
| Thermal relief | Fixed export defaults, not analysed; six vias in the pad; the temperature is not proven | A16, B2 |
| Connector ESD paths | D2 SMBJ30A on VIN is the transient clamp; J1 and J2 are 2- and 4-pin power connectors with no signal pins | B1, README |
| Power sequencing | The power-good signal exists and is pulled up (A15); its thresholds, delay and fault behaviour are not checked or measured | A15, A17, B3, B11 |
| ERC and DRC | 0 violations on ERC, 0 errors and 0 warnings on DRC | A4, A7 |
| Schematic correctness | Netlist equals the source net for net; readability scored, not tidy | A8, B15 |
| BOM correctness | 27 components, one BOM row each, no MPN or LCSC on the generic parts | A6, B14 |
| Footprint correctness | 8 of 8 registry parts qualify; the LED footprint differs from the datasheet pad recommendation | B8 |
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
