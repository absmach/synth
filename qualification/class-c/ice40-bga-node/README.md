# iCE40HX4K BGA node with a USB-C input and an MCU (Class C, case 1)

An iCE40HX4K FPGA in a 121-ball, 0.8 mm-pitch BGA with its supply chain, an SPI boot flash, one 12-pin header and four status LEDs, powered from a USB-C receptacle, with an STM32F103C8 companion MCU that programs the flash over USB, on six copper layers. It answers #328 under #40: a board with a BGA part, escaped under the pads, with via and fanout rules, made by an agent driving `synth mcp`.

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS) with `R_CC1` and `R_CC2`, 5.1 k pull-downs on CC1 and CC2; `F1` Bourns MF-NSMF050-2 (0.5 A hold, 1 A trip, 13.2 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown, 10.3 V clamp at 38.8 A); `Q1` Vishay SI2301CDS P-MOSFET reverse-polarity stage; `R3`, `C1` |
| USB data | `R25` and `R26`, 22 ohm series resistors in D+ and D- |
| MCU | `U5` STMicroelectronics STM32F103C8T6 (LQFP-48) on `+3V3_MCU`, which comes from `+3V3` through the ferrite bead `FB2`; supply decoupling `C27` to `C32` (five 100 nF and 4.7 uF); reset network `R21`, `C33` and the tactile switch `SW1`; `R22` pulls BOOT0 low; the crystal `Y1` with `C34` and `C35`; the 5-pin SWD header `J4` (3V3, SWDIO, SWCLK, NRST, GND) |
| 3.3 V | `U3` AMS1117-3.3 with 4 x 10 uF |
| 1.2 V core | `U4` Texas Instruments TLV70012DDCR, TSOT-23-5, with 1 uF in and 1 uF + 4.7 uF out; PLL supply `VCCPLL` through the ferrite bead `FB1` (Murata BLM18AG601SN1D) and `C20` to `C22` |
| FPGA | `U1` Lattice iCE40HX4K-BG121 (`VPP_FAST` unconnected), every supply ball decoupled |
| Configuration | `U2` Winbond W25Q128JVSIQ, SOIC-8 208 mil (5.3 mm body), master SPI; `CRESET_B` pulled up, `CDONE` with its LED |
| I/O | `J3` JXTCONN JXT-PZ2.54-1X12P-A1, 12 positions (3.3 V, ground and four FPGA I/O through 100 ohm series resistors) |
| Indicators and test points | four LEDs in one row, test points on 5 V, 3.3 V, 1.2 V, `CDONE` and `CRESET_B`, two M2.5 mounting holes at opposite corners (bottom left and top right) |

The MCU's job is to program the flash. `U5` streams the bitstream from the host over USB to `U2` over SPI1 (`PA4` to `PA7` to the flash's chip select, clock and data pins) and drives `CRESET_B` and `CDONE` from `PB0` and `PB1`. The iCE40HX4K bitstream is 135,183 bytes (1,081,464 bits) according to Lattice's iCE40 Programming and Configuration technical note FPGA-TN-02001-3.4, Table 9.2, which is more than the STM32F103C8's 64 KB of flash, so the 128 Mbit flash `U2` stays the bitstream store. The board carries no firmware.

The crystal `Y1` is the JYJE 3TJ480000FYLBC (LCSC `C2924620`): 8 MHz, load capacitance 8 pF, ESR 150 ohm maximum and C0 5 pF maximum, from the manufacturer's specification. The load capacitors are `C = 2 x (CL - Cstray) = 2 x (8 - 3) = 10 pF` each, with 3 pF of stray capacitance assumed. The start-up criterion `gm_crit = 4 x ESR x (2 pi f)^2 x (C0 + CL)^2` is 0.26 mA/V at the maximum ESR and C0.

The flash's package was checked against the manufacturer listings (LCSC, Arrow, RS, JITX): the W25Q128JVSIQ is an 8-pin SOIC of 208 mil, and the board's footprint is `Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`. The TLV70012 is listed at LCSC only in the DDC (thin) package, which the board's footprint `TSOT-23-5` matches; its pinout is the same as the DBV package (1 IN, 2 GND, 3 EN, 4 NC, 5 OUT).

The qualification verdict, measurements, evidence and sign-off are in [`QUALIFICATION.md`](QUALIFICATION.md); how the board was made is in [`PROVENANCE.md`](PROVENANCE.md). Gerber content hash `ceeb7444dd`.
