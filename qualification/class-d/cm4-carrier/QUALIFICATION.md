# Qualification matrix: CM4 industrial carrier (SYN-407)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated, powered or measured on hardware. Section A lists
what Synth or KiCad checked and the evidence. Section B lists what no tool here
proves and a human has to sign off. No human has reviewed this board.
`qualification.json` keeps `qualified: false`.

This is Class D board 1 of 4 under #41. It carries the CM4 carrier case: a
200-pin module interface, a multi-rail power chain, a USB hub tree, GbE
magnetics, HDMI, microSD, two 22-pin FFCs, a 40-pin GPIO header and an RTC.

## Reproduce

Run from the repository root with the Synth code of this branch. Use an empty
`XDG_DATA_HOME` so a user registry cannot shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
B=qualification/class-d/cm4-carrier
synth validate $B/cm4_carrier.synth
SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad $B/cm4_carrier.synth --out out/cm4 \
  --allow-incomplete --router free-routing
kicad-cli sch erc --severity-all --format json -o $B/reports/cm4_carrier.kicad-erc.json $B/kicad/cm4_carrier.kicad_sch
kicad-cli pcb drc --severity-all --refill-zones --format json -o $B/reports/cm4_carrier.kicad-drc.json $B/kicad/cm4_carrier.kicad_pcb
synth check $B/cm4_carrier.synth --json
synth check $B/cm4_carrier.synth --fab --json
cargo test -p synth-cli --test cm4_carrier_board
```

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses and lowers | `synth dump-ir $B/cm4_carrier.synth` | Exit 0; 149 nets, 179 components |
| A2 | The netlist has no conflicting net names or undefined pins | `synth dump-ir` | Exit 0 |
| A3 | Synth ERC has zero blocking diagnostics | `synth export-kicad ... --allow-incomplete` (ERC runs before export) | **0 errors**, 100+ warnings (see A5) |
| A4 | The CM4 pin map matches datasheet Table 6 | `cargo test -p synth-cli --test cm4_carrier_board` | 3 tests pass (pin map, GPIO header, mounting holes) |
| A5 | Warning list | export stderr | `W-SYNTH-PART-UNVERIFIED` for every authored part; `E-SYNTH-CONNECT-008` (2 floating inputs); `E-SYNTH-CONNECT-009` (4 open-drain LED nets); `E-SYNTH-NAME-008` (2 single-use labels); `E-SYNTH-POWER-004` (5 residual-energy warnings); `E-SYNTH-IMPEDANCE-001`/`W-SYNTH-IMPEDANCE-001` (geometry/inner-layer advisories); `E-SYNTH-SCHEM-001/003` (schematic readability). None is blocking |
| A6 | `synth validate` completes | `synth validate $B/cm4_carrier.synth` | **Blocked**: the schematic layout does not finish within 10 minutes on this board. The same ERC runs clean inside `export-kicad` (A3). See `agent-run/VERIFICATION_REPORT.md` |
| A7 | `synth place` completes | `synth place $B/cm4_carrier.synth` | **Blocked**: exits 1 with no placement (the automatic placer cannot place 185 components with ten edge-hard connectors). The committed placement sidecar pins every component |
| A8 | Export completes | `synth export-kicad ... --allow-incomplete --router free-routing` | **Blocked/timeout**: the schematic layout for 185 components and 158 signal nets runs for more than 30 minutes at 97 % CPU. No `kicad/` artifacts are committed |
| A9 | ERC on the exported schematic | `kicad-cli sch erc` | Not run: no schematic was exported |
| A10 | DRC on the exported PCB | `kicad-cli pcb drc` | Not run: no PCB was exported |
| A11 | `synth check` | `synth check --json` | **Fail**: `source: fail` because the `validate` child times out at 120 s; `drc: unknown` |
| A12 | `synth check --fab` | `synth check --fab --json` | **Fail**: `source: fail`, `drc: unknown`, `manufacturing: unknown` |
| A13 | Determinism | two exports of the same inputs | Not run: no export completed |

## B. Not checked by any tool here

These stay `passed: false` in `qualification.json` and need a named human
reviewer. No reviewer is invented.

| Gate | Why it is open |
| --- | --- |
| Power-up sequencing and power-good | Synth has no sequencing rule. The PG_5V -> 3V3 -> PG_3V3 -> 1V8 chain and the CM4 GLOBAL_EN/RUN_PG/nEXTRST timing are designed from datasheets, not simulated or measured |
| Signal integrity / PI | USB 2.0, HDMI TMDS, GbE, CSI/DSI and the 200-pin DF40 are high-speed interfaces. The diff pairs are declared, but the router does not guarantee coupled geometry, length matching or reference-plane continuity. Synth's impedance check is a closed-form estimate |
| EMC / ESD / surge | No emissions, immunity, ESD-gun or surge test. The TVS and ferrite bead are datasheet choices |
| Thermal | LMR33640 at 4 A and the LMR38010s at 1 A have exposed pads and via-in-pad, but no thermal simulation or measurement. The 4 A rating is not a measured board limit |
| Component and manufacturer review | Every authored part has `reviewed_by = ""`. The existing reviewed parts are reused as-is; the CM4, USB hub, magjack, microSD, FFC, coin cell, RTC, crystals, P-MOSFET and PTC are unreviewed |
| Fabrication review | `manufacturer "jlc-filled-via"` and `requires via_in_pad`; JLCPCB has not confirmed the via-in-pad fill/cap for this stackup. The DF40 0.4 mm pitch and the 100-pin connector escape were not DFM-reviewed |
| CM4 module mechanical fit | The 1.5 mm mated height and the module outline are from the Hirose/CM4 datasheets, not a mechanical fit check |
| Firmware / enumeration | No CM4, USB hub, card, camera, display or battery was fitted |

## C. #41 traceability

| #41 item | Where |
| --- | --- |
| Class D board 1, multi-rail, high pin-count | this directory |
| Board spec, blocks and nets | `cm4_carrier.synth`, `agent-run/NETLIST.md` |
| 200-pin module interface | `J_CM4A`/`J_CM4B`, `agent-run/PIN_MAPPING.md` |
| Power chain and sequencing | `README.md` "Power tree" and "Sequencing"; `agent-run/POWER_BUDGET.md` |
| USB hub tree | `U4` USB2514B, `U5`–`U8` TPS22918, `J_USB1`/`J_USB2` |
| GbE magnetics | `J_ETH` Pulse JXD6-0001NL |
| HDMI | `J_HDMI`, `U9` TXS0108E, `U10` TPS22918 |
| microSD | `J_SD` Molex 47219-2001, `U11` TPS22918 |
| 2x CSI/DSI FFC | `J_CAM`, `J_DSI` FH12-22S |
| 40-pin GPIO | `J_GPIO1`, `J_GPIO2` |
| RTC with coin cell | `U12` PCF85063ATL, `J_BT1` Keystone 1058 |
| Agent-run record | `agent-run/` |
| Matrix row | `qualification/matrix.toml` |

## D. Acceptance checklist

| Item | State | Evidence |
| --- | --- | --- |
| `synth check` passes | **blocked** | A11: `synth validate` does not finish; `check` reports `source: fail`, `drc: unknown` |
| `--fab` recorded honestly | **blocked** | A12: same upstream block; no fab package exists |
| ERC zero errors with `kicad-cli` | **blocked** | A9: no schematic exported. Synth ERC is zero errors (A3) |
| DRC report committed | **blocked** | A10: no PCB exported |
| Every used CM4 pin traced to a datasheet row | **done** | `agent-run/PIN_MAPPING.md` (143 used pins, Table 6 page per pin); test A4 |
| Rail table with source, load, limit, margin | **done** | `agent-run/POWER_BUDGET.md`; E-SYNTH-POWER-009/010 clean (A3) |
| README "Not verified by Synth" | **done** | `README.md` |
| Rerun twice -> identical hashes | **not done** | A13: no export completed |
| `qualified: false`; human gates present and false | **done** | `qualification.json` |

## E. CM4 pin traceability

`agent-run/PIN_MAPPING.md` lists every used CM4 pin, its connector pad, the
CM4 signal name from Table 6 and the net. The 57 unused pins are listed in the
`CM4 MODULE` group notes and in that file.

The pin-map test checks a representative subset: +5V pins 77/79/81/83/85/87,
GND pins on both connectors, Ethernet pairs 0 and 1, USB 105/103, HDMI0 TMDS2
170/172, SD_CLK/CMD/DAT0, the 40-pin header power and ground pins, GPIO2,
GPIO21 and GPIO26 (RTC INT).

## F. Rail summary

See `agent-run/POWER_BUDGET.md` for the consumer-by-consumer table. The
short version:

| Rail | Source | Declared load | Limit | Margin |
| --- | --- | --- | --- | --- |
| +5V | LMR33640, 4 A | CM4 1.4 A + 4 USB ports 2.0 A + hub 0.1 A + HDMI 0.04 A + fan 0.05 A = 3.59 A | 4.0 A | 0.41 A (10 %) |
| +5V_AUX | L_FB1 from +5V | 3V3 buck 0.27 A + 1V8 buck ~0 A = 0.27 A | bead not rated in registry | not verified |
| +3V3 | LMR38010, 1 A | hub 0.1 A + SD 0.1 A + RTC 0.001 A + LEDs 0.02 A + header 0.05 A = 0.27 A | 1.0 A | 0.73 A |
| +1V8 | LMR38010, 1 A | J_VREF only, ~0 A | 1.0 A | 1.0 A |

The +5V figure uses the CM4's **typical** 1.4 A operating current, not a
datasheet maximum (the datasheet gives 1.4 A typical and no maximum). The four
USB ports are counted at the USB 2.0 500 mA limit each. If the CM4 draws more
than 1.4 A, the 4 A rail loses margin; this is a design risk, not a measured
result.
