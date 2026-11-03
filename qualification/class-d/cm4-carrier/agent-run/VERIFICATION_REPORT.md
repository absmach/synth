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
| Schematic export | `synth export-kicad ... --out /tmp/opencode/cm4/export15 --allow-incomplete --router free-routing` | **PARTIAL** — exit 124 at 2700 s. The corrected-source four-sheet schematic, `.kicad_sym`, `.kicad_pro`, `.kicad_dru` and `sym-lib-table` were written at about 28 minutes; the PCB was not written |
| KiCad ERC | `kicad-cli sch erc --severity-all --format json -o reports/cm4_carrier.kicad-erc.json kicad/cm4_carrier.kicad_sch` | **FAIL (1 error)** — 67 violations: 1 error, 66 warnings on the corrected-source schematic |
| PCB export (`--pcb-only`) | `synth export-kicad ... --pcb-only --allow-incomplete --router free-routing` | **PASS for the file**: the PCB, BOM and PnP are written in seconds from the placement sidecar; the router then runs and is still in flight |
| KiCad DRC | `kicad-cli pcb drc --severity-all --refill-zones ... kicad/cm4_carrier.kicad_pcb` | **Unrouted board**: 153 violations, 359 unconnected items. This is the expected result for an unrouted placement; it is not a routed-board DRC |
| `synth check` | `synth check qualification/class-d/cm4-carrier/cm4_carrier.synth --json` | **FAIL** — `drc: unknown`; the DRC stage calls `synth validate`, which times out |
| `synth check --fab` | `synth check qualification/class-d/cm4-carrier/cm4_carrier.synth --fab --json` | **FAIL** — same upstream block; no manufacturing stage |
| Board test | `cargo test -p synth-cli --test cm4_carrier_board` | **PARTIAL** — the two IR tests pass; the mounting-hole test exports the board with the stub router and did not finish in the test budget |

## `synth check` JSON

```
synth check ... --json
  status: fail
  stages: source=fail (validate child timed out, parse_error: true),
          drc=unknown
synth check ... --fab --json
  status: fail
  stages: source=fail, drc=unknown,
          manufacturing=unknown ("upstream source or DRC gate failed")
```

The `check` command's `validate` child has a 120 s budget; this board's
schematic layout is about 30 minutes, so the source stage is a timeout, not a
diagnostic failure.

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

## KiCad ERC errors (1)

The corrected-source schematic has **one** error:

- **`power_pin_not_driven` at `#PWR2153`.** The symbol is `RTC_BAT`, the
  coin-cell side of D_RTC2. The battery symbol's pin is passive, so the net
  has no power output. The final source adds `power "RTC_BAT" 3v`; the
  exporter then adds a PWR_FLAG. Export16 is the re-run.

The 13 errors in the first (pre-GbE, pre-crystal-fix) schematic were five
TPS22918 QOD/VOUT conflicts, one U4 XTALOUT/PWR_FLAG association, two FFC MP
pads, the Q_LED gate and the RTC_BAT flag. The QOD, FFC and Q_LED findings
were schematic-layout defects: the source netlist already had the right
connections. They are gone in the corrected schematic.

The 66 `lib_symbol_issues` warnings are mostly "the current configuration
does not include the symbol library 'synth'" / missing custom power symbols
(`+5V_AUX`, `RTC_VDD`, `CM4_3V3`, `CM4_1V8`, `USB_VBUS*`). The exported
schematic embeds its symbols; the custom power symbols are the exporter's
known gap for non-standard rail names. The `Si2301CDS` warning is the P-FET
symbol standing in for the DMP3098L.

## PCB-only export

The board's schematic layout is the slow stage (about 28–40 minutes). The
`--pcb-only` flag added in this branch writes the PCB side from the placement
sidecar directly, without the schematic layout:

```
synth export-kicad ... --out ... --pcb-only --allow-incomplete --router free-routing
```

It writes `cm4_carrier.kicad_pcb`, `cm4_carrier.synth.kicad_pcb`, `bom.csv`,
`pnp.csv`, the project and the DRC rules in seconds. The router then runs on
the unrouted PCB; the 3D and copper images in `images/` are from that
unrouted board. A real FreeRouting pass is running separately; its result is
not in this report yet.

The new CLI test `pcb_only_export_writes_the_pcb_without_a_schematic` passes
(it takes about 100 s with the stub router).

## What was not run

- No routed PCB, no routed-board DRC, no Gerber, drill or fab package.
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
  $B/kicad/cm4_carrier.kicad_sch                          # 1 error
```

The block is in Synth's schematic layout, not in the netlist: `dump-ir` and
the ERC complete, and the same 185 components with only the GND net lay out
in seconds. Adding signal nets grows the layout time super-linearly; 158
signal nets do not finish in 30 minutes.
