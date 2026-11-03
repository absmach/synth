# I2C sensor node

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from a 5 V input on a JST XH connector through a 0.5 A resettable fuse and a Schottky diode, with an AMS1117-3.3 making the 3.3 V rail. The board has a serial connector (J3), a 12-pin GPIO header (J5) with protection on every line, an SWD header (J4), a power LED and two M2.5 mounting holes. Two copper layers (F.Cu and B.Cu, with a GND pour on each), 86.99 x 51.0 mm, 57 parts.

The design is `i2c_sensor_node.synth` with its placement sidecar `i2c_sensor_node.placement.layout.toml`. It was made with `synth mcp` from a prompt, then exported through the Synth MCP server (the `synth_export` tool) and checked with KiCad. [`PROVENANCE.md`](PROVENANCE.md) is the record of that path: the exact prompt, the agent's tool-call trace, the Synth revisions, the registry inputs, the artifact hashes and the validation commands with their results.

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

KiCad 10.0.7 on a 16-vCPU server; Synth built from commit `c0649a053a68ed007a7ce4a73d2f2d244dfef236` (branch `case-binary-v4` of `JeffMboya/synth`, the merge of Synth pull requests #370, #388 and #394); KiCadRoutingTools `9399bfb29e072bfe60b10d9cdae20c274fc2a17a`. The figures below are from the first of the three exports in [`export/`](export/).

| Item | Result |
|---|---|
| Router state | `routed`, `fabrication_ready` True, 0 blocking findings, 0 open nets |
| Release gate (`synth export-kicad`, [`export/release.json`](export/release.json)) | `release_ready` True, `production_status` production, 0 blocking signal-integrity findings |
| Outline | 86.99 x 51.0 mm = 4436 mm2 |
| Parts | 57 (including the two mounting holes); courtyard fill 21.1 % |
| Routing | 35 nets connected, 537 track segments, 136 vias |
| KiCad DRC (`--refill-zones --severity-all`, with the footprint library table) | 0 errors, 0 unconnected; warnings: 57 `lib_footprint_issues` ([`reports/kicad_drc.json`](reports/kicad_drc.json)) |
| KiCad ERC, design diagnostics | 1 `pin_to_pin` warning, 0 errors ([`reports/kicad_erc.json`](reports/kicad_erc.json)): `U3` pin 5 (`SDO`) is tied to ground to select I2C address 0x76. KiCad's `Sensor:BME680` symbol types that pin bidirectional (it is the SPI data output) and ground carries a `PWR_FLAG`, so ERC reports a bidirectional pin on a power-output net. In I2C mode the pin is a static input and nothing drives it. |
| KiCad ERC, library configuration | 121 `lib_symbol_issues` and 57 `footprint_link_issues` warnings: the exporter's embedded symbols and footprints against the library copies |
| Gerber and drill content hash | `0af456c3a2` (identical across the three exports once the creation time in the Gerber headers is masked) |

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

Net classes (PWR 0.5 mm, V33 0.4 mm, SIG 0.25 mm, GNDC 0.4 mm, Default 0.2 mm; vias 0.6 mm over a 0.2 mm drill) are declared and assigned by net; the exported board uses them as listed, with the router narrowing tracks near pads:

    classes {'PWR': (0.5, 0.6, 0.2), 'SIG': (0.25, 0.6, 0.2), 'V33': (0.4, 0.6, 0.2), 'GNDC': (0.4, 0.6, 0.2), 'Default': (0.2, 0.6, 0.2)}
    SIG {0.25: 247, 0.2: 71, 0.21: 30, 0.22: 30, 0.23: 30, 0.24: 31}
    V33 {0.4: 76, 0.2: 7}
    PWR {0.5: 15}

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The server, the request, the response and the hash of every file written are recorded in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/). Each cell is the first ten characters of a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `i2c_sensor_node.synth.kicad_pcb` (un-routed) | 9db9ad9732 | 9db9ad9732 | 9db9ad9732 | byte-identical |
| `i2c_sensor_node.kicad_pcb` | b7474ccb47 | e46e6ed44e | 506bfa2692 | not byte-identical; identical after the router's and KiCad's random ids are masked (normalized) |
| `i2c_sensor_node.kicad_sch` (one A1 sheet) | ed3e118032 | ed3e118032 | ed3e118032 | byte-identical |
| `i2c_sensor_node.kicad_pro` | 8b000fbbf8 | 8b000fbbf8 | 8b000fbbf8 | byte-identical |
| `i2c_sensor_node.kicad_sym` | f8b593e51b | f8b593e51b | f8b593e51b | byte-identical |
| `bom.csv` | ec31ca8c8a | ec31ca8c8a | ec31ca8c8a | byte-identical |
| `pnp.csv` | 43819c0b2a | 43819c0b2a | 43819c0b2a | byte-identical |
| Gerber and drill content | 0af456c3a2 | 0af456c3a2 | 0af456c3a2 | identical after the creation time in the headers is masked (normalized) |

With the random ids masked the routed board is `3c411267b4` in all three exports ([`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking). [`reproduce/reproduce.sh`](reproduce/reproduce.sh) is supplemental tooling, not the evidence of how the board was made (see [`PROVENANCE.md`](PROVENANCE.md)); it rebuilds Synth at the pinned commit, exports three times and compares, then builds the fabrication package.

## Return paths

The release report ([`export/release.json`](export/release.json)) lists 12 `W-SYNTH-DRC-016` findings, 0 of them blocking: a track changes layer at a via with no ground-return via within 1.53 mm. By the net of the via at each finding's position:

| Net | Findings |
|---|---|
| `+5V_PROT` | 2 |
| `LED_PWR` | 2 |
| `GPIO_PA5` | 2 |
| `GPIO_PB0_MCU` | 1 |
| `SWDIO` | 1 |
| `GPIO_PA6` | 1 |
| `3V3_REF` | 1 |
| `GPIO_PA1` | 1 |
| `GPIO_PA0` | 1 |

Synth's release gate holds a board back for a missing return via only on a net that rides a fast edge, and it classified all nine nets as not doing so. The router looks for a ground via beside each layer change and reports the places where no spot in the ground pour is clear of pads, vias and tracks. Both outer layers carry a GND pour, so the return current on these nets stays on a continuous ground plane.

## Human review

Every part of the board is recorded as reviewed by JeffMboya, with the review date and the datasheet or product page, in the part files of [`reproduce/user-registry/`](reproduce/user-registry/) (`reviewed_by`, `reviewed_at`, `datasheet_url`). The release report ([`export/release.json`](export/release.json)) lists every part as reviewed.

## Review points (Sammy, case 2) and the state of each

| Point | State |
|---|---|
| A plain default export is valid; no optional flag, no split ground pour | The exports call `synth_export` with the source, the sidecar and the output directory only (no `allow_incomplete`, no via-in-pad override). The ground net GND is poured on both layers as one net. |
| Three exports agree | Byte-identical across the three exports: `kicad_sch`, `kicad_pro`, `kicad_sym`, `bom.csv`, `pnp.csv` and the un-routed `synth.kicad_pcb`. Not byte-identical but equal after normalization: the routed `kicad_pcb` (the router and KiCad write random ids; compared with those masked, so geometrically equivalent) and the Gerber, drill and job files (the headers carry the creation time; compared by the content hash with it masked). |
| Every external line of J3 protected | TX goes through R11 (330 ohm) and D7, a 3.3 V zener (BZT52C3V3, 500 mW); RX through R12 (1 k) and D24. A 5 V fault: (5 - 3.3) / 330 = 5.2 mA and (5 - 3.3) / 1000 = 1.7 mA, so 17 mW and 5.6 mW in the zener. The 3.3 V sense line has D14 (1N5819HW) and 100 ohm: 3 mA maximum, 30 mA into a short. The zener's leakage and clamp figures are from its listing, not an opened datasheet. |
| Net classes declared and applied | Declared (PWR, V33, GNDC, SIG) and assigned to 39 nets; the exported board's tracks use them as listed above, with the router narrowing tracks near pads. |
| AMS1117 thermal | P = (5.0 - 3.3) x 0.1 A = 0.17 W at the 100 mA budget and 0.51 W at 300 mA. The datasheet gives thetaJA = 90 C/W for the SOT-223 (45 to 59 C/W with 1000 to 2500 mm2 of copper, its Table 1), so the rise is 15.3 C and 45.9 C and the ambient may reach 109 C and 79 C at the 125 C junction limit. The input is rated 15 V absolute maximum and D4 clamps at 10.3 V; the output needs a 22 uF tantalum for stability and C3 is 47 uF. The registry part `ams1117_3v3` carries the datasheet figures: thetaJA 90 C/W and 1.2 W guaranteed power dissipation (its Note 2). |
| Every part has distributor identity | [`fab/i2c_sensor_node_bom_orderable.csv`](fab/i2c_sensor_node_bom_orderable.csv): manufacturer, MPN, package, rating, datasheet or page, distributor and order number for every line; the exports use no `--allow-unverified-parts`. |
| Schematic readability and silk overlaps | The schematic is one A1 sheet with seven named groups, each with its notes, and no `OTHER` region; the DRC reports no `silk_overlap`. |
