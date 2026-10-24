# Qualification matrix: 24 V to 3.3 V buck board (SYN-67)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the
evidence. Section B lists what no tool here proves and an SI/PI/RF or power expert has to sign
off. No expert has reviewed this board.

This is one board in Class B (#38) and it carries **case 1 only**, the switching regulator:
24 V to 3.3 V with input and output filtering, feedback, power-good and thermal copper. The analog
front end, the USB/Ethernet peripheral and the other negative cases belong to other boards.
Section C maps every `#38` checklist item to a row here, including the ones that do not apply.

## Reproduce

Run from the repository root on Synth `main` at `1eac8a1` or later (it includes the filled-pour
delivery #114, the label placement #144 and the 1.5 mm edge rim #272). Use an empty
`XDG_DATA_HOME` so a user registry cannot shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh --freerouting-only
B=$(pwd)/qualification/class-b/buck
synth validate $B/buck.synth
synth validate $B/buck_neg_fb_sw.synth                     # must exit 1 with E-SYNTH-POWER-011
synth export-kicad $B/buck.synth --out out/buck \
  --allow-unverified-parts --validate-erc --router free-routing \
  --pnp --gerbers --drill                                  # the fab package, reports/release-fab.json
synth export-kicad $B/buck.synth --out out/buck-nofab \
  --allow-unverified-parts --validate-erc --router free-routing   # source/DRC gate, reports/release.json
kicad-cli pcb drc --severity-all --refill-zones --format json \
  -o $B/reports/buck.kicad-drc.json $B/kicad/buck.kicad_pcb
kicad-cli sch erc --severity-all --format json \
  -o $B/reports/buck.kicad-erc.json $B/kicad/buck.kicad_sch
kicad-cli sch export netlist --format kicadsexpr -o /tmp/buck.net $B/kicad/buck.kicad_sch
(cd $B && sha256sum -c SHA256SUMS)
```

Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.5, Synth 0.1.1 built from
`main` `1eac8a1`, on 2026-10-24. Every export run at this revision produced the same `.kicad_pcb`
(`69da985d...`) and `.kicad_sch`. A different placement or router version can give different
copper. `out/` is a scratch directory and is not committed, so the paths in `reports/` do not
resolve from a fresh checkout.

## Artifact hashes

`SHA256SUMS` lists every design artifact (source, sidecar, KiCad project, Gerbers, drill files,
BOM, pick-and-place, pictures, routing, DRC and ERC reports). It leaves out `README.md`,
`QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `buck.synth` | `b4ee63362959ac1e63225fca28d02ed49a94af07c89239544b59c590e79fbc06` |
| `buck.placement.layout.toml` | `4670d461fcafff669019778ef2d3a3794f6209523f292f8486d53eb40ddf870f` |
| `buck_neg_fb_sw.synth` | `0c9d7afe8475607bd124f96f010ec6a41a21460cff08812799748f352699db09` |
| `kicad/buck.kicad_pcb` | `69da985df34eb6ab07848871eb4b83b11f635d3cd242961f57d63b9b6cf5261a` |
| `kicad/buck.kicad_sch` | `be25f4577d716335a952ee5d587a641b04d482e59f6d6b815b42122cbf447825` |
| `kicad/bom.csv` | `864c880c95e3fa7bdf6c683c698599614e7d0e61e97286794f96348275eb3e28` |
| `reports/release.json` | `5c9a36c501ed97efb68d0c66f3d2c86d819855decc9e1c34415dbce5de5ca49b` |
| `reports/release-fab.json` | `3bb92e06c70eb1b5876ed0fd5efbbc2601504c260122b2b627085bc8f30493d3` |
| `reports/buck.kicad-drc.json` | `3c0999f22f7fb4d43cf5651bc0675f26102df14c50d83b01b2c3670be679f4a9` |
| `reports/buck.kicad-erc.json` | `8cf1da3ea528c7364ff077a59fd89d1b2a784f1fb42623e58ccd2ed95eb4c143` |

Datasheet values used for the part values are in `README.md` ("Parts and datasheets"). The
datasheets are not committed, so the figures taken from them cannot be checked from this
repository.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate buck.synth` | Exit 0, 0 errors, 16 warnings: 8 `W-SYNTH-PART-UNVERIFIED`, 3 `E-SYNTH-SCHEM-003`, 1 `E-SYNTH-SCHEM-005`, 1 `W-SYNTH-ANOMALY-001`, 3 `W-SYNTH-SUPPLY-002` |
| A2 | Regulator feedback is not taken from the switch node | `E-SYNTH-POWER-011` on `buck_neg_fb_sw.synth` | The twin exits 1 with the error and 16 warnings; the good board does not raise it. The rule follows one series resistor only, so a longer feedback path is not checked |
| A3 | Every net is connected end to end | `reports/buck.connectivity.json`, `reports/buck.routing.json` | 12 nets, 0 open, 0 unconnected pads, 81 segments, 18 vias, 220.57 mm of copper, no via-in-pad |
| A4 | No KiCad DRC rule violation on the delivered board | `kicad-cli pcb drc --refill-zones`, `reports/buck.kicad-drc.json` | 0 errors, 0 unconnected, 30 warnings: 25 `lib_footprint_issues` (library links), 3 `silk_overlap`, 2 `silk_over_copper` |
| A5 | The route passes Synth's source/DRC gate | `reports/release.json` (`synth export-kicad`, no fab flags) | `release_ready` true, `production_status` production, state `routed`, 0 blocking signal-integrity findings |
| A6 | The package is not a fabrication release | `reports/release-fab.json` (`--gerbers --drill`) | `release_ready` **false**, `production_status` `untrusted`, override `unverified_parts` naming 8 parts. This is the correct answer until #51 lands a part review |
| A7 | Schematic electrical rules | `kicad-cli sch erc`, `reports/buck.kicad-erc.json` | 0 errors, 68 warnings: 43 `lib_symbol_issues`, 25 `footprint_link_issues` (the KiCad libraries are not in the config) |
| A8 | The schematic netlist matches the source | `kicad-cli sch export netlist` | 12 nets, node for node the source's 8 `net` blocks plus the 4 names its 8 `connect` statements create: GND 16 nodes, `+3V3` 9, VIN 7, FB 4, PG 3, EN 3, SW 3, BOOT 2, U1_VCC 2, VIN_FUSED 2, VIN_RAW 2, LED_PWR 2 |
| A9 | A layer change without a nearby ground return via is reported | `W-SYNTH-DRC-016` in both release files | One advisory finding: net 3 changes layers at (51.67, 23.38) mm with no ground via within 0.64 mm. The via at that point is on `+3V3`; "net 3" is Synth's numbering, and in the `.kicad_pcb` net 3 is FB. Not blocking; whether it matters is for the reviewer. `synth capability list` still declares return-path analysis unsupported, so read this check as a proximity test |
| A10 | Manufacturing exports exist and are complete | `kicad/gerbers/` (22 Gerbers plus the `.gbrjob`), `kicad/drill/` | 4 copper layers (F, In1, In2, B), mask, paste, silkscreen and Edge.Cuts present, Edge.Cuts traces the 71.9 x 31.1 mm outline; PTH drill 24 holes (18 x 0.3 mm vias, 6 connector pins at 0.95/1.0 mm), NPTH 2 x 2.5 mm mounting holes. Not fabricated |
| A11 | The exported project is readable by KiCad | KiCad 10.0.7 headless | `pcb drc`, `sch erc`, `sch export netlist`, `sch export pdf`, `pcb render` and `pcb export pos` all load the committed files and succeed. Not opened in the KiCad GUI |
| A12 | Ground pours are stored filled | `grep -c filled_polygon kicad/buck.kicad_pcb` | 7 filled polygons for the 6 zones (4 GND, one per layer, plus two split pieces), so KiCad opens showing the connectivity the gate validated |
| A13 | The placement sidecar is what was placed | sidecar compared with `kicad/buck.kicad_pcb` | All 25 parts match within 0.01 mm, except J1 and J2, which differ by the documented JST pin-1 anchor offset (-1.25, -0.525) mm |
| A14 | The placement passes Synth's structural review | MCP `synth_describe_placement` with the sidecar | `requires_revision` false, score 100, no findings, no functional warnings. The reported outline (76.9 x 35.6 mm) does not match the exported board, so ignore that figure, not the verdict |
| A15 | Power-good reaches the connector | `kicad-cli sch export netlist` (A8), `E-SYNTH-POWER-011` rule set | Net `PG` has exactly three nodes: `U1.pg` (pin 4, open drain), `R_PG.p1` (100k to `+3V3`), and `J2.p3`. The connection and the pull-up are proven; the behaviour is B3 |
| A16 | Thermal copper exists under the converter | `kicad/buck.kicad_pcb`, `reports/buck.connectivity.json` | U1 pin 9 is a 2.41 x 3.1 mm exposed pad in the `GND` net, soldered to the F.Cu pour; the board has 17 GND vias, 2 within 4 mm of the pad edge. The copper is proven; the temperature is B2 |

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | Survives a rated surge or a sustained 28 to 32 V rail | The converter is rated 3.8-36 V recommended and 38 V absolute maximum, so a sustained 28-32 V rail is inside its rating. A surge is not covered: SMBJ30A clamps at 48.4 V (12.4 A), above the 38 V absolute maximum, so the TVS is an ESD/transient clamp and not an OVP. A rated surge needs a 60 V front end or a series surge stopper, not designed here. Datasheet numbers only | Sustained rail in spec; surge unprotected |
| B2 | Thermal copper, thermal vias, thermal relief on pours and junction temperature | `synth capability list` declares `thermal_relief_analysis` unsupported: zone relief gap and spoke width are fixed export defaults and are not analysed. There are no vias in the pad, because the board's fabrication floor is a 0.3 mm drill and the KiCad HSOP-8 thermal-via footprint ships 0.2 mm drills. Estimate from TI Figure 9-3 (about 38 C/W at 20-24 cm2 of copper per layer on a four-layer board) and the assumed 85% efficiency: junction about 92 C at 1 A and 70 C ambient, about 114 C at 2 A. Not a simulation, not a measurement | Copper present; temperature unverified |
| B3 | Power-good behaviour | The connection is proven (A15) and the part's datasheet gives 93/95/105/107% thresholds, a glitch filter and a 4 ms start-up delay, but nothing declares those limits and `synth capability list` marks `power_sequencing_and_power_good` unsupported, so no stage checks them. Nothing was measured | Unsupported by Synth; unverified by an expert |
| B4 | Input and output filtering, loop stability, capacitor derating | Table 9-2 values are used (10 uF + 220 nF in, 3x 22 uF out, 6.8 uH, internal compensation). DC-bias derating of the input and output ceramics is unknown (no vendor data). C1 electrolytic ESR unknown. Crossover, phase margin and load-transient response were not computed or measured | Requires expert review |
| B5 | Switch-node loop area, return paths, EMI and ripple | Only pad-edge gaps were measured (README). No loop area, inductance, ripple or EMI result. The input cap C3 is 2.88 mm from the VIN pad and L1 3.80 mm from the SW pad; whether that loop is small enough is not analysed | Requires expert review |
| B6 | Analog placement separation and grounding | `synth capability list` declares `split_ground_planes` unsupported: each ground net gets one unsplit zone. The FB divider is 3.55 mm from U1 pin 5, but R_FB_B's ground is 11.31 mm from U1 pin 1 and reaches it through a via to the plane. Whether that is acceptable is for a reviewer | Requires expert review |
| B7 | Reverse-polarity block, PTC margin, UVLO thresholds, output voltage | All by hand calculation from datasheet numbers (README): PTC 0.162 A at 24 V and 0.248 A at the UVLO stop voltage against Ihold 0.63 A at 70 C; UVLO 18.09 / 16.62 V; Vout 3.315 V. No bench measurement | Requires expert review |
| B8 | Part pins and footprints against the manufacturer drawings | `synth registry qualify --part <id>` (output in `reports/registry-qualify.txt`): all 8 new parts blocked by `E-SYNTH-QUAL-008` — 7 for having no reviewer, the electrolytic for carrying no provenance — and `mf_msmf110_33x` additionally reports `E-SYNTH-QUAL-016` (its footprint names a 3D model that is not installed). No person sourced or compared the footprint: the stock KiCad HSOP-8 DDA footprint was not checked against TI's DDA drawing, nor the JST footprints against JST's. LED datasheet unreachable; the electrolytic and the generic passives have no MPN | Requires expert review |
| B9 | Compactness | Outline 71.9 x 31.125 mm (2238 mm2), up from 58.0 x 32.6 mm (1891 mm2) for the SOT-23-6 version: 5 more parts, 2 mounting holes and the HSOP-8 package. No auto-placed run was compared against this sidecar | Not a defect; not compared |
| B10 | Keepouts and shielding | The language has keepouts; none was declared under the switch node or the inductor in this design, and none was checked | Unsupported here |
| B11 | Power sequencing, soft-start and inrush | The converter has reference-based internal soft start (no SS pin) and a 4 ms power-good delay. Inrush into C1 and the output capacitors, and the sequencing of the PG signal against the output, were not analysed or measured | Unverified |
| B12 | Decoupling placement | C4 0.62 mm from BOOT, C8 0.87 mm from VCC, C7 0.55 mm from R_FB_T, C3 2.88 mm from VIN, C5/C6/C9 1.20 to 1.73 mm from L1 pad 2, in the pad-gap table in `README.md` (`pad_gaps.py`). Loop inductance was not computed | Unverified |
| B13 | Stackup and impedance assumptions | The board uses the default 4-layer stackup with inner ground planes. No stackup block was declared, no controlled-impedance net exists, and the capability descriptor marks inner-layer impedance unsupported | Not applicable to a single-ended power board |
| B14 | What the Gerbers, drill files and BOM say about orderability | The files in `kicad/` were generated and hashed (A10), but nothing was fabricated, no fab accepted them, no part carries an LCSC number, and the electrolytic and every generic passive have no MPN either. Availability not checked | Not done |
| B15 | Schematic correctness and readability | ERC has 0 errors (A7) and the netlist matches the source (A8). The sheet was rendered and read: `tools/schematic_quality.py` reports 1 overlapping text pair, 7 texts with a wire through them, 4 wire crossings, 928.4 mm of wire, and a mean passive-to-pin distance of 79.1 mm (max 155.9). The power-good and thermal groups read correctly | Readable, not tidy |
| B16 | The other negative cases from #38 (missing pair constraints, invalid stackup assumptions) | This board has no differential pair and no stackup claim, so those cases do not apply here; the USB board (#103) carries them. Only the unsafe-feedback case is built here, and the rule follows one series resistor only | Not applicable here |
| B17 | Comparison with a human reference design | Only TI's copper-area curve and efficiency curves were used. No reference layout was compared (#68) | Not done |
| B18 | Function on real hardware | Nothing was powered, programmed or measured. Two M2.5 mounting holes exist, so the board can be fixed down | Not done |
| B19 | Independent mixed-signal / RF sign-off (#38) | No reviewer | Not done |

Rows A2, A9 and B1 to B8 are the ones that decide whether this board meets a Class B bar (#38
treats missing SI/PI/RF evidence as a failure). They are open or bounded, not closed.

## C. #38 traceability for case 1

Every line of `#38`'s "Check exactly" list, against this board.

| `#38` item | Verdict here | Where |
| --- | --- | --- |
| Differential-pair membership and geometry, length constraints | Not applicable: no differential pair on this board | B16 |
| Return paths and layer transitions | One advisory proximity finding; no return-current analysis, and the capability descriptor says so | A9 |
| Impedance assumptions | Not applicable: no controlled-impedance net, no stackup block | B13 |
| Keepouts | Unsupported here: the language has them, none was declared or checked | B10 |
| Analog placement separation | FB divider 3.55 mm from the FB pin; divider ground 11.31 mm out through a via; not judged | B6 |
| Grounding | One unsplit GND zone per layer; split planes unsupported; see the layer-change finding | B6, A9 |
| Decoupling | Boot and decoupling capacitors 0.55 to 2.88 mm from the pin they serve; loop inductance not computed | B12 |
| Thermal relief | Fixed export defaults, not analysed; the pad's copper is proven, the temperature is not | A16, B2 |
| Connector ESD paths | D2 SMBJ30A on VIN is the ESD/transient clamp; J1 and J2 are 2- and 4-pin power connectors with no signal pins | B1, README |
| Power sequencing | The power-good signal exists and is pulled up (A15); its thresholds, delay and fault behaviour are not checked or measured | A15, B3, B11 |
| ERC and DRC | 0 errors on both, 30 + 68 warnings (library links and 5 silkscreen) | A4, A7 |
| Schematic correctness | Netlist equals the source net for net; readability measured, not tidy | A8, B15 |
| BOM correctness | 25 components, one BOM row each, no MPN or LCSC on the generic parts | A6, B14 |
| Footprint correctness | Pins and packages not compared with the manufacturer drawings; none of the 8 new parts has a reviewer | B8 |
| Manufacturing exports | Gerbers, drill, BOM and pick-and-place generated and hashed; not fabricated | A10, B14 |
| KiCad reopenability | Loaded headlessly by KiCad 10.0.7 for every export used here; not opened in the GUI | A11 |
| Which claims Synth proves | Section A, and only section A | A1-A16 |
| Which claims need SI/PI/RF expert analysis | Section B | B1-B19 |
| Comparison against a human reference | Only TI's curves; no layout comparison | B17 |

All four features #38's first case names are now carried by the board: input and output filtering
(A3, B4, B12), feedback (A2, A8), power-good (A15, B3) and thermal copper (A16, B2). The last two
are present and connected but unverified, where the previous revision declared them unsupported.

## D. The row this board declares

`qualification/matrix.toml` (the corpus #38 is measured on) is not on `main` yet, so this board
cannot be added to it in this branch. When it lands, this is the row, with the values measured
above and no value guessed:

```toml
[[board]]
id = "buck_24v_3v3"
path = "qualification/class-b/buck/buck.synth"
class = "B"
status = "experimental"
rationale = "Class B case 1: a 24 V to 3.3 V LMR33640 buck with input protection, UVLO, power-good \
on J2 pin 3, an exposed thermal pad and a switch-node-feedback negative twin. The board is its own \
case: the feedback-safety rule, the power-good net, the unreviewed-part fab block and the \
layer-change finding are measured on this source, not carried over from another board."
```

`expect` and `expect_fab` are deliberately missing. `synth check` in this revision always
selects KiCadRoutingTools and has no flag to select FreeRouting, and no KiCadRoutingTools
checkout was installed here, so the `synth check` verdict for this board was not measured. The
measured facts that would feed it are `validate` exit 0 (A1), `routing.state` routed (A3) and
`kicad-cli pcb drc` 0 errors (A4) for source/DRC, and `release_ready` false with
`production_status` untrusted (A6) for manufacturing.
