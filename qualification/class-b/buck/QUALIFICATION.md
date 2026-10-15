# Qualification matrix: 24 V to 3.3 V buck board (SYN-67)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the evidence. Section B lists what no tool here proves and an SI/PI/RF or power expert has to sign off. No expert has reviewed this board.

The board routes with 0 open nets and passes Synth's release gate (`release_ready: true` in `reports/release.json`, see A5). A passed gate means the tool's checks passed; it is not a qualification.

## Reproduce

Run from the repository root with Synth `main` at `a2a4fa5` or later (it includes the filled-pour delivery, #114, and the label placement, #144). Use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh --freerouting-only
B=$(pwd)/qualification/class-b/buck
synth validate $B/buck.synth
synth validate $B/buck_neg_fb_sw.synth        # must exit 1 with E-SYNTH-POWER-011
synth export-kicad $B/buck.synth --out <dir> --allow-unverified-parts --validate-erc --router free-routing
kicad-cli pcb drc --severity-all --refill-zones --format json -o <dir>/drc.json <dir>/buck.kicad_pcb
kicad-cli sch erc --severity-all --format json -o <dir>/erc.json <dir>/buck.kicad_sch
(cd $B && sha256sum -c SHA256SUMS)
```

Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.4.1, Synth 0.1.1 built from `main` a2a4fa5 plus this branch (release build), on 2026-10-14. Two exports gave the same file hashes. A different placement or router version can give different copper.

## Artifact hashes

`SHA256SUMS` in this folder lists every design artifact (source, sidecar, KiCad files, BOM, pick-and-place, pictures, routing, DRC and ERC reports). It leaves out `README.md`, `QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `buck.synth` | `f723600457999808dee340bcdc7b75850056e3df1d4cefd5c2686faf1cdf5796` |
| `buck.placement.layout.toml` | `26f162ecff18fd997af69431fc21b7c9a42fb53876860d2e137242c1c6710d64` |
| `buck_neg_fb_sw.synth` | `928ba7985c9722f259d56a6a2c491b5c4aad4dece47b0402bf1c79b408c60c06` |
| `kicad/buck.kicad_pcb` | `b0aaaa111a15b60020b28ac8cf2cc077bb83dbb62d7058657c5872f6aa779bbb` |
| `kicad/buck.kicad_sch` | `47b7451740a8ebb606ed427d4abac50329bff12a97c7c0078efa7dc5b1d6156c` |
| `kicad/bom.csv` | `6bcdcd49cdd480d13f13a3ca4052ab3149f22614b13f9f347a6a9443c0b9e50d` |
| `reports/release.json` | `34aaa8c9fb737dac9eedfd1e2689f6f9c8a749348d0d54ae1954240bc24a00ea` |
| `reports/buck.kicad-drc.json` | `bad2a05aa9a3a61b9aaca18aea569b1fe2214653427441b2ba45a361de00c77c` |

`reports/` holds the files as captured, so they contain the absolute paths of the machine that produced them, and `engine_version` in `release.json` is a jar path.

Datasheets read for the part values are listed in `README.md` ("Parts and datasheets"). They are not committed, so the figures taken from them cannot be checked from this repository.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate buck.synth` | Exit 0, 0 errors, 14 warnings (8 part-unverified, 3 E-SYNTH-SCHEM-003, 1 anomaly, 1 supply, 1 power symbol) |
| A2 | Regulator feedback is not taken from the switch node | `E-SYNTH-POWER-011` on `buck_neg_fb_sw.synth` | The twin exits 1 with the error; the good board does not raise it. The rule follows one resistor only, so a longer path is not checked |
| A3 | Every net is connected end to end | `reports/buck.connectivity.json` | 10 nets connected, 0 open, 0 unconnected pads, no via-in-pad |
| A4 | No KiCad DRC rule violation on the routed board | `kicad-cli pcb drc --refill-zones`, `reports/buck.kicad-drc.json` | 0 errors, 0 unconnected, 20 warnings, all `lib_footprint_issues` (library links). No silkscreen warning |
| A5 | The package passes Synth's release gate | `synth export-kicad`, `reports/release.json` | **Yes**: `release_ready` true, state `routed`, 52 segments, 5 vias, 0 blocking signal-integrity findings. A passed gate is not a qualification |
| A6 | Schematic electrical rules | `kicad-cli sch erc`, `reports/buck.kicad-erc.json` | 0 errors, 54 warnings (34 symbol-library, 20 footprint-link: the KiCad libraries are not in the config) |
| A7 | Export is reproducible | two `synth export-kicad` runs, sha256, run by hand on 2026-10-14 (no log stored) | `.kicad_pcb` and `.kicad_sch` identical. Re-run the commands above to check |
| A8 | Ground pours are stored filled | `grep -c filled_polygon kicad/buck.kicad_pcb` | 4 filled polygons are stored in the file (4 GND zones). Not checked by opening it in the KiCad GUI |
| A9 | A layer change without a nearby ground return via is reported | `W-SYNTH-DRC-016` in `reports/release.json` | One finding: net 3 changes layers at (35.97, 14.43) mm, no ground via within 0.64 mm. Advisory, not blocking. The via is on **+3V3** (the sense and output track; the report's "net 3" is Synth's own numbering, while in the `.kicad_pcb` net 3 is FB). Whether the missing return via matters is for the reviewer |

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | Survives a rated surge or a sustained 28 to 32 V rail | TPS54202 absolute maximum VIN is 30 V; SMBJ26A VBR is 28.9 to 31.9 V and clamps at 42.1 V (14.3 A). The TVS clamps surge energy only. A 24 V industrial rail needs a series OVP stage or a 40 to 60 V buck. Datasheet numbers only | Requires expert review |
| B2 | Thermal copper, thermal vias, thermal relief on pours and junction temperature | Synth has no thermal-pour or via model. Estimated junction about 133 C at 70 C ambient (118.6 C/W) at 1 A, above the 125 C limit; about 100 C with the EVM figure 57.2 C/W. The 85% efficiency is an assumption. Pad thermal relief was not examined | Unverified (needs expert) |
| B3 | Power-good | The TPS54202 has no PG pin. Synth has no power-good statement | Unsupported |
| B4 | Input and output filtering, loop stability, capacitor derating | Table 7-2 asks for 44 uF at 3.3 V; crossover 27.3 kHz at nominal value. DC-bias derating of the 10 uF input and 2 x 22 uF output ceramics is unknown (no vendor data). C1 electrolytic ESR unknown | Requires expert review |
| B5 | Switch-node loop area, return paths, EMI and ripple | Only pad-edge gaps were measured (README). No loop area, inductance, ripple or EMI result. FreeRouting ran with defaults, no length or coupling constraints. The +3V3 sense track runs on B.Cu under the buck core | Requires expert review |
| B6 | Analog placement separation and grounding | The FB divider ground is 4.25 mm from U1 pin 1 and reaches it through a via to the plane. Whether that is acceptable is for a reviewer | Requires expert review |
| B7 | Reverse-polarity block, PTC margin, UVLO thresholds, output voltage | All by hand calculation from datasheet numbers (README): PTC 0.248 A vs Ihold 0.63 A at 70 C with the assumed efficiency; UVLO 18.31 / 15.67 V; Vout 3.293 V. No bench measurement | Requires expert review |
| B8 | Part pins and footprints against the manufacturer drawings | `synth registry qualify --part <id>` (output in `reports/registry-qualify.txt`): `E-SYNTH-QUAL-008` (no reviewer; no provenance for the electrolytic) blocks every new part and nothing else is listed. The stock KiCad `SOT-23-6` footprint was not compared with the TI DDC drawing. LED datasheet unreachable; connectors and generic passives have no MPN | Requires expert review |
| B9 | Compactness | Outline 63.0 x 37.6 mm (2369 mm2). A run with the auto placer and no sidecar gave 48.0 x 50.9 mm (2445 mm2), routed, DRC 0 / 0 / 20 (measured once by hand, not stored). The hand layout was kept because the areas are within 3% | Not a defect; no better layout found |
| B10 | Keepouts and shielding | The language has no keepout under the switch node or inductor in this design; none was declared or checked | Unsupported here |
| B11 | Power sequencing, soft-start and inrush | UVLO thresholds are calculated (B7). Soft-start time, inrush into C1 and the output capacitors, and PTC behaviour in a surge were not analysed | Unverified |
| B12 | Decoupling placement | C3 (100 nF) is 1.18 mm from the U1 VIN pad, C7 0.75 mm from R_FB_T, in the pad-gap table in `README.md` (`pad_gaps.py`). Loop inductance was not computed | Unverified |
| B13 | Stackup and impedance assumptions | The board uses the default 4-layer stackup with inner ground planes. No stackup block was declared, no controlled-impedance net exists, and Synth labels any impedance claim "not verified" without one | Unverified (not applicable to a single-ended power board) |
| B14 | Manufacturing exports (Gerber, drill) and BOM | `kicad/bom.csv` and `kicad/pnp.csv` are generated and hashed. Gerbers and drill files were not generated or checked for this board. No MPN or LCSC number on the generic passives, connectors and LED; availability not checked | Not done |
| B15 | Schematic correctness | `kicad-cli sch erc` has 0 errors (A6), and the schematic was rendered and read once: value text overlaps, the `BUCK` title crowds U1, and signal nets are drawn with pin-derived names (`P2` for +3V3) that differ from the PCB names. See README | Readable, not tidy |
| B16 | Other negative cases from #38 (missing pair constraints, invalid stackup assumptions) | This board has no differential pair and no stackup claim, so those cases do not apply here; the USB board (#103) carries them. Only the unsafe-feedback case is built here, and the rule follows one resistor only | Not applicable here |
| B17 | Comparison with a human reference design | Only the TI EVM thermal figure (57.2 C/W) was used. No reference layout was compared (#68) | Not done |
| B18 | Function on real hardware | Nothing was powered, programmed or measured | Not done |
| B19 | Independent mixed-signal / RF sign-off (#38) | No reviewer | Not done |

Rows A2, A9 and B1 to B8 are the ones that decide whether this board meets a Class B bar (issue #38 treats missing SI/PI/RF evidence as a failure). They are open.
