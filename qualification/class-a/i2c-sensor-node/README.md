# I2C sensor node with a 5 V JST input (Class A, case 2)

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from 5 V on a JST XH connector, with an AMS1117-3.3 making the 3.3 V rail. The board has two copper layers with a GND pour on each and measures 84.0 x 50.0 mm.

Case 2 is claimed for a plain default export of a protected, low-speed board: every external line has a series resistor and a 3.3 V clamp, and every net is a static or slow line. No firmware has run on the board and no board has been built.

The prompt used to produce the board is in [`PROMPT.md`](PROMPT.md).

## The board

### Parts

| Group | Parts |
|---|---|
| 5 V input | `J1` JST S2B-XH-A-1; `F1` Littelfuse 1812L050/60MR (0.5 A hold); `D1` 1N5819HW Schottky against reverse polarity; `D4` Nexperia PTVS6V0S1UR (6 V standoff, 10.3 V maximum clamp per the distributor listings) |
| 3.3 V | `U1` AMS1117-3.3 (SOT-223); `C1`, `C2` 10 uF on the input; `C3` 47 uF tantalum, `C4` 10 uF and `C5` 100 nF on the output; power LED `D15` with `R24` |
| Microcontroller | `U2` STM32G030F6P6 (TSSOP-20); `C6` 100 nF and `C7` 4.7 uF on VDD; `R3` 10 k, `C8` 100 nF and `SW1` on NRST; `J4` 1x5 SWD header behind `R13`, `R14` (100 ohm), with 3.3 V zeners `D8`, `D9`, `D19` |
| I2C sensing | `U3` BME680 and `U5` 24LC256 on one bus (PB7 SDA, PB6 SCL), pull-ups `R9`, `R10` (4.7 k), decoupling `C10` to `C12` |
| Serial connector | `J3` JST S4B-PH-K-S: UART TX through `R11` (330 ohm) and `D7`, RX through `R12` (1 k) and `D24`, 3.3 V sense and ground |
| GPIO header | `J5` 1x12 header: eight MCU lines (ADC, SPI and timer pins), each behind 330 ohm (`R15` to `R22`) with a 3.3 V zener (`D10` to `D13`, `D20` to `D23`); a 3.3 V sense line through `D14` and `R23` (100 ohm) |
| Mounting | `H2`, `H3`: two M2.5 non-plated holes at opposite corners |

### Layout

| Item | Value |
|---|---|
| Outline | 84.0 x 50.0 mm |
| Copper layers | 2 (F.Cu, B.Cu) |
| Stackup | 1.6 mm board thickness |
| Parts in the BOM | 57 lines: 55 parts and 2 non-plated mounting holes (`H2`, `H3`) |
| Parts placed in the CPL | 55, all on the top side |
| Nets | 35 named nets, counted on the board file |
| Track segments | 422 |
| Vias | 142 |
| Track length | 1239.4 mm |
| Zones and planes | GND pour on F.Cu and on B.Cu; keepout zones for the `H2` and `H3` screw heads (no tracks, vias or copper pour) |
| Net classes | `PWR` 0.5 mm, `V33` 0.4 mm, `GNDC` 0.4 mm and `SIG` 0.25 mm, declared in the source; `PWR`, `V33` and `SIG` declare `edge_rate slow` |
| Fab package | `fab/gerbers.zip` (copper, mask, paste, silkscreen, outline, drill and job file), `fab/bom.csv`, `fab/cpl.csv`; KiCad project in `kicad/`, STEP model in `models/`, images in `images/` |
| Source | `i2c_sensor_node.synth` with the placement sidecar `i2c_sensor_node.placement.layout.toml`; made with `synth mcp` from a prompt and exported through the Synth MCP server (the `synth_export` tool) |

### Checks

| Check | Result | Notes |
|---|---|---|
| KiCad DRC (10.0.8, --refill-zones) | 0 errors, 57 warnings, 0 unconnected | Warnings: 55 lib_footprint_mismatch, 2 track_dangling |
| KiCad ERC (10.0.8) | 0 errors, 6 warnings, 0 unconnected | Warnings: 5 lib_symbol_issues, 1 pin_to_pin |
| Signal class | No clock, differential or switching-node net | Every net is a static or slow line, as declared in the source net classes |

### Design figures

| Area | Item | Value | Basis |
|---|---|---|---|
| Power | 3.3 V rail load budget | 100 mA from 5 V | Stated in the source |
| Thermal | `U1` dissipation at 100 mA | (5.0 - 3.3) x 0.1 A = 0.17 W, a 15.3 C rise | Calculated with thetaJA 90 C/W from the AMS1117 datasheet as cited in the source; datasheet not opened |
| Thermal | `U1` at 300 mA | 0.51 W, 45.9 C rise; the part's ceiling, not a supported load | Calculated with thetaJA 90 C/W |
| Power | `C3` output capacitor | 47 uF tantalum, where 22 uF is required | Stated in the source |
| Power | Power LED current | 1.3 to 2.1 mA through 680 ohm at VF 1.9 to 2.4 V | Calculated |
| Protection | Input order | Fuse `F1` first, then reverse-polarity diode `D1`; `D4` clamps +5V_PROT behind `D1`, so a reversed supply never forward-biases it | Stated in the source |
| Protection | 5 V fault on a 330 ohm line | (5 - 3.3) / 330 = 5.2 mA, 17 mW into its zener | Calculated |
| Protection | 5 V fault on the 1 k UART RX line | 1.7 mA, 5.6 mW into `D24` | Calculated |
| Protection | 5 V fault on a 100 ohm SWD line | (5 - 3.3) / 100 = 17 mA, 56 mW into `D8`, `D9` or `D19` | Calculated |
| Protection | 3.3 V sense line | At most 3 mA | Stated in the source |
| Signal integrity | I2C addresses | `U3` SDO (pin 5) tied to ground selects 0x76; `U5` A2:A0 low selects 0x50; no collision | Stated in the source |
| Signal integrity | I2C pull-ups | One 4.7 k pair for the whole bus | Stated in the source |
| Signal integrity | Unused pins | PA9 and PA10 (pins 16, 17) are unconnected | Stated in the source |
| Signal integrity | BOOT0 | Shares PA14 and SWCLK; the default option bytes ignore it | Stated in the source |
| Assembly | SWD header `J4` | Pin 1 3.3 V sense, 2 SWDIO, 3 SWCLK, 4 GND, 5 NRST; use a J-Link or ST-LINK at 3.3 V logic | Stated in the source |
| Assembly | Reset | `SW1` and NRST on `J4` reset the chip | Stated in the source |
| Assembly | UART connector `J3` | Pin 1 GND, 2 UART_TX, 3 UART_RX, 4 3.3 V sense; host logic 3.3 V to 5 V | Stated in the source |
| Assembly | GPIO header `J5` | Pin 1 3.3 V sense, 2 GND, 3 PA0, 4 PA1, 5 PA4, 6 GND, 7 PA5, 8 PA6, 9 PA7, 10 GND, 11 PB0, 12 PB9 | Stated in the source |

### Sourcing

| Item | Status | Notes |
|---|---|---|
| `U2` | STM32G030F6P6TR, LCSC `C529330` | Tape-and-reel code of the same TSSOP-20 part |
| `SW1` | C&K PTS645SM43SMTR92LFS, LCSC `C221880` | JLCPCB listing; re-check stock before ordering |
| `J5` | XFCN PZ254V-11-12P, LCSC `C2840012` | JLCPCB listing |
| `J4` | Hand-fitted, Farnell 1022253 | Harwin M20-9990546; also RS 681-2985 |
| `D4` | Farnell (CPC) SC21458 | No LCSC listing found |
| `H2`, `H3` | Board feature, no part | Non-plated holes |

## Not verified by Synth

| Area | What is not verified |
|---|---|
| Routing reports | The routing report and the return-via advisory list are not in this folder. |
| Fabrication | The JLCPCB DFM report and quote for this package are not in this folder. |
| Reproducibility | The three-export comparison and the Gerber content hash are not in this folder. |
| Narrow copper | The router draws 0.127 mm stubs at the pads of `U2`, below the class widths. The current they carry is not checked. |
| Part data | Several BOM lines cite distributor listings only (for example `F1`, `D4`, `J5` and the `U1` thermal figures). Those datasheets were not opened. |
| Firmware and bring-up | No firmware has run and no board has been built. |
