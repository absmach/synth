# Irrigation controller (Class A, case 1)

A two-zone irrigation controller powered from 12 V DC: an input fuse, a TVS and an ideal-diode reverse-polarity stage, an MP2307 buck to 3.3 V, an STM32G030 microcontroller, two low-side solenoid drivers, two switched resistive soil probes, a flow-pulse input, an I2C header with an ambient light sensor and an EEPROM, an SWD header, and rail decoupling, bleeders, snubbers, indicator LEDs and test points. Two copper layers, JLCPCB process, 79.2 x 70.7 mm, 127 placed items (107 parts and 20 mounting holes and test points). There is no humidity sensor on the board: a breakout attaches to the I2C header at address 0x44.

## The board

| Group | Parts |
|---|---|
| Power input | `J1` JST XH 2-pin supply connector; `F1` Littelfuse 1812L150/16DR resettable fuse (1.5 A hold); `D_TVS` SMAJ13A (13.0 V standoff, 21.5 V clamp at 18.6 A); the ideal-diode stage `U_IDEAL` Texas Instruments LM74700-Q1 (SOT-23-6) driving `Q1` Diodes DMT6007LFG (60 V, 8.5 mOhm maximum at 4.5 V gate drive) with `C_VCAP` and `C_VIN` (100 nF each) at the controller and `C_BULK` (100 uF) on the output; test points `TP_VIN`, `TP_12V` |
| 3.3 V buck | `U1` MPS MP2307DN, input capacitors `C_IN1` and `C_IN2` (10 uF), inductor `L1` Bourns SRN6045TA-100M (10 uH), output capacitors `C_OUT1` and `C_OUT2` (22 uF), feedback divider `R_FBT` 26.1 k and `R_FBB` 10 k, compensation `R_COMP` 5.6 k and `C_COMP` 4.7 nF, soft-start `C_SS`, enable divider `R_EN`; test points `TP_3V3`, `TP_GND` |
| MCU | `U2` STMicroelectronics STM32G030K8T6 (LQFP-32), crystal `Y1` JYJE 3TJ480000FYLBC (8 MHz) with `C_X1` and `C_X2` (6.2 pF), supply capacitors `C_VDD1` and `C_VDD2`, reset network `R_NRST`, `C_NRST` and the tactile switch `SW_RESET`, the 5-pin SWD header `J_SWD`; test points `TP_NRST`, `TP_SWDIO`, `TP_SWCLK` |
| Valves 1 and 2 | per valve: a JST XH 2-pin connector (`J_VALVE1`, `J_VALVE2`), a low-side AO3400A N-MOSFET (`Q_V1`, `Q_V2`) with a 100 ohm gate resistor and a 100 k pull-down, a 1N5819HW flyback diode (`D_FLY1`, `D_FLY2`) and an RC snubber (100 ohm, 10 nF) |
| Soil probes 1 and 2 | per probe: a JST PH 3-pin connector (`J_SOIL1`, `J_SOIL2`), a Si2301CDS P-MOSFET that switches the excitation (`Q_E1`, `Q_E2`), the sense divider and a 10 nF filter |
| Flow input | `J_FLOW` JST PH 3-pin, pull-up `R_FPU`, series resistor `R_FS` and a 3.3 V zener `D_FZ` that clamps the pulse input, filter `C_FL` |
| I2C | `J_I2C` JST PH 4-pin, 4.7 k pull-ups `R_SDA` and `R_SCL`, `U4` Broadcom APDS-9301 ambient light sensor, `U5` Microchip 24LC256 EEPROM, each with a 100 nF capacitor |
| Indicators | `D_PWR` and `D_N2` green (Lite-On LTST-C190KGKT), `D_VALVE` and `D_N1` amber (KENTO KT-0603Y), each with a series resistor |
| Rail decoupling and bleeders | the `+3V3` decoupling group (22 parts: capacitors from 100 nF to 10 uF and the bleeders `R_N1` and `R_N2`) and the 12 V decoupling group (7 parts, with the bleeder `R_B1`) |
| Mechanical | four M2.5 mounting holes, one in each corner; test points on the nets listed above |

The qualification verdict, measurements, evidence and sign-off are in [`QUALIFICATION.md`](QUALIFICATION.md); how the board was made is in [`PROVENANCE.md`](PROVENANCE.md). Gerber content hash `a26463bd09`.
