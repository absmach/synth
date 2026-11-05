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
| A3 | Synth ERC has zero blocking diagnostics | `synth export-kicad ... --allow-incomplete` (ERC runs before export) | **0 errors**, 100+ warnings (see A5), on the pre-GbE source. The GbE magjack and 24 MHz crystal pad-map fixes are in the final source; export15 is the re-run |
| A4 | The CM4 pin map matches datasheet Table 6 | `cargo test -p synth-cli --test cm4_carrier_board` | 3 tests pass (pin map, GPIO header, mounting holes) |
| A5 | Warning list | export stderr | `W-SYNTH-PART-UNVERIFIED` for every authored part; `E-SYNTH-CONNECT-008` (2 floating inputs); `E-SYNTH-CONNECT-009` (4 open-drain LED nets); `E-SYNTH-NAME-008` (2 single-use labels); `E-SYNTH-POWER-004` (5 residual-energy warnings); `E-SYNTH-IMPEDANCE-001`/`W-SYNTH-IMPEDANCE-001` (geometry/inner-layer advisories); `E-SYNTH-SCHEM-001/003/004/005` (schematic readability). None is blocking |
| A6 | `synth validate` completes | `synth validate $B/cm4_carrier.synth` | **Blocked**: the schematic layout does not finish within 10 minutes on this board. The same ERC runs clean inside `export-kicad` (A3). See `agent-run/VERIFICATION_REPORT.md` |
| A7 | `synth place` completes | `synth place $B/cm4_carrier.synth` | **Blocked**: exits 1 with no placement (the automatic placer cannot place 185 components with ten edge-hard connectors). The committed placement sidecar pins every component |
| A8 | Export completes | `synth export-kicad ... --pcb-only --allow-incomplete` | **Partial**: the full export still exceeds the 45-minute budget, but `--pcb-only` writes the PCB/BOM/PnP in seconds from the placement sidecar. `kicad/` has the four-sheet schematic, the routed `.kicad_pcb` and the unrouted baseline |
| A9 | ERC on the exported schematic | `kicad-cli sch erc` | **Fail (1 error, 67 warnings)** in `reports/cm4_carrier.kicad-erc.json`. The single error is the `RTC_BAT` power pin on the Peripherals sheet. The source adds `power "RTC_BAT" 3v`; export16 is the re-run |
| A10 | DRC on the exported PCB | `kicad-cli pcb drc` | **Routed**: 0 violations, 0 unconnected items on the 6-layer HDI `kicad/cm4_carrier.kicad_pcb`. The KRT route reached 359/360 pad pairs; a targeted `DSI_D1_N` re-route closed the last pair |
| A11 | `synth check` | `synth check --json` | **Fail**: `source: fail` because the `validate` child times out at 120 s; `drc: unknown` |
| A12 | `synth check --fab` | `synth check --fab --json` | **Fail**: `source: fail`, `drc: unknown`, `manufacturing: unknown`. A manual `fab/` package exists but is not release evidence |
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
| Fabrication review | `manufacturer "jlc-hdi"` (6-layer HDI: 0.1 mm drill, 0.075 mm ring, 0.25 mm via, 0.1 mm track/clearance) and `requires via_in_pad`; JLCPCB has not confirmed the HDI microvia/fill capability for this stackup. The DF40 0.4 mm pitch and the 100-pin connector escape were not DFM-reviewed. The 6-layer/HDI stackup is a deliberate deviation from #407's 4-layer `jlc-filled-via` constraint, approved by the user |
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
| GbE magnetics | `J_ETH` UDE RB1-125B8G1A (10/100/1000BASE-T, C363353) |
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
| `--fab` recorded honestly | **blocked** | A12: same upstream block; a manual `fab/` package exists but is not release evidence |
| ERC zero errors with `kicad-cli` | **not met (1 error)** | A9: the only error is `RTC_BAT`; export16 is the re-run with the power declaration |
| DRC report committed | **done (routed)** | A10: `reports/cm4_carrier.kicad-drc.json`, 0 violations / 0 unconnected on the routed 6-layer HDI board |
| Every used CM4 pin traced to a datasheet row | **done** | `agent-run/PIN_MAPPING.md` (143 used pins, Table 6 page per pin); test A4 |
| Rail table with source, load, limit, margin | **done** | `agent-run/POWER_BUDGET.md`; E-SYNTH-POWER-009/010 clean (A3) |
| README "Not verified by Synth" | **done** | `README.md` |
| Rerun twice -> identical hashes | **not done** | A13: no export completed |
| `qualified: false`; human gates present and false | **done** | `qualification.json` |

## E. CM4 pin traceability

`agent-run/PIN_MAPPING.md` lists every used CM4 pin, its connector pad, the
CM4 signal name from Table 6 and the net. The 57 unused pins are listed in the
`CM4 MODULE` group notes and in that file.

| Connector | Pad | CM4 pin | CM4 signal (Table 6) | Net | Table 6 page |
|---|---|---|---|---|---|
| J_CM4A | 1 | 1 | GND | `GND` | 16 |
| J_CM4A | 2 | 2 | GND | `GND` | 16 |
| J_CM4A | 3 | 3 | Ethernet_Pair3_P | `ETH3_P` | 16 |
| J_CM4A | 4 | 4 | Ethernet_Pair1_P | `ETH1_P` | 16 |
| J_CM4A | 5 | 5 | Ethernet_Pair3_N | `ETH3_N` | 16 |
| J_CM4A | 6 | 6 | Ethernet_Pair1_N | `ETH1_N` | 16 |
| J_CM4A | 7 | 7 | GND | `GND` | 16 |
| J_CM4A | 8 | 8 | GND | `GND` | 16 |
| J_CM4A | 9 | 9 | Ethernet_Pair2_N | `ETH2_N` | 16 |
| J_CM4A | 10 | 10 | Ethernet_Pair0_N | `ETH0_N` | 16 |
| J_CM4A | 11 | 11 | Ethernet_Pair2_P | `ETH2_P` | 16 |
| J_CM4A | 12 | 12 | Ethernet_Pair0_P | `ETH0_P` | 16 |
| J_CM4A | 13 | 13 | GND | `GND` | 16 |
| J_CM4A | 14 | 14 | GND | `GND` | 16 |
| J_CM4A | 15 | 15 | Ethernet_nLED3 | `ETH_LED_ACT` | 16 |
| J_CM4A | 17 | 17 | Ethernet_nLED2 | `ETH_LED_SPD` | 16 |
| J_CM4A | 21 | 21 | Pi_nLED_Activity | `PI_LED_ACT` | 16 |
| J_CM4A | 22 | 22 | GND | `GND` | 16 |
| J_CM4A | 23 | 23 | GND | `GND` | 16 |
| J_CM4A | 24 | 24 | GPIO26 | `GPIO26` | 16 |
| J_CM4A | 25 | 25 | GPIO21 | `GPIO21` | 16 |
| J_CM4A | 26 | 26 | GPIO19 | `GPIO19` | 16 |
| J_CM4A | 27 | 27 | GPIO20 | `GPIO20` | 16 |
| J_CM4A | 28 | 28 | GPIO13 | `GPIO13` | 17 |
| J_CM4A | 29 | 29 | GPIO16 | `GPIO16` | 17 |
| J_CM4A | 30 | 30 | GPIO6 | `GPIO6` | 17 |
| J_CM4A | 31 | 31 | GPIO12 | `GPIO12` | 17 |
| J_CM4A | 32 | 32 | GND | `GND` | 17 |
| J_CM4A | 33 | 33 | GND | `GND` | 17 |
| J_CM4A | 34 | 34 | GPIO5 | `GPIO5` | 17 |
| J_CM4A | 35 | 35 | ID_SC | `ID_SC` | 17 |
| J_CM4A | 36 | 36 | ID_SD | `ID_SD` | 17 |
| J_CM4A | 37 | 37 | GPIO7 | `GPIO7` | 17 |
| J_CM4A | 38 | 38 | GPIO11 | `GPIO11` | 17 |
| J_CM4A | 39 | 39 | GPIO8 | `GPIO8` | 17 |
| J_CM4A | 40 | 40 | GPIO9 | `GPIO9` | 17 |
| J_CM4A | 41 | 41 | GPIO25 | `GPIO25` | 17 |
| J_CM4A | 42 | 42 | GND | `GND` | 17 |
| J_CM4A | 43 | 43 | GND | `GND` | 17 |
| J_CM4A | 44 | 44 | GPIO10 | `GPIO10` | 17 |
| J_CM4A | 45 | 45 | GPIO24 | `GPIO24` | 17 |
| J_CM4A | 46 | 46 | GPIO22 | `GPIO22` | 17 |
| J_CM4A | 47 | 47 | GPIO23 | `GPIO23` | 17 |
| J_CM4A | 48 | 48 | GPIO27 | `GPIO27` | 17 |
| J_CM4A | 49 | 49 | GPIO18 | `GPIO18` | 17 |
| J_CM4A | 50 | 50 | GPIO17 | `GPIO17` | 17 |
| J_CM4A | 51 | 51 | GPIO15 | `GPIO15` | 17 |
| J_CM4A | 52 | 52 | GND | `GND` | 17 |
| J_CM4A | 53 | 53 | GND | `GND` | 17 |
| J_CM4A | 54 | 54 | GPIO4 | `GPIO4` | 18 |
| J_CM4A | 55 | 55 | GPIO14 | `GPIO14` | 18 |
| J_CM4A | 56 | 56 | GPIO3 | `GPIO3` | 18 |
| J_CM4A | 57 | 57 | SD_CLK | `SD_CLK` | 18 |
| J_CM4A | 58 | 58 | GPIO2 | `GPIO2` | 18 |
| J_CM4A | 59 | 59 | GND | `GND` | 18 |
| J_CM4A | 60 | 60 | GND | `GND` | 18 |
| J_CM4A | 61 | 61 | SD_DAT3 | `SD_DAT3` | 18 |
| J_CM4A | 62 | 62 | SD_CMD | `SD_CMD` | 18 |
| J_CM4A | 63 | 63 | SD_DAT0 | `SD_DAT0` | 18 |
| J_CM4A | 65 | 65 | GND | `GND` | 18 |
| J_CM4A | 66 | 66 | GND | `GND` | 18 |
| J_CM4A | 67 | 67 | SD_DAT1 | `SD_DAT1` | 18 |
| J_CM4A | 69 | 69 | SD_DAT2 | `SD_DAT2` | 18 |
| J_CM4A | 71 | 71 | GND | `GND` | 18 |
| J_CM4A | 74 | 74 | GND | `GND` | 18 |
| J_CM4A | 75 | 75 | SD_PWR_ON | `SD_PWR_ON` | 18 |
| J_CM4A | 77 | 77 | +5V | `+5V` | 18 |
| J_CM4A | 78 | 78 | GPIO_VREF | `GPIO_VREF` | 18 |
| J_CM4A | 79 | 79 | +5V | `+5V` | 18 |
| J_CM4A | 80 | 80 | SCL0 | `SCL0` | 18 |
| J_CM4A | 81 | 81 | +5V | `+5V` | 18 |
| J_CM4A | 82 | 82 | SDA0 | `SDA0` | 18 |
| J_CM4A | 83 | 83 | +5V | `+5V` | 18 |
| J_CM4A | 84 | 84 | CM4_3.3V | `CM4_3V3` | 18 |
| J_CM4A | 85 | 85 | +5V | `+5V` | 18 |
| J_CM4A | 86 | 86 | CM4_3.3V | `CM4_3V3` | 18 |
| J_CM4A | 87 | 87 | +5V | `+5V` | 18 |
| J_CM4A | 88 | 88 | CM4_1.8V | `CM4_1V8` | 19 |
| J_CM4A | 90 | 90 | CM4_1.8V | `CM4_1V8` | 19 |
| J_CM4A | 92 | 92 | RUN_PG | `RUN_PG` | 19 |
| J_CM4A | 93 | 93 | nRPIBOOT | `NRPIBOOT` | 19 |
| J_CM4A | 95 | 95 | PI_LED_nPWR | `PI_LED_NPWR` | 19 |
| J_CM4A | 97 | 97 | Camera_GPIO | `CAM_GPIO` | 19 |
| J_CM4A | 98 | 98 | GND | `GND` | 19 |
| J_CM4A | 99 | 99 | GLOBAL_EN | `GLOBAL_EN` | 19 |
| J_CM4A | 100 | 100 | nEXTRST | `NEXTRST` | 19 |
| J_CM4B | 1 | 101 | USB_OTG_ID | `GND` | 19 |
| J_CM4B | 3 | 103 | USB_N | `USB_UP_N` | 19 |
| J_CM4B | 5 | 105 | USB_P | `USB_UP_P` | 19 |
| J_CM4B | 7 | 107 | GND | `GND` | 19 |
| J_CM4B | 8 | 108 | GND | `GND` | 19 |
| J_CM4B | 13 | 113 | GND | `GND` | 19 |
| J_CM4B | 14 | 114 | GND | `GND` | 19 |
| J_CM4B | 19 | 119 | GND | `GND` | 20 |
| J_CM4B | 20 | 120 | GND | `GND` | 20 |
| J_CM4B | 25 | 125 | GND | `GND` | 20 |
| J_CM4B | 26 | 126 | GND | `GND` | 20 |
| J_CM4B | 28 | 128 | CAM0_D0_N | `CAM_D0_N` | 20 |
| J_CM4B | 30 | 130 | CAM0_D0_P | `CAM_D0_P` | 20 |
| J_CM4B | 31 | 131 | GND | `GND` | 20 |
| J_CM4B | 32 | 132 | GND | `GND` | 20 |
| J_CM4B | 34 | 134 | CAM0_D1_N | `CAM_D1_N` | 20 |
| J_CM4B | 36 | 136 | CAM0_D1_P | `CAM_D1_P` | 20 |
| J_CM4B | 37 | 137 | GND | `GND` | 20 |
| J_CM4B | 38 | 138 | GND | `GND` | 20 |
| J_CM4B | 40 | 140 | CAM0_C_N | `CAM_CK_N` | 20 |
| J_CM4B | 42 | 142 | CAM0_C_P | `CAM_CK_P` | 20 |
| J_CM4B | 44 | 144 | GND | `GND` | 20 |
| J_CM4B | 50 | 150 | GND | `GND` | 20 |
| J_CM4B | 51 | 151 | HDMI0_CEC | `HDMI_CEC` | 21 |
| J_CM4B | 53 | 153 | HDMI0_HOTPLUG | `HDMI_HPD` | 21 |
| J_CM4B | 55 | 155 | GND | `GND` | 21 |
| J_CM4B | 56 | 156 | GND | `GND` | 21 |
| J_CM4B | 57 | 157 | DSI0_D0_N | `DSI_D0_N` | 21 |
| J_CM4B | 59 | 159 | DSI0_D0_P | `DSI_D0_P` | 21 |
| J_CM4B | 61 | 161 | GND | `GND` | 21 |
| J_CM4B | 62 | 162 | GND | `GND` | 21 |
| J_CM4B | 63 | 163 | DSI0_D1_N | `DSI_D1_N` | 21 |
| J_CM4B | 65 | 165 | DSI0_D1_P | `DSI_D1_P` | 21 |
| J_CM4B | 67 | 167 | GND | `GND` | 21 |
| J_CM4B | 68 | 168 | GND | `GND` | 21 |
| J_CM4B | 69 | 169 | DSI0_C_N | `DSI_CK_N` | 21 |
| J_CM4B | 70 | 170 | HDMI0_TX2_P | `HDMI_D2_P` | 21 |
| J_CM4B | 71 | 171 | DSI0_C_P | `DSI_CK_P` | 21 |
| J_CM4B | 72 | 172 | HDMI0_TX2_N | `HDMI_D2_N` | 21 |
| J_CM4B | 73 | 173 | GND | `GND` | 21 |
| J_CM4B | 74 | 174 | GND | `GND` | 21 |
| J_CM4B | 76 | 176 | HDMI0_TX1_P | `HDMI_D1_P` | 21 |
| J_CM4B | 78 | 178 | HDMI0_TX1_N | `HDMI_D1_N` | 21 |
| J_CM4B | 79 | 179 | GND | `GND` | 21 |
| J_CM4B | 80 | 180 | GND | `GND` | 21 |
| J_CM4B | 82 | 182 | HDMI0_TX0_P | `HDMI_D0_P` | 21 |
| J_CM4B | 84 | 184 | HDMI0_TX0_N | `HDMI_D0_N` | 21 |
| J_CM4B | 85 | 185 | GND | `GND` | 21 |
| J_CM4B | 86 | 186 | GND | `GND` | 21 |
| J_CM4B | 88 | 188 | HDMI0_CLK_P | `HDMI_CK_P` | 21 |
| J_CM4B | 90 | 190 | HDMI0_CLK_N | `HDMI_CK_N` | 22 |
| J_CM4B | 91 | 191 | GND | `GND` | 22 |
| J_CM4B | 92 | 192 | GND | `GND` | 22 |
| J_CM4B | 97 | 197 | GND | `GND` | 22 |
| J_CM4B | 98 | 198 | GND | `GND` | 22 |
| J_CM4B | 99 | 199 | HDMI0_SDA | `HDMI_SDA_MCU` | 22 |
| J_CM4B | 100 | 200 | HDMI0_SCL | `HDMI_SCL_MCU` | 22 |


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
