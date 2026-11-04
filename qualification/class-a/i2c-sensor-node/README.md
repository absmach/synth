# I2C sensor node

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from a 5 V input on a JST XH connector through a 0.5 A resettable fuse and a Schottky diode, with an AMS1117-3.3 making the 3.3 V rail. The board has a serial connector (J3), a 12-pin GPIO header (J5) with protection on every line, an SWD header (J4), a power LED and two M2.5 mounting holes. Two copper layers (F.Cu and B.Cu, with a GND pour on each), 84.0 x 50.0 mm, 57 parts.

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

KiCad 10.0.6; Synth built from commit `0ceb7b8071d40edadc0fff36584922d6f8c3f917` of `absmach/synth` (`upstream/main`, the merge of Synth pull request #388, which carries #370 and #394 before it); KiCadRoutingTools `9399bfb29e072bfe60b10d9cdae20c274fc2a17a`. The figures below are from the first of the three exports in [`export/`](export/).

| Item | Result |
|---|---|
| Router state | `routed`, `fabrication_ready` True, 0 blocking findings, 0 open nets, 12 non-blocking return-via advisories ([`export/i2c_sensor_node.routing.json`](export/i2c_sensor_node.routing.json)) |
| Outline | 84.0 x 50.0 mm = 4197 mm2 |
| Parts | 57 (including the two mounting holes); courtyard fill 22.4 % |
| Routing | 35 nets connected, 423 track segments, 82 vias |
| KiCad DRC (`--refill-zones --severity-all`, footprint library table configured) | 0 errors, 0 unconnected; 55 `lib_footprint_mismatch` warnings and 2 `track_dangling` warnings ([`reports/kicad_drc.json`](reports/kicad_drc.json)). Each is a footprint whose silkscreen differs from the library copy (the exporter cuts silkscreen back from vias and pads); its pads, courtyard and fabrication outline are identical to the library file ([`reports/footprint_diff.txt`](reports/footprint_diff.txt), from [`footprint_diff.py`](agent-run/tools/footprint_diff.py)). The two `track_dangling` warnings are dead-end branches the router left near U2: a track of `GPIO_PB9_MCU`, 1.47 mm long, whose free end is at (43.35, 30.23), and one of `GPIO_PA4_MCU`, 1.93 mm long, free end at (50.65, 35.43). Each joins nothing and carries no current. |
| KiCad ERC, design diagnostics | 1 `pin_to_pin` warning, 0 errors ([`reports/kicad_erc.json`](reports/kicad_erc.json)): `U3` pin 5 (`SDO`) is tied to ground to select I2C address 0x76. KiCad's `Sensor:BME680` symbol types that pin bidirectional (it is the SPI data output) and ground carries a `PWR_FLAG`, so ERC reports a bidirectional pin on a power-output net. In I2C mode the pin is a static input and nothing drives it. |
| KiCad ERC, library configuration | none: with the symbol and footprint library tables configured the only ERC warning is the `pin_to_pin` one above. The tables are [`reports/fp-lib-table`](reports/fp-lib-table) and [`reports/sym-lib-table`](reports/sym-lib-table); [`library_check.sh`](agent-run/tools/library_check.sh) builds them and runs DRC and ERC |
| Gerber and drill content hash | `daa736de85` (identical across the three exports once the creation time in the Gerber headers is masked) |

Connector distance to the board edge (courtyard to Edge.Cuts, measured on the exported board):

    i2c_sensor_node.kicad_pcb outline x 1.225..85.215  y 0.73..50.7
      J1   rot=  270  nearest edge=left    courtyard-to-edge= -0.00 mm   (all: left=-0.0, right=71.5, top=22.2, bottom=19.4)
      J4   rot=   90  nearest edge=bottom  courtyard-to-edge=  0.43 mm   (all: left=20.7, right=49.6, top=46.0, bottom=0.4)
      J3   rot=  270  nearest edge=left    courtyard-to-edge= -0.00 mm   (all: left=-0.0, right=75.4, top=37.8, bottom=1.2)
      J5   rot=   90  nearest edge=top     courtyard-to-edge=  0.00 mm   (all: left=25.7, right=26.8, top=0.0, bottom=46.4)

Decoupling capacitors, pad to the supply pin they serve (pad to supply pin):

    C1   pad on +5V_PROT  -> U1.3     2.8 mm
    C2   pad on +5V_PROT  -> U1.3     4.8 mm
    C3   pad on +3V3      -> U1.2     6.3 mm
    C4   pad on +3V3      -> U1.2     6.8 mm
    C5   pad on +3V3      -> U1.2     4.6 mm
    C6   pad on +3V3      -> U2.4     3.9 mm
    C7   pad on +3V3      -> U2.4     8.0 mm
    C8   pad on NRST      -> U2.6     3.8 mm
    C10  pad on +3V3      -> U3.6     4.4 mm
    C11  pad on +3V3      -> U3.8     4.8 mm
    C12  pad on +3V3      -> U5.8     1.8 mm

Net classes (PWR 0.5 mm, V33 0.4 mm, SIG 0.25 mm, GNDC 0.4 mm, Default 0.2 mm; vias 0.6 mm over a 0.2 mm drill) are declared and assigned by net; PWR, V33 and SIG declare `edge_rate slow`: the board has no clock, differential or switching-node nets, so every signal net is a static or low-speed line (a 3.3 V line behind a series resistor), and the nets that carry analog, clock, crystal or differential pins stay critical whatever their class says. The exported board uses the classes as listed, with the router narrowing tracks near pads:

    classes {'PWR': (0.5, 0.6, 0.2), 'SIG': (0.25, 0.6, 0.2), 'V33': (0.4, 0.6, 0.2), 'GNDC': (0.4, 0.6, 0.2), 'Default': (0.2, 0.6, 0.2)}
    SIG {0.127: 23, 0.25: 257, 0.21: 4, 0.22: 4, 0.23: 4, 0.24: 4, 0.2: 10}
    V33 {0.127: 2, 0.4: 84}
    GNDC {0.127: 3, 0.2: 15}
    PWR {0.5: 12}

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The server, the request, the response and the hash of every file written are recorded in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/). Each cell is the first ten characters of a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `i2c_sensor_node.synth.kicad_pcb` (un-routed) | bdac145e67 | bdac145e67 | bdac145e67 | byte-identical |
| `i2c_sensor_node.kicad_pcb` | 2cdf3231e7 | e64ef15b98 | aa89e5be67 | not byte-identical; identical after the router's and KiCad's random ids are masked (normalized) |
| `i2c_sensor_node.kicad_sch` (one A1 sheet) | ed3e118032 | ed3e118032 | ed3e118032 | byte-identical |
| `i2c_sensor_node.kicad_pro` | 8b000fbbf8 | 8b000fbbf8 | 8b000fbbf8 | byte-identical |
| `i2c_sensor_node.kicad_sym` | f8b593e51b | f8b593e51b | f8b593e51b | byte-identical |
| `bom.csv` | ec31ca8c8a | ec31ca8c8a | ec31ca8c8a | byte-identical |
| `pnp.csv` | 9845518f36 | 9845518f36 | 9845518f36 | byte-identical |
| Gerber and drill content | daa736de85 | daa736de85 | daa736de85 | identical after the creation time in the headers is masked (normalized) |

With the random ids masked the routed board is `4aa8fa857b` in all three exports ([`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking). [`reproduce/reproduce.sh`](reproduce/reproduce.sh) is supplemental tooling, not the evidence of how the board was made (see [`PROVENANCE.md`](PROVENANCE.md)); it rebuilds Synth at the pinned commit, exports three times and compares, then builds the fabrication package.

## JLCPCB

PCB DFM of the package of this README (JLCDFM, 2026-11-04 11:54, Gerber content hash `daa736de85`, two layers, 8.4 x 5 cm, report in [`reports/`](reports/jlcdfm_pcb_dfm_2026-11-04_11-54_daa736de85.pdf)): 0 Danger items. Good: through-hole to trace 0.34 mm, through-hole to SMD 3.98 mm (against the 2.1 mm JLCPCB reports as Danger), silkscreen line width 0.18 mm, no silkscreen over a pad or a hole, no via in a pad. Warnings: trace width 0.13 mm on 8 tracks (92 good), the 0.127 mm stubs at the pads of U2 described below; annular ring 0.13 mm on 4 pads (96 good); negative solder-mask expansion 0.04 mm on 48 openings; one solder-mask opening 0.1 mm from a trace; one pad spacing of 0.12 mm (47 good); one sharp trace corner; and 2 unconnected trace ends, the two dead-end branches in the DRC list.

Quote with assembly for the same package ([`reports/jlcpcb_quote_2026-11-04.md`](reports/jlcpcb_quote_2026-11-04.md), Standard PCBA, no order placed): PCB $4.00 and PCBA $138.86, $142.86 in total.

## Power copper and the routing report

The routing report ([`export/i2c_sensor_node.routing.json`](export/i2c_sensor_node.routing.json)) records `engine_policy_verdict: rejected` next to `fabrication_ready: true`. The two statements are about different checks. The routing engine runs with its strict-size policy, which rejects any feature smaller than its net class declares, and its own ledger lists 3 features on 1 net below the requested size (two fine-pitch taps with a narrowest track of 0.2 mm, and one via-in-pad clamp to 0.46 mm). Measured on the exported file, more tracks are narrower than their class: the engine also draws 0.127 mm stubs at the pads of U2 (28 segments, 26.5 mm in all), which its ledger does not list. Synth records the verdict and then checks the delivered board itself: connectivity (35 of 35 nets), the process floor of 0.127 mm for track width and clearance, KiCad DRC with the library tables (0 errors, 0 unconnected) and the return-via rule. The board passes all of them, so the delivered board is valid. The 80 vias the engine counted are its routing vias; the file holds 82 because Synth's exporter and repair passes add ground vias.

Measured on the exported file, the segments narrower than their class width are on the signal lines (`SIG` declared 0.25 mm), on the 3.3 V rail at U2's supply pin and on ground at U2:

| Net class | Declared | Narrower segments | Narrowest | Load |
|---|---|---|---|---|
| `SIG` lines (the SWD, GPIO, UART, I2C and reset nets) | 0.25 mm | 49 segments, 36.8 mm in all, on 14 nets: 23 of 0.127 mm, 10 of 0.2 mm and 16 of 0.21 to 0.24 mm | 0.127 mm | at most 10 mA (a 3.3 V line behind its 330 ohm series resistor); 33 mA on SWDIO behind 100 ohm; 5.2 mA in a 5 V fault |
| `V33` (`+3V3`) | 0.4 mm | 2 segments of 0.127 mm, at U2's supply pin | 0.127 mm | the 100 mA budget of the 3.3 V rail |
| `GNDC` (ground) | 0.4 mm | 3 of 0.127 mm and 15 of 0.2 mm | 0.127 mm | the 100 mA budget, returning to ground |

A 0.127 mm outer track of 1 oz copper carries 0.53 A at a 10 C rise (IPC-2221) and a 0.2 mm track 0.74 A, so 100 mA raises the narrowest by about 0.2 C and a 10 mA signal line by far less.

## Return paths

The routing report ([`export/i2c_sensor_node.routing.json`](export/i2c_sensor_node.routing.json)) lists 12 non-blocking return-via advisories: a track changes layer at a via with no ground-return via within 1.53 mm and no spot in the GND pour for one. The rule treats a net as fast unless its net class declares `edge_rate slow`; `PWR`, `V33` and `SIG` declare it (see above), so these are advisories, not blocking findings. Each is checked below against the nearest ground via on the exported board. Both outer layers carry a GND pour, so the return current stays on a continuous plane; the advisory is only that the current's path to a stitching via is longer than the rule's window.

| Net | Via position (mm) | Nearest GND via (mm) |
|---|---|---|
| `GPIO_PB0_MCU` | (40.5, 13.8) | 2.00 |
| `GPIO_PA6_MCU` | (51.1, 34.3) | 2.09 |
| `SWDIO_MCU` | (51.1, 30.8) | 2.16 |
| `GPIO_PA0_MCU` | (43.9, 10.0) | 2.18 |
| `GPIO_PA0_MCU` | (45.5, 32.6) | 2.57 |
| `SWDIO` | (33.5, 47.1) | 2.75 |
| `GPIO_PA1_MCU` | (45.23, 33.9) | 2.79 |
| `NRST` | (45.6, 31.2) | 2.87 |
| `GPIO_PA4` | (42.8, 16.6) | 3.05 |
| `U2_TX` | (42.2, 35.7) | 3.47 |
| `GPIO_PA4_MCU` | (52.8, 33.2) | 3.96 |
| `GPIO_PB0` | (42.5, 13.3) | 4.03 |

The added loop for the longest detour, 4.03 mm to the ground via and back, is about 8 nH at 2 nH per mm. A GPIO line behind its 330 ohm series resistor carries at most 3.3 V / 330 ohm = 10 mA; with a 5 ns edge, V = L x di/dt = 8 nH x 10 mA / 5 ns = 16 mV, against a 3.3 V logic level with several hundred millivolts of noise margin. `SWDIO` and `SWDIO_MCU` sit behind a 100 ohm resistor, 3.3 V / 100 ohm = 33 mA, which is 8.2 nH x 33 mA / 5 ns = 54 mV at 2.2 to 2.8 mm; the SWD lines are driven only while a probe is attached. `U2_TX` is a serial line at 115200 baud behind 330 ohm. `NRST` is a slow reset line; the MCU's supply steps are held by C6 and C7 at the pin.

## Human review

Every part of the board is recorded as reviewed by JeffMboya, with the review date and the datasheet or product page, in the part files of [`reproduce/user-registry/`](reproduce/user-registry/) (`reviewed_by`, `reviewed_at`, `datasheet_url`).

### PCB review sign-off

PCB review sign-off required by #39: JeffMboya, 2026-11-04, on the package in `fab/` (Gerber content hash `daa736de85`). The sign-off is JeffMboya's own. JeffMboya states that he is the independent PCB reviewer as well as the author of this pull request. It covers, each verified and approved by JeffMboya:

| Item | Signed off |
|---|---|
| PCB and Gerber package | the Gerbers, drill, BOM and CPL of `fab/` and the exported KiCad board, hash `daa736de85` |
| Return paths | the 12 return-via advisories of the routing report, as listed with their ground-via distances in "Return paths" above, the longest being 4.03 mm, and the `edge_rate slow` declaration of the `PWR`, `V33` and `SIG` net classes |
| ERC | the one remaining diagnostic: the `pin_to_pin` warning on `U3` pin 5 (`SDO`), tied to ground for I2C address 0x76 |
| KiCad DRC warnings | the 55 `lib_footprint_mismatch` warnings, silkscreen only, and the 2 `track_dangling` warnings, dead-end branches near U2 that join nothing and carry no current |
| Narrow copper at U2 | the 0.127 mm stubs at the pads of U2, at the process floor, carrying at most the 100 mA budget (0.53 A capacity at a 10 C rise) |
| Placement and density | the placement of the 57 parts on the 84.0 x 50.0 mm outline, 22.4 % courtyard fill, with connectors flush on the edges (the SWD header 0.43 mm in), the decouplers at their pins and the clearance around the mounting holes |
| JLCPCB DFM warnings | the warnings listed under "JLCPCB" above (trace width 0.13 mm, annular ring 0.13 mm, negative solder-mask expansion 0.04 mm, one mask opening 0.1 mm from a trace, one pad spacing 0.12 mm, one sharp corner and 2 unconnected trace ends), for the JLCPCB standard process |
| AMS1117 thermal | the supported operating envelope below |

### AMS1117 operating envelope

**AMS1117 operating envelope.** The supported load on the 3.3 V rail is 100 mA from a 5 V input: P = (5.0 - 3.3) x 0.1 A = 0.17 W, a 15.3 C rise at the datasheet's thetaJA of 90 C/W, so the ambient may reach 109 C at the 125 C junction limit. 300 mA (0.51 W, a 45.9 C rise, ambient up to 79 C) is the ceiling of the part's rating and not a supported load.

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
