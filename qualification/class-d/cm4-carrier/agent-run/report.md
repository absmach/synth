# REPORT — CM4 carrier (SYN-407)

## Branch and commits

- Branch: `syn-407-cm4-carrier`, based on upstream `a437fac`.
- Commits: `9413c017` (board source, parts, tests, qualification and agent-run
  docs), `10fa9d47` (report commit line), `fe6fdbb9` (test formatting),
  `5242b037` (gate blocks and pinout gaps) and `3621bfc0` (GbE magjack and
  24 MHz crystal pad-map fixes).
- Draft PR: https://github.com/absmach/synth/pull/417 (base `main`, head
  `nyagamunene:syn-407-cm4-carrier`).

## What is in the tree

- `cm4_carrier.synth` (824 lines, 179 components, 149 nets).
- `cm4_carrier.placement.layout.toml` (185 components pinned).
- 11 authored Tier-2 registry parts plus the CM4 B variant.
- `crates/synth-cli/tests/cm4_carrier_board.rs` (2 pass, 1 ignored).
- `README.md`, `QUALIFICATION.md`, `qualification.json`, `BOM_sourcing.md`.
- `agent-run/` (this report, prompt, tool calls, design notes, pin map,
  netlist, power budget, assembly, bring-up, verification).
- `kicad/` — four-sheet schematic, embedded symbols, project, DRC rules, routed 6-layer HDI PCB, unrouted baseline, BOM and PnP.
- `reports/cm4_carrier.kicad-erc.json` — 1 error, 67 warnings; `reports/cm4_carrier.kicad-drc.json` — routed board, 0 violations / 0 unconnected.
- `images/schematic/*.svg` — the four schematic sheets.
- `qualification/matrix.toml` row, class "D", status "experimental".

## Acceptance checklist

| Item | State | Proof |
| --- | --- | --- |
| `synth check` passes | **blocked** | `synth check --json` -> `source: fail` (validate timeout), `drc: unknown`; `synth validate` exits 124 at 600 s |
| `--fab` recorded honestly | **blocked** | `synth check --fab --json` -> no manufacturing stage; the PCB is routed but no fab package exists |
| ERC zero errors with `kicad-cli` | **not met (1 error)** | 1 `RTC_BAT` power-flag error in `reports/cm4_carrier.kicad-erc.json`; Synth ERC is 0 errors |
| DRC report committed | **done (routed)** | `reports/cm4_carrier.kicad-drc.json`; 0 violations / 0 unconnected on the routed 6-layer HDI board |
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
kicad-cli sch erc ...                                  68 violations: 1 error, 67 warnings
synth check --json                                      fail, drc: unknown
synth check --fab --json                                fail, no manufacturing stage
cargo test -p synth-cli --test cm4_carrier_board        2 passed, 1 ignored
```

## Firecrawl datasheet follow-up

The user asked for the missing component datasheets through Firecrawl. Two
real design errors were found and fixed in the source:

- **Ethernet was 100BASE-TX, not GbE.** The Pulse JXD6-0001NL product page
  and datasheet say 100BASE-TX; it is not a GbE part. Replaced with the UDE
  RB1-125B8G1A (10/100/1000BASE-T, LCSC C363353, through-hole, stock KiCad
  symbol/footprint). The four CM4 pairs now go to TD1–TD4 and the single
  centre tap to GND through 100 nF.
- **24 MHz crystal pad map was wrong.** The ECS ECX-33Q datasheet says pads
  1/3 are the crystal and 2/4 are GND. The first revision had 1/2 crystal and
  3/4 GND, which would have put XTALOUT on a ground pad. Fixed.

The corrected-source export15 wrote the schematic at about 28 minutes and
ran out of its 45-minute budget before the PCB. The final export (after the
SYN-420 sheet-fill merge) re-drew the four sheets as A1/A2; `kicad/` and
`images/` are that final schematic. Its KiCad ERC has 1 error and 67
warnings; the earlier QOD/VOUT, FFC MP and Q_LED findings were schematic
layout defects and are gone.

## Gaps and assumptions

1. **Synth's schematic layout is slow (~28 minutes).** The SYN-420
   sheet-fill layout now produces four A1/A2 sheets at 91-93% fill; the
   `synth validate`/`synth check` budgets still time out.
2. **The automatic placer exits 1.** The placement sidecar pins every
   component.
3. **No Gerber, drill or fab package, and no two-run determinism check.**
   The routed PCB, DRC, BOM and PnP exist.
4. **KiCad ERC has 1 error, 67 warnings.** The error is the `RTC_BAT` power
   pin on the Peripherals sheet; the source netlist is correct.
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
