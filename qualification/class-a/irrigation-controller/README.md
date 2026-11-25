# Irrigation controller (Class A, case 1)

A two-zone irrigation controller powered from 12 V DC. It has an input fuse, a TVS, an ideal-diode reverse-polarity stage, an MP2307 buck to 3.3 V, an STM32G030 microcontroller, two low-side solenoid drivers, two soil probes with AC excitation, a flow-pulse input and an I2C header with an ambient light sensor and an EEPROM. The board has two copper layers, uses the JLCPCB process and measures 79.2 x 70.7 mm.

The board answers case 1 of the Class A qualification set: a mixed power, MCU and field-wiring design. It claims only what the tables below show from the board files, the measured KiCad checks and the cited datasheets. No figure is measured on hardware.

The prompt used to produce the board is in [`PROMPT.md`](PROMPT.md).

## The board

### Parts

| Group | Parts |
|---|---|
| Power input | `J1` JST XH 2-pin supply connector; `F1` Littelfuse 1812L150/16DR resettable fuse (1.5 A hold); `D_TVS` SMAJ13A (13.0 V standoff, 21.5 V clamp at 18.6 A) |
| Ideal-diode stage | `U_IDEAL` Texas Instruments LM74700-Q1 (SOT-23-6) driving `Q1` Diodes DMT6007LFG (60 V, 8.5 mOhm maximum at 4.5 V gate drive); `C_VCAP` and `C_VIN` (100 nF each) at the controller; `C_BULK` (100 uF) on the output |
| 3.3 V buck | `U1` MPS MP2307DN; input capacitors `C_IN1` and `C_IN2` (10 uF); inductor `L1` Bourns SRN6045TA-100M (10 uH); output capacitors `C_OUT1` and `C_OUT2` (22 uF); feedback divider `R_FBT` 26.1 k and `R_FBB` 10 k; compensation `R_COMP` 5.6 k and `C_COMP` 4.7 nF; soft-start `C_SS`; enable divider `R_EN` |
| MCU | `U2` STMicroelectronics STM32G030K8T6 (LQFP-32); supply capacitors `C_VDD1` and `C_VDD2`; reset network `R_NRST`, `C_NRST` and the tactile switch `SW_RESET`; 5-pin SWD header `J_SWD` |
| MCU clock | 32.768 kHz LSE crystal `Y1` Epson FC-135 32.7680KF-AG0 (7 pF load) with `C_X1` and `C_X2` (4.3 pF); HSI16 is the system clock, because the LQFP-32 has no HSE crystal oscillator |
| Valves 1 and 2 | Per valve: a JST XH 2-pin connector (`J_VALVE1`, `J_VALVE2`); a low-side AO3400A N-MOSFET (`Q_V1`, `Q_V2`) with a 100 ohm gate resistor and a 100 k pull-down; a 1N5819HW flyback diode (`D_FLY1`, `D_FLY2`); an RC snubber (100 ohm, 10 nF) |
| Valve short-circuit sense | Per valve: a 0.1 ohm shunt (`R_SH1`, `R_SH2`) in the MOSFET source, read on `PA1` and `PA0` through 1 k and 10 nF (`R_IS`, `C_IS`) |
| Soil probes 1 and 2 | Per probe: a JST PH 3-pin connector (`J_SOIL1`, `J_SOIL2`); AC excitation from two GPIOs through 10 k resistors (`R_S1`, `R_D1`, `R_S2`, `R_D2`); both probe nodes sampled through 1 k and 10 nF filters |
| Flow input | `J_FLOW` JST PH 3-pin; pull-up `R_FPU`; series resistor `R_FS`; 3.3 V zener `D_FZ` that clamps the pulse input; filter `C_FL` |
| I2C | `J_I2C` JST PH 4-pin; 4.7 k pull-ups `R_SDA` and `R_SCL`; `U4` Broadcom APDS-9301 ambient light sensor and `U5` Microchip 24LC256 EEPROM, each with a 100 nF capacitor |
| Humidity sensor | None on the board. A breakout attaches to the I2C header at address 0x44. |
| Field-wiring protection | PRTR5V0U2X ESD arrays at the soil and I2C connectors (`D_ESD_S1`, `D_ESD_S2`, `D_ESD_I2C`); SMAJ15A TVS on the flow input (`D_ESD_FL`) |
| Indicators | `D_PWR` green (Lite-On LTST-C190KGKT); `D_VALVE` and `D_N1` amber (KENTO KT-0603Y); each with a series resistor |
| Test points | `TP_VIN`, `TP_12V`, `TP_3V3`, `TP_GND`, `TP_NRST`, `TP_SWDIO`, `TP_SWCLK`, `TP_SDA`, `TP_SCL`, `TP_FLOW`, `TP_SOIL1`, `TP_SOIL2`, `TP_V1`, `TP_V2`, `TP_N1`, `TP_N2` |
| Mechanical | Four M2.5 mounting holes `H1` to `H4`, one in each corner |

### Layout

| Item | Value |
|---|---|
| Outline | 79.2 x 70.7 mm |
| Copper layers | 2 |
| Stackup | 1.6 mm board thickness; JLCPCB process; no layer stackup is defined in the board file |
| Parts in the BOM | 107 lines: 87 parts and 20 board features (4 mounting holes, 16 test points) |
| Parts placed in the CPL | 87 |
| Nets | 51 |
| Track segments | 648 |
| Vias | 474 |
| Track length | 1663.8 mm |
| Zones and planes | GND pour on F.Cu and on B.Cu; four keepout zones at the screw heads of `H1` to `H4` on both layers |
| Fab package | `fab/gerbers.zip` (copper, mask, paste, silkscreen, edge cuts, PTH and NPTH drill files and maps, job file), `fab/bom.csv`, `fab/cpl.csv`; STEP model in `models/`; images and schematic PDF in `images/` |

### Checks

| Check | Result | Notes |
|---|---|---|
| KiCad DRC (10.0.8, --refill-zones) | 0 errors, 0 unconnected, 8 warnings | 7 `silk_over_copper`, 1 `silk_overlap` |
| KiCad ERC (10.0.8) | 0 errors, 0 unconnected, 28 warnings | 27 `lib_symbol_issues` (library symbol and footprint-link warnings), 1 `label_multiple_wires` |

### Design figures

| Area | Item | Value | Basis |
|---|---|---|---|
| Power | Supply range | 12 V +/-5 % (10.8 to 12.6 V) | Stated design assumption |
| Power | Load budget | 1.18 A at 12 V: two valves 1.0 A, buck input and flow sensor 0.18 A. 1.25 A at 13.0 V. | Calculated |
| Power | Buck compensation | `R_COMP` 5.6 k and `C_COMP` 4.7 nF put the crossover at 24 kHz (44 uF output) to 34 kHz (about 31 uF after DC-bias derating). This is 0.07 to 0.10 of the 340 kHz switching frequency. | Calculated; MP2307 datasheet |
| Power | Ideal-diode capacitors | `C_VCAP` 100 nF (at least 0.1 uF and 10 x the 2.09 nF Ciss of `Q1`); `C_VIN` 100 nF (at least 22 nF); `C_BULK` 100 uF (at least 100 nF) | LM74700-Q1 and DMT6007LFG datasheets |
| Power | `Q1` conduction | 10 mV drop and 12 mW at 1.18 A | Calculated from the 8.5 mOhm maximum |
| Protection | Surge | At most 14 A peak (10/1000 us), 75 % of the SMAJ13A's 18.6 A rating | SMAJ13A datasheet; calculated |
| Protection | Open-circuit surge covered | 21.5 V + 14 A x R_source: 35.5 V at 1 ohm, 49.5 V at 2 ohm | Calculated |
| Protection | TVS standoff | 12.6 V is 0.4 V (3.2 %) below the 13.0 V standoff; leakage at most 1 uA there; breakdown 14.40 V minimum at 1 mA | SMAJ13A datasheet |
| Protection | Clamp margin, `U1` | At 21.5 V: 1.5 V below the MP2307's 23 V recommended input; 4.5 V below its 26 V absolute maximum | MP2307 datasheet; calculated |
| Protection | Clamp margin, `U_IDEAL` | At 21.5 V: 43.5 V below its 65 V maximum | LM74700-Q1 datasheet; calculated |
| Protection | Clamp margin, `Q1` | At 21.5 V: 38.5 V below its 60 V rating | DMT6007LFG datasheet; calculated |
| Protection | Clamp margin, capacitors | At 21.5 V: `C_BULK`, `C_IN1`, `C_IN2` are 3.5 V below their 25 V rating | `fab/bom.csv` ratings; calculated |
| Protection | Valve short circuit | `R_SH1`, `R_SH2` (0.1 ohm, 1 W) give 50 mV at 0.5 A and 0.15 V at the 1.5 A fuse hold. Firmware trips at 0.1 V (1.0 A). | Calculated |
| Protection | Flow input clamp | `D_FZ` 3.3 V zener behind `R_FS` 2.2 k: 4.4 mA and 14 mW at 13.0 V, in a 500 mW part | Calculated |
| Thermal | Fuse derating | `F1` holds 1.5 A at 20 C, 1.25 A at 40 C, 1.10 A at 50 C and 0.60 A at 85 C | 1812L150/16DR datasheet |
| Thermal | Ambient maximum | 40 C: the 1.25 A load at 13.0 V equals the 1.25 A hold at 40 C | Calculated |
| Thermal | Shunt dissipation | 0.225 W in one shunt at 1.5 A, in a 1 W part | Calculated |
| Valve drive | Valve current | Coil at most 0.5 A. The AO3400A (5.7 A listing) and the 1N5819HW (1 A) are above it. | Stated assumption; part listings |
| Valve drive | Valve net width | 0.8 mm (net class `PWR12`) | Net class in `irrigation_controller.synth` |
| Sensing | Soil probe formula | With Vd = V(A) - V(B) in one phase, Rprobe = 20 k x Vd / (3.3 V - Vd). This cancels the drive level and the 10 k tolerances. | Calculated |
| Sensing | Soil probe range | Vd is 0.157 V at 1 k and 3.0 V at 200 k | Calculated |
| Sensing | Soil probe DC | Both drive pins idle low or Hi-Z, so the probe sees no DC | Circuit in `irrigation_controller.synth` |
| Clock | LSE crystal load | CL = C1 x C2 / (C1 + C2) + 5 pF stray. 4.3 pF capacitors give 7.15 pF against the crystal's 7 pF. | Calculated; FC-135 datasheet |
| Clock | LSE gm_crit | gm_crit = 4 R1 (2 pi f)^2 (C0 + CL)^2 = 4 x 70 kOhm x (2.059e5 rad/s)^2 x (8.15 pF)^2 = 0.79 uA/V | Calculated; FC-135 typical ESR |
| Clock | LSE drive setting | Gmcritmax is 0.5, 0.75, 1.7 and 2.7 uA/V for LSEDRV low, medium low, medium high and high. These include the safety margin, so firmware must set LSEDRV to medium high (1.7 uA/V). | STM32G030 DS12991 Rev 6, Table 37 |
| Signal integrity | I2C bus capacitance | 4.7 k pull-ups allow up to 251 pF at 100 kHz and 75 pF at 400 kHz | NXP UM10204 rise times; calculated |

### Sourcing

| Item | Status | Notes |
|---|---|---|
| BOM parts | 87 of 87 sourced | 79 from LCSC; others from RS, DigiKey, Farnell, JLCPCB and Mouser |
| Board features | 20, no part | `H1` to `H4` and the 16 test points |
| Hand-fitted parts | 3 | `C_BULK` (through-hole), `J_SWD`, `U4` |
| Datasheets not opened | 6 lines | `C_BULK`, `C_IN1`, `C_IN2`, `R_COMP`, `U1`, `U4`: the BOM link is recorded but marked not opened |
| `L1` stock | Not read | Order number taken from the JLCPCB listing |

## Not verified by Synth

| Area | What is not verified |
|---|---|
| Hardware | No board has been built, powered or run with firmware. Every figure here is calculated from datasheets and listings. |
| Valve short circuit | Firmware must read the shunt on `PA1` or `PA0` and turn the valve off above 0.1 V (1.0 A). Until it does, a shorted coil cable is cleared only when `F1` trips. |
| Trip speed | The trip depends on firmware polling, so the shunt and MOSFET carry the fault current for one polling interval. There is no hardware comparator into a GPIO interrupt or timer break input. |
| Valve current | The 0.5 A coil current is a stated assumption, not a measurement. |
| Connectors | `J1`, `J_VALVE1` and `J_VALVE2` are all 2-pin JST XH, so the 12 V input and a valve can be cross-mated. Keyed or different-pitch connectors are not fitted. |
| Surge | The input protection holds only for a 12 V +/-5 % supply and a surge of at most 14 A peak (10/1000 us). The clamp is 1.5 V below the MP2307's 23 V recommended input. |
| Fuse derating | The `F1` hold current derates with temperature, so the board is specified to 40 C ambient. |
| Crystal stray | The 5 pF stray capacitance is an assumption. |
| Crystal ESR | The gm_crit figure uses the crystal's typical 70 kOhm ESR, because its datasheet states no maximum. |
| Oscillator start-up | No oscillator start-up has been measured. |
| Buck output | The 31 uF derated output capacitance is an assumption, not a measurement. |
| I2C speed | The bus is specified for 100 kHz. 400 kHz holds only while the breakout and its cable add no more than 75 pF. |
| Fabrication checks | No JLCPCB DFM, BOM-matching or quote run is recorded for this package. |
