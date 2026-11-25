# I2C sensor node with a 5 V JST input (Class A, case 2)

A field sensor node: an STM32G030F6P6 microcontroller reads a Bosch BME680 (temperature, humidity, pressure and gas) and a 24LC256 EEPROM over one I2C bus. It runs from 5 V on a JST XH connector, with an AMS1117-3.3 making the 3.3 V rail. Two copper layers with a GND pour on each, 84.0 x 50.0 mm, 57 parts including two mounting holes.

The design is `i2c_sensor_node.synth` with its placement sidecar `i2c_sensor_node.placement.layout.toml`. It was made with `synth mcp` from a prompt and exported through the Synth MCP server (the `synth_export` tool). The folder holds the KiCad project, the fabrication package in `fab/`, the STEP model and the images.

Case 2 is claimed for a plain default export of a protected, low-speed board: every external line has a series resistor and a 3.3 V clamp, and every net is a static or slow line. No firmware has run on the board and no board has been built.

Jeff Mboya is the author of this board and has self-reviewed it. The independent PCB review sign-off required by #39 is open.

## The board

| Group | Parts |
|---|---|
| 5 V input | `J1` JST S2B-XH-A-1; `F1` Littelfuse 1812L050/60MR (0.5 A hold); `D1` 1N5819HW Schottky against reverse polarity; `D4` Nexperia PTVS6V0S1UR (6 V standoff, 10.3 V maximum clamp per the distributor listings) |
| 3.3 V | `U1` AMS1117-3.3 (SOT-223); `C1`, `C2` 10 uF on the input; `C3` 47 uF tantalum, `C4` 10 uF and `C5` 100 nF on the output; power LED `D15` with `R24` |
| Microcontroller | `U2` STM32G030F6P6 (TSSOP-20); `C6` 100 nF and `C7` 4.7 uF on VDD; `R3` 10 k, `C8` 100 nF and `SW1` on NRST; `J4` 1x5 SWD header behind `R13`, `R14` (100 ohm), with 3.3 V zeners `D8`, `D9`, `D19` |
| I2C sensing | `U3` BME680 and `U5` 24LC256 on one bus (PB7 SDA, PB6 SCL), pull-ups `R9`, `R10` (4.7 k), decoupling `C10` to `C12` |
| Serial connector | `J3` JST S4B-PH-K-S: UART TX through `R11` (330 ohm) and `D7`, RX through `R12` (1 k) and `D24`, 3.3 V sense and ground |
| GPIO header | `J5` 1x12 header: eight MCU lines (ADC, SPI and timer pins), each behind 330 ohm (`R15` to `R22`) with a 3.3 V zener (`D10` to `D13`, `D20` to `D23`); a 3.3 V sense line through `D14` and `R23` (100 ohm) |
| Mounting | `H2`, `H3`: two M2.5 non-plated holes at opposite corners |

- Program and debug over `J4` with a J-Link or ST-LINK at 3.3 V logic: pin 1 is the 3.3 V sense, 2 SWDIO, 3 SWCLK, 4 GND, 5 NRST. `SW1` and NRST on `J4` reset the chip.
- PA9 and PA10 are unconnected. BOOT0 shares PA14 and SWCLK; the default option bytes ignore it.
- `U3` pin 5 (`SDO`) is tied to ground to select I2C address 0x76.
- A 5 V fault on a protected line drives (5 - 3.3) / 330 = 5.2 mA, 17 mW, into its zener. The 3.3 V sense line delivers at most 3 mA.
- Net classes `PWR` 0.5 mm, `V33` 0.4 mm, `GNDC` 0.4 mm and `SIG` 0.25 mm are declared in the source. `PWR`, `V33` and `SIG` declare `edge_rate slow`: the board has no clock, differential or switching-node net.
- At the 100 mA budget of the 3.3 V rail from 5 V, `U1` dissipates 0.17 W, a 15.3 C rise at the datasheet's 90 C/W. 300 mA (0.51 W, 45.9 C rise) is the part's ceiling, not a supported load.
- In `fab/bom.csv`, `U2` is the STM32G030F6P6TR (`C529330`), `SW1` the C&K PTS645SM43SMTR92LFS (`C221880`) and `J5` the XFCN PZ254V-11-12P (`C2840012`). `J4` is hand-fitted from Farnell 1022253.

## Not verified by Synth

- **DRC, ERC and routing results:** the KiCad DRC and ERC reports, the routing report and the return-via advisory list are no longer in the tree. This folder does not check them.
- **Fabrication:** the JLCPCB DFM report and quote for this package are no longer in the tree.
- **Reproducibility:** the three-export comparison and the Gerber content hash are no longer in the tree.
- **Narrow copper:** the router draws 0.127 mm stubs at the pads of `U2`, below the class widths. The current they carry is not checked here.
- **Part data:** several BOM lines cite distributor listings only (for example `F1`, `D4`, `J5` and the `U1` thermal figures); those datasheets were not opened.
- **Firmware and bring-up:** no firmware has run and no board has been built.
- **Review:** the board has a self-review only. The independent PCB review sign-off required by #39 is open.
