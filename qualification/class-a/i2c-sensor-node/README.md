# I2C sensor node

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from a 5 V input on a JST XH connector through a 0.5 A resettable fuse and a Schottky diode, with an AMS1117-3.3 making the 3.3 V rail. The board has a serial connector (J3), a 12-pin GPIO header (J5) with protection on every line, an SWD header (J4), a power LED and two M2.5 mounting holes. Four copper layers on the JLCPCB 1.6 mm stackup, 86.99 x 51.0 mm, 57 parts.

The design is `i2c_sensor_node.synth` with its placement sidecar `i2c_sensor_node.placement.layout.toml`. It was made with `synth mcp` from a prompt, then checked and exported with `synth export-kicad`.

## What the board is

| Function | Parts | What it does |
|---|---|---|
| 5 V input | J1 (JST S2B-XH-A-1), F1 (1812L050/60MR, 0.5 A hold), D1 (1N5819HW), D4 (PTVS6V0S1UR) | 5 V enters on J1. F1 limits fault current, D1 blocks reverse polarity, D4 clamps surges (standoff 6 V, clamp 10.3 V maximum per the distributor listings). |
| 3.3 V regulator | U1 (AMS1117-3.3), C1 and C2 (10 uF, input), C3 (47 uF tantalum), C4 (10 uF) and C5 (100 nF) on the output, D15 and R24 (power LED) | Makes the 3.3 V rail from the protected 5 V. The LED shows that the rail is up. |
| Microcontroller | U2 (STM32G030F6P6), C6 (100 nF) and C7 (4.7 uF) on VDD, R3 (10 k), C8 (100 nF) and SW1 on the reset line, J4 (SWD header), R13 and R14 (100 ohm), D8, D9, D19 (3.3 V zeners) | Runs the firmware. The SWD lines and the reset line have series resistors or pull-ups and 3.3 V zener clamps. |
| I2C sensing | U3 (BME680), U5 (24LC256), C10, C11, C12 (100 nF), R9 and R10 (4.7 k pull-ups) | One I2C bus (PB7 SDA, PB6 SCL) shared by the sensor and the EEPROM. |
| Serial connector | J3 (JST S4B-PH-K-S), R11 (330 ohm), R12 (1 k), D7, D24 (3.3 V zeners) | UART TX and RX of the MCU (PA2 and PA3), with 3.3 V sense and ground. |
| GPIO header | J5 (12-pin), R15 to R22 (330 ohm), D10 to D13 and D20 to D23 (3.3 V zeners), D14 and R23 | Eight MCU lines (ADC, SPI and timer pins) with a series resistor and a 3.3 V zener each, plus a 3.3 V sense line through D14 and 100 ohm. |
| Mounting | H2, H3 | Two M2.5 non-plated holes on opposite corners: H2 at (7.2, 7.2), H3 at (82.5, 42.9). |

## Power and programming

- The board is powered from J1 only: 5 V in.
- It is programmed and debugged over the SWD header J4 with a J-Link or ST-LINK at 3.3 V logic: pin 1 is the 3.3 V sense, 2 SWDIO, 3 SWCLK, 4 GND, 5 NRST. The reset button SW1 and the NRST pin on J4 reset the chip.
- A serial console is on the J3 UART pins (3.3 V logic) with an external USB-UART adapter.
- PA9 and PA10 are unconnected. BOOT0 shares PA14 and SWCLK; with the default option bytes the pin is ignored.

## Measured results

KiCad 10.0.7 on a 16-vCPU server; Synth built from commit `aaa8d193fe9573c554a5ed4a4a1c26a492030c8d` (branch `SYN-case2-binary` of `JeffMboya/synth`, the merge of Synth pull requests #370, #388 and #394); KiCadRoutingTools `9399bfb29e072bfe60b10d9cdae20c274fc2a17a`. The figures below are from the first of the three exports in [`export/`](export/).

| Item | Result |
|---|---|
| Router state | `routed`, `fabrication_ready` True, 0 blocking findings, 0 open nets |
| Outline | 86.99 x 51.0 mm = 4436 mm2 |
| Parts | 57 (including the two mounting holes) |
| Routing | 35 nets connected, 602 track segments, 73 vias |
| KiCad DRC (`--refill-zones --severity-all`, with the footprint library table) | 0 errors, 0 unconnected; warnings: 52 `lib_footprint_mismatch` ([`reports/kicad_drc.json`](reports/kicad_drc.json)) |
| KiCad ERC | 121 `lib_symbol_issues`, 1 `pin_to_pin` ([`reports/kicad_erc.json`](reports/kicad_erc.json)) |
| Gerber and drill content hash | `1c1755660b` (identical across the three exports) |

Connector distance to the board edge (courtyard to Edge.Cuts, measured on the exported board):

    i2c_sensor_node.kicad_pcb outline x 0.4..87.39  y 0.5..51.5
      J1   rot=  270  nearest edge=left    courtyard-to-edge= -0.00 mm   (all: left=-0.0, right=74.5, top=22.4, bottom=20.2)
      J4   rot=   90  nearest edge=bottom  courtyard-to-edge=  0.02 mm   (all: left=21.6, right=51.7, top=47.4, bottom=0.0)
      J3   rot=  270  nearest edge=left    courtyard-to-edge= -0.00 mm   (all: left=-0.0, right=78.4, top=38.0, bottom=2.0)
      J5   rot=   90  nearest edge=top     courtyard-to-edge=  0.00 mm   (all: left=26.6, right=29.0, top=0.0, bottom=47.5)

Decoupling capacitors, pad to the supply pin they serve (pad to supply pin):

    C1   pad on +5V_PROT  -> U1.3     2.8 mm
    C2   pad on +5V_PROT  -> U1.3     5.1 mm
    C3   pad on +3V3      -> U1.2     6.4 mm
    C4   pad on +3V3      -> U1.2     7.0 mm
    C5   pad on +3V3      -> U1.2     4.7 mm
    C6   pad on +3V3      -> U2.4     4.3 mm
    C7   pad on +3V3      -> U2.4     8.2 mm
    C8   pad on NRST      -> U2.6     4.1 mm
    C10  pad on +3V3      -> U3.6     4.2 mm
    C11  pad on +3V3      -> U3.8     4.9 mm
    C12  pad on +3V3      -> U5.8     2.0 mm

Net classes (PWR 0.5 mm, V33 0.4 mm, SIG 0.25 mm, GNDC 0.4 mm, Default 0.2 mm; vias 0.6 mm over a 0.2 mm drill), as exported:

    classes {'PWR': (0.5, 0.6, 0.2), 'SIG': (0.25, 0.6, 0.2), 'V33': (0.4, 0.6, 0.2), 'GNDC': (0.4, 0.6, 0.2), 'Default': (0.2, 0.6, 0.2)}
    assignments 52 patterns 17
    SIG {0.25: 229, 0.2: 75, 0.21000000000000002: 34, 0.22: 33, 0.23: 34, 0.24: 32, 0.127: 8}
    V33 {0.4: 85, 0.36000000000000004: 1, 0.32: 1, 0.28: 1, 0.24000000000000002: 1, 0.2: 2}
    PWR {0.5: 23}
    GNDC {0.2: 34, 0.8: 9}
    segments narrower or wider than their class width: 265
    [(('SIG', 0.2, 0.25), 75), (('SIG', 0.21000000000000002, 0.25), 34), (('SIG', 0.23, 0.25), 34), (('GNDC', 0.2, 0.4), 34), (('SIG', 0.22, 0.25), 33), (('SIG', 0.24, 0.25), 32)]

## Reproducibility

The source and sidecar in this directory were exported three times with `synth export-kicad`, each into an empty directory, router included. Each cell is the first ten characters of a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `i2c_sensor_node.synth.kicad_pcb` (un-routed) | ba85e901ff | ba85e901ff | ba85e901ff | identical |
| `i2c_sensor_node.kicad_pcb` | c76451a264 | 1d288a6a22 | 691a25cd42 | identical after the router's and KiCad's random ids |
| `i2c_sensor_node.kicad_sch` (one A1 sheet) | ed3e118032 | ed3e118032 | ed3e118032 | identical |
| `i2c_sensor_node.kicad_pro` | 8b000fbbf8 | 8b000fbbf8 | 8b000fbbf8 | identical |
| `i2c_sensor_node.kicad_sym` | f8b593e51b | f8b593e51b | f8b593e51b | identical |
| `bom.csv` | ec31ca8c8a | ec31ca8c8a | ec31ca8c8a | identical |
| `pnp.csv` | 706483f12e | 706483f12e | 706483f12e | identical |
| Gerber and drill content | 1c1755660b | 1c1755660b | 1c1755660b | identical |

With the random ids masked the routed board is `52980d7667` in all three exports ([`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking). [`reproduce/reproduce.sh`](reproduce/reproduce.sh) rebuilds Synth at the pinned commit, exports three times and compares, then builds the fabrication package.

## JLCPCB

PCB DFM of the package of this README (2026-11-02 13:06, Gerber content hash `1c1755660b`, four layers, 8.7 x 5.1 cm, report in [`reports/`](reports/jlcpcb_pcb_dfm_2026-11-02_13-06_1c1755660b.pdf)): 0 Danger items. Trace spacing 0.25 mm, pad spacing 0.21 mm, annular ring 0.20 mm (100 rings), through-hole to SMD 3.77 mm, solder-mask opening near a trace 0.17 mm and silkscreen line width 0.18 mm are good; silkscreen to pad, silkscreen to hole, via to pad and sharp corners have no finding.

Quote with assembly for the same package ([`reports/jlcpcb_quote_2026-11-02.md`](reports/jlcpcb_quote_2026-11-02.md)): PCB $8.00 and Standard PCBA $140.17, $148.17 before shipping.

BOM matching of the same package in JLCPCB's assembly flow, PCB assembled qty 5, estimated total $69.9405 ([`reports/jlcpcb_assembly_bom_match_2026-11-02.xlsx`](reports/jlcpcb_assembly_bom_match_2026-11-02.xlsx)): 55 lines, 53 matched to a JLCPCB part (21 Basic, 33 Extended library types among the 54 lines with a library type), `SW1` matched but unconfirmed (its comment differs from the matched part's) and `J4`, the SWD pin header, fitted by hand.

## Review points (Sammy, case 2) and the state of each

| Point | State |
|---|---|
| A plain default export is valid; no optional flag, no split ground pour | The exports use `synth export-kicad` with the router options only (no `--allow-incomplete`, no `--krt-allow-via-in-pad`). The ground net GND is poured on all four layers as one net. |
| Three identical exports | Above. |
| Every external line of J3 protected | TX goes through R11 (330 ohm) and D7, a 3.3 V zener (BZT52C3V3, 500 mW); RX through R12 (1 k) and D24. A 5 V fault: (5 - 3.3) / 330 = 5.2 mA and (5 - 3.3) / 1000 = 1.7 mA, so 17 mW and 5.6 mW in the zener. The 3.3 V sense line has D14 (1N5819HW) and 100 ohm: 3 mA maximum, 30 mA into a short. The zener's leakage and clamp figures are from its listing, not an opened datasheet. |
| Net classes declared and applied | Declared (PWR, V33, GNDC, SIG) and assigned to 39 nets; the exported board's tracks use them as listed above, with the router narrowing tracks near pads. |
| AMS1117 thermal | P = (5.0 - 3.3) x 0.1 A = 0.17 W at the 100 mA budget and 0.51 W at 300 mA. The datasheet gives thetaJA = 90 C/W for the SOT-223 (45 to 59 C/W with 1000 to 2500 mm2 of copper, its Table 1), so the rise is 15.3 C and 45.9 C and the ambient may reach 109 C and 79 C at the 125 C junction limit. The input is rated 15 V absolute maximum and D4 clamps at 10.3 V; the output needs a 22 uF tantalum for stability and C3 is 47 uF. The registry part `ams1117_3v3` carries the datasheet figures: thetaJA 90 C/W and 1.2 W guaranteed power dissipation (its Note 2). |
| Every part has distributor identity | [`fab/i2c_sensor_node_bom_orderable.csv`](fab/i2c_sensor_node_bom_orderable.csv): manufacturer, MPN, package, rating, datasheet or page, distributor and order number for every line; the exports use no `--allow-unverified-parts`. |
| Schematic readability and silk overlaps | The schematic is one A1 sheet with seven named groups, each with its notes, and no `OTHER` region; the DRC reports no `silk_overlap`. |
