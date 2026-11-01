# I2C sensor node

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from a 5 V input on a JST XH connector through a 0.5 A resettable fuse and a Schottky diode, with an AMS1117-3.3 making the 3.3 V rail. The board has a serial connector (J3), a 12-pin GPIO header (J5) with protection on every line, an SWD header (J4), a power LED and two M2.5 mounting holes. Four copper layers on the JLCPCB 1.6 mm stackup, 93.36 x 67.7 mm, 57 parts.

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
| Mounting | H2, H3 | Two M2.5 non-plated holes on opposite corners: H2 at (7.2, 7.2), H3 at (87.0, 61.4). |

## Power and programming

- The board is powered from J1 only: 5 V in.
- It is programmed and debugged over the SWD header J4 with a J-Link or ST-LINK at 3.3 V logic: pin 1 is the 3.3 V sense, 2 SWDIO, 3 SWCLK, 4 GND, 5 NRST. The reset button SW1 and the NRST pin on J4 reset the chip.
- A serial console is on the J3 UART pins (3.3 V logic) with an external USB-UART adapter.
- PA9 and PA10 are unconnected. BOOT0 shares PA14 and SWCLK; with the default option bytes the pin is ignored.

## Measured results

KiCad 10.0.7 on a 16-vCPU server; Synth built from commit `cfec42751ffd898cd4425322247e56b77951ed0e` (branch `SYN-case2-binary` of `JeffMboya/synth`, the merge of Synth pull requests #370, #388 and #394); KiCadRoutingTools `9399bfb29e072bfe60b10d9cdae20c274fc2a17a`. The figures below are from the first of the three exports in [`export/`](export/).

| Item | Result |
|---|---|
| Router state | `routed`, `fabrication_ready` True, 0 blocking findings, 0 open nets |
| Outline | 93.36 x 67.7 mm = 6320 mm2 |
| Parts | 57 (including the two mounting holes) |
| Routing | 35 nets connected, 508 track segments, 69 vias |
| KiCad DRC (`--refill-zones --severity-all`, with the footprint library table) | 0 errors, 0 unconnected; warnings: 53 `lib_footprint_mismatch` ([`reports/kicad_drc.json`](reports/kicad_drc.json)) |
| KiCad ERC | 126 `lib_symbol_issues`, 1 `pin_to_pin` ([`reports/kicad_erc.json`](reports/kicad_erc.json)) |
| Gerber and drill content hash | `169492d786` (identical across the three exports) |

Connector distance to the board edge (courtyard to Edge.Cuts, measured on the exported board):

    i2c_sensor_node.kicad_pcb outline x 0.0..93.36  y 0.0..67.7
      J1   rot=  270  nearest edge=left    courtyard-to-edge= -0.00 mm   (all: left=-0.0, right=80.9, top=28.4, bottom=30.9)
      J4   rot=   90  nearest edge=bottom  courtyard-to-edge=  0.00 mm   (all: left=40.4, right=39.2, top=64.2, bottom=0.0)
      J3   rot=   90  nearest edge=right   courtyard-to-edge=  0.00 mm   (all: left=84.8, right=0.0, top=27.4, bottom=29.4)
      J5   rot=   90  nearest edge=top     courtyard-to-edge=  0.00 mm   (all: left=31.4, right=30.4, top=0.0, bottom=64.2)

Decoupling capacitors, pad to the supply pin they serve (centre to centre; the placement review's limit is 4 mm):

    C1   pad on +5V_PROT  -> U1.3     5.7 mm
    C2   pad on +5V_PROT  -> U1.3    11.6 mm
    C3   pad on +3V3      -> U1.2     8.5 mm
    C4   pad on +3V3      -> U1.2     7.0 mm
    C5   pad on +3V3      -> U1.2     4.7 mm
    C6   pad on +3V3      -> U2.4     4.3 mm
    C7   pad on +3V3      -> U2.4     8.2 mm
    C8   pad on NRST      -> J4.5    16.4 mm
    C10  pad on +3V3      -> U3.6     7.1 mm
    C11  pad on +3V3      -> U3.8     4.9 mm
    C12  pad on +3V3      -> U5.8     6.4 mm

Net classes (PWR 0.5 mm, V33 0.4 mm, SIG 0.25 mm, GNDC 0.4 mm, Default 0.2 mm; vias 0.6 / 0.3 mm), as exported:

    classes {'PWR': (0.5, 0.6, 0.3), 'SIG': (0.25, 0.6, 0.3), 'V33': (0.4, 0.6, 0.3), 'GNDC': (0.4, 0.6, 0.3), 'Default': (0.2, 0.6, 0.3)}
    assignments 39 patterns 4
    SIG {0.25: 197, 0.21000000000000002: 29, 0.22: 30, 0.23: 29, 0.24: 30, 0.2: 70}
    V33 {0.4: 81, 0.36000000000000004: 1, 0.32: 1, 0.28: 1, 0.24000000000000002: 1, 0.2: 4}
    PWR {0.5: 10}
    GNDC {0.2: 24}
    segments narrower or wider than their class width: 220
    [(('SIG', 0.2, 0.25), 70), (('SIG', 0.22, 0.25), 30), (('SIG', 0.24, 0.25), 30), (('SIG', 0.21000000000000002, 0.25), 29), (('SIG', 0.23, 0.25), 29), (('GNDC', 0.2, 0.4), 24)]

## Reproducibility

The source and sidecar in this directory were exported three times with `synth export-kicad`, each into an empty directory, router included. Each cell is the first ten characters of a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `i2c_sensor_node.synth.kicad_pcb` (un-routed) | 37a9b29833 | 37a9b29833 | 37a9b29833 | identical |
| `i2c_sensor_node.kicad_pcb` | 34178c2fa3 | 0de7e93c23 | f2779b74e9 | identical after the router's and KiCad's random ids |
| `i2c_sensor_node.kicad_sch` and its sheets | 5c212dd7a5 | 5c212dd7a5 | 5c212dd7a5 | identical |
| `i2c_sensor_node.kicad_pro` | 3a3bf024b2 | 3a3bf024b2 | 3a3bf024b2 | identical |
| `i2c_sensor_node.kicad_sym` | 57046d2a5c | 57046d2a5c | 57046d2a5c | identical |
| `bom.csv` | ec31ca8c8a | ec31ca8c8a | ec31ca8c8a | identical |
| `pnp.csv` | d377cf29b9 | d377cf29b9 | d377cf29b9 | identical |
| Gerber and drill content | 169492d786 | 169492d786 | 169492d786 | identical |

With the random ids masked the routed board is `masked)` in all three exports ([`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking). [`reproduce/reproduce.sh`](reproduce/reproduce.sh) rebuilds Synth at the pinned commit, exports three times and compares, then builds the fabrication package.

## Review points (Sammy, case 2) and the state of each

| Point | State |
|---|---|
| A plain default export is valid; no optional flag, no split ground pour | The exports use `synth export-kicad` with the router options only (no `--allow-incomplete`, no `--krt-allow-via-in-pad`). The ground net GND is poured on all four layers as one net. |
| Three identical exports | Above. |
| Every external line of J3 protected | TX goes through R11 (330 ohm) and D7, a 3.3 V zener (BZT52C3V3, 500 mW); RX through R12 (1 k) and D24. A 5 V fault: (5 - 3.3) / 330 = 5.2 mA and (5 - 3.3) / 1000 = 1.7 mA, so 17 mW and 5.6 mW in the zener. The 3.3 V sense line has D14 (1N5819HW) and 100 ohm: 3 mA maximum, 30 mA into a short. The zener's leakage and clamp figures are from its listing, not an opened datasheet. |
| Net classes declared and applied | Declared (PWR, V33, GNDC, SIG) and assigned to 39 nets; the exported board's tracks use them as listed above, with the router narrowing tracks near pads. |
| AMS1117 thermal | P = (5.0 - 3.3) x 0.1 A = 0.17 W at the 100 mA budget and 0.51 W at 300 mA. The datasheet gives thetaJA = 90 C/W for the SOT-223 (45 to 59 C/W with 1000 to 2500 mm2 of copper, its Table 1), so the rise is 15.3 C and 45.9 C and the ambient may reach 109 C and 79 C at the 125 C junction limit. The input is rated 15 V absolute maximum and D4 clamps at 10.3 V; the output needs a 22 uF tantalum for stability and C3 is 47 uF. |
| Every part has distributor identity | [`fab/i2c_sensor_node_bom_orderable.csv`](fab/i2c_sensor_node_bom_orderable.csv): manufacturer, MPN, package, rating, datasheet or page, distributor and order number for every line; the exports use no `--allow-unverified-parts`. |
| Schematic readability and silk overlaps | The schematic has one sheet per functional group; the DRC reports no `silk_overlap`. |

## Open items

- JLCPCB PCB and SMT DFM, and the quote: the package in [`fab/`](fab/) is ready to upload (no order is placed).
- Decoupling distance: every decoupling capacitor except C5 and C6 sits more than 4 mm from the supply pin it serves (4.3 to 16.4 mm in the table above; the review's limit is 4 mm). Placing them at the pins left the router unable to connect the MCU's GPIO and SWD nets in every attempt.
