# VERIFICATION_REPORT — CM4 carrier (SYN-407)

Run on 2026-11-03 in `/home/steve/work/synth-407` at upstream `a437fac`.
KiCad 10.0.7. The Synth MCP server could not load the registry (see
`tool-calls.md`), so the CLI was used; it runs the same compiler, ERC, layout
and export code.

## Verdicts

| Gate | Command | Verdict |
| --- | --- | --- |
| Netlist lowers | `synth dump-ir qualification/class-d/cm4-carrier/cm4_carrier.synth` | **PASS** — exit 0, 149 nets, 179 components, no `E-SYNTH-NAME-005` or undefined-pin diagnostics |
| Synth ERC (blocking) | `synth export-kicad ... --allow-incomplete --router free-routing` | **PASS** — 0 errors; 100+ warnings. The ERC runs before the export and the export proceeded to layout |
| `synth validate` | `synth validate qualification/class-d/cm4-carrier/cm4_carrier.synth` | **FAIL (timeout)** — exit 124 at 120 s, 300 s and 600 s. The schematic layout does not finish |
| `synth place` | `synth place qualification/class-d/cm4-carrier/cm4_carrier.synth` | **FAIL** — timeout at 120 s; a 900 s run exits 1 with a null placement |
| Schematic export | `synth export-kicad ... --out /tmp/opencode/cm4/export13 --allow-incomplete --router free-routing` | **PARTIAL** — exit 124 at 1800 s. The four-sheet schematic, `.kicad_sym`, `.kicad_pro` and `.kicad_dru` were written at about 29 minutes; the PCB was not written |
| KiCad ERC | `kicad-cli sch erc --severity-all --format json -o reports/cm4_carrier.kicad-erc.json kicad/cm4_carrier.kicad_sch` | **FAIL** — 93 violations: 13 errors, 80 warnings |
| PCB export | same export | **NOT RUN** — killed before the PCB |
| KiCad DRC | `kicad-cli pcb drc ...` | **NOT RUN** — no PCB |
| `synth check` | `synth check qualification/class-d/cm4-carrier/cm4_carrier.synth --json` | **FAIL** — `drc: unknown`; the DRC stage calls `synth validate`, which times out |
| `synth check --fab` | `synth check qualification/class-d/cm4-carrier/cm4_carrier.synth --fab --json` | **FAIL** — same upstream block; no manufacturing stage |
| Board test | `cargo test -p synth-cli --test cm4_carrier_board` | **PARTIAL** — the two IR tests pass; the mounting-hole test exports the board with the stub router and did not finish in the test budget |

## Synth ERC warning counts

From the export stderr (the exact count varies with the part list; the last
run had 100+):

| Code | Count | Meaning |
| --- | --- | --- |
| `W-SYNTH-PART-UNVERIFIED` | 44 | every authored part, `reviewed_by = ""` |
| `E-SYNTH-SCHEM-003` | 6 | decoupling capacitor separation on the schematic |
| `E-SYNTH-SCHEM-001` | 2 | inverted power symbol |
| `E-SYNTH-IMPEDANCE-001` / `W-SYNTH-IMPEDANCE-001` | 19 | diff-pair geometry/inner-layer advisory |
| `E-SYNTH-CONNECT-008` | 2 | floating digital input (U1, U4.test) |
| `E-SYNTH-CONNECT-009` | 4 | open-drain LED nets without a pull-up (the LED + resistor is the pull-up) |
| `E-SYNTH-POWER-004` | 5 | power-net residual energy |
| `E-SYNTH-NAME-008` | 0–2 | single-use labels |
| `W-SYNTH-SUPPLY-002` | 20 | test points, headers, mounting holes without MPN |
| `W-SYNTH-ANOMALY-001` | 1 | unusual design structure |

None of these is blocking in Synth. The KiCad ERC errors below are real
pin-type/unconnected findings.

## KiCad ERC errors (13)

1. **5 × `pin_to_pin`: TPS22918 QOD (open collector) connected to VOUT
   (power output).** U5, U6, U7, U8, U10, U11. The QOD pin is a discharge
   FET; the KiCad symbol types it open collector. The final source removes
   the QOD-to-VOUT tie (QOD left floating, as the datasheet allows) and the
   re-export is expected to clear these.
2. **1 × `pin_to_pin`: U4 XTALOUT (output) and `#FLG2712` (power output).**
   The PWR_FLAG is in the Power sheet; the ERC associates it with the
   `HUB_XTAL2` global label. This looks like a cross-sheet association, not
   a short. Not fixed.
3. **2 × `pin_not_connected` + 2 × `power_pin_not_driven`: J_CAM/J_DSI
   mounting pad `MP`.** The FFC part's MP pad was `ground_reference` but not
   in a net. The final source ties `J_CAM.mp` and `J_DSI.mp` to GND; the
   re-export is expected to clear these.
4. **1 × `pin_not_driven`: Q_LED gate.** The CM4 `PI_LED_nPWR` pin was typed
   `open_drain_low`; the 2N7000 gate is an input. The final source types the
   CM4 pin `output`; the re-export is expected to clear this.
5. **1 × `power_pin_not_driven`: `#PWR2153` input power pin.** A power
   symbol on a rail the exporter did not flag. Not identified further; the
   re-export may move it.

The 78 `lib_symbol_issues` warnings are "the current configuration does not
include the symbol library 'synth'": the exported schematic embeds its
symbols, so this is a library-table warning, not a design fault. The
`Si2301CDS` warning is the P-FET symbol standing in for the DMP3098L.

## What was not run

- No routed PCB, no `kicad-cli pcb drc`, no Gerber, drill, BOM, pick-and-place
  or 3D render.
- No `synth check` pass; the command's 30 s DRC budget is far below the
  board's 30-minute schematic layout.
- No two-run determinism check.
- No hardware, no firmware, no measurement.

## Reproduce the block

```
B=qualification/class-d/cm4-carrier
timeout 600 synth validate $B/cm4_carrier.synth          # exit 124
timeout 900 synth place $B/cm4_carrier.synth             # exit 1, null
timeout 1800 synth export-kicad $B/cm4_carrier.synth \
  --out /tmp/cm4 --allow-incomplete --router free-routing # exit 124
kicad-cli sch erc --severity-all --format json \
  -o $B/reports/cm4_carrier.kicad-erc.json \
  $B/kicad/cm4_carrier.kicad_sch                          # 13 errors
```

The block is in Synth's schematic layout, not in the netlist: `dump-ir` and
the ERC complete, and the same 185 components with only the GND net lay out
in seconds. Adding signal nets grows the layout time super-linearly; 158
signal nets do not finish in 30 minutes.
