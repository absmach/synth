# REPORT — CM4 carrier (SYN-407)

## Branch and commits

- Branch: `syn-407-cm4-carrier`, based on upstream `a437fac`.
- Commits: `9413c017` (board source, parts, tests, qualification and agent-run docs) and the follow-up docs commit that adds this line.

## What is in the tree

- `cm4_carrier.synth` (824 lines, 179 components, 149 nets).
- `cm4_carrier.placement.layout.toml` (185 components pinned).
- 11 authored Tier-2 registry parts plus the CM4 B variant.
- `crates/synth-cli/tests/cm4_carrier_board.rs` (2 pass, 1 ignored).
- `README.md`, `QUALIFICATION.md`, `qualification.json`, `BOM_sourcing.md`.
- `agent-run/` (this report, prompt, tool calls, design notes, pin map,
  netlist, power budget, assembly, bring-up, verification).
- `kicad/` — four-sheet schematic, embedded symbols, project, DRC rules.
- `reports/cm4_carrier.kicad-erc.json` — 13 errors, 80 warnings.
- `images/schematic/*.svg` — the four schematic sheets.
- `qualification/matrix.toml` row, class "D", status "experimental".

## Acceptance checklist

| Item | State | Proof |
| --- | --- | --- |
| `synth check` passes | **blocked** | `synth check --json` -> `source: fail` (validate timeout), `drc: unknown`; `synth validate` exits 124 at 600 s |
| `--fab` recorded honestly | **blocked** | `synth check --fab --json` -> no manufacturing stage; no PCB |
| ERC zero errors with `kicad-cli` | **not met** | 13 errors in `reports/cm4_carrier.kicad-erc.json`; Synth ERC is 0 errors |
| DRC report committed | **blocked** | no PCB |
| Every used CM4 pin traced to a datasheet row | **done** | `agent-run/PIN_MAPPING.md`, 143 used pins, Table 6 page per pin; test passes |
| Rail table with source, load, limit, margin | **done** | `agent-run/POWER_BUDGET.md`; E-SYNTH-POWER-009/010 clean |
| README "Not verified by Synth" | **done** | `README.md` |
| Rerun twice -> identical hashes | **not done** | no export completed twice |
| `qualified: false`; human gates present and false | **done** | `qualification.json` |

## Gate verdict lines

```
synth dump-ir cm4_carrier.synth                         exit 0 (149 nets, 179 components)
synth export-kicad ... --allow-incomplete               ERC 0 errors, 100+ warnings
synth validate cm4_carrier.synth                        exit 124 (timeout, 600 s)
synth place cm4_carrier.synth                           exit 1 (null placement, 900 s)
synth export-kicad ... --router free-routing            exit 124 (schematic written, no PCB)
kicad-cli sch erc ...                                  93 violations: 13 errors, 80 warnings
synth check --json                                      fail, drc: unknown
synth check --fab --json                                fail, no manufacturing stage
cargo test -p synth-cli --test cm4_carrier_board        2 passed, 1 ignored
```

## Gaps and assumptions

1. **Synth cannot lay out this board's schematic in a useful time.** The
   netlist and ERC are fine; the schematic layout is the block. This is the
   main reason the board is not releasable. The same 185 components with only
   the GND net lay out in seconds; 158 signal nets do not finish in 30
   minutes.
2. **The automatic placer exits 1** with no placement. The sidecar pins every
   component, but the exporter still spends ~29 minutes on the schematic.
3. **No PCB, no routing, no DRC, no Gerber, no BOM.** The export was killed
   at the 30-minute budget after writing the schematic.
4. **KiCad ERC has 13 errors.** The clear ones (TPS22918 QOD/VOUT, FFC MP
   pads, Q_LED gate) are fixed in the source and a re-export is running; the
   result was not available at commit time. The U4 XTALOUT/PWR_FLAG and
   `#PWR2153` findings are not fully explained.
5. **The CM4 has no VBAT pin.** The issue's "diode to CM4 VBAT" is
   unimplementable; the board uses a standalone PCF85063ATL with a CR2032.
   This is a documented deviation, not a datasheet guess.
6. **The 4 A +5V rail is tight.** The true load is 3.77 A (0.23 A margin),
   and the 6.8 µH inductor is 3.6 A Irms. The CM4 1.4 A is typical, not a
   maximum. This is a design risk.
7. **The DF40 +5V contact rating is 0.3 A × 6 = 1.8 A.** The CM4's 1.4 A
   typical leaves 0.4 A; a 2 A CM4 load would exceed the connector rating.
8. **The HDMI DDC uses a TXS0108E**, a push-pull translator, on an open-drain
   bus. Pull-ups are fitted on both sides; this is a design risk.
9. **Every authored part is unreviewed** (`reviewed_by = ""`): CM4
   connectors, USB2514B, USB-A, magjack, microSD, FFC, coin cell, RTC,
   crystals, DMP3098L, PTC. The existing reviewed parts are reused.
10. **Datasheets not fully verified:** the Keystone 1058 and the 24 MHz ECS
    crystal have no LCSC number in this record; the ferrite bead's current
    rating is not in the registry; the USB2514B 3.3 V operating current is a
    placeholder; the fan current is assumed.
11. **No hardware was built or powered.** Sequencing, SI, EMC, thermal,
    USB/HDMI/Ethernet function, SD/FFC/RTC function and battery life are all
    unverified.
