# iCE40HX4K BGA node with a USB-C power input (Class C, case 1)

An iCE40HX4K FPGA in a 121-ball, 0.8 mm-pitch BGA with its supply chain, an SPI boot flash, one 12-pin header and four status LEDs, powered from a USB-C receptacle (power only) and programmed through a 6-pin header, on six copper layers. It answers #328 under #40: a board with a BGA part, escaped under the pads, with via and fanout rules, made by an agent driving `synth mcp`.

The prompt used to produce the board is in [`PROMPT.md`](PROMPT.md).

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS) with `R_CC1` and `R_CC2`, 5.1 k pull-downs on CC1 and CC2; `F1` Bourns MF-NSMF050-2 (0.5 A hold, 1 A trip, 13.2 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown, 10.3 V clamp at 38.8 A); `C1`; no reverse-polarity stage, since a Type-C receptacle cannot be mated reversed |
| CC protection | `D8` and `D9` TPD1E10B06 bidirectional TVS diodes, one on each CC line at the receptacle, with no rail connection |
| FPGA clock | `X1`, 25 MHz oscillator on global buffer input `GBIN1` (`iot_197_gbin1`), decoupled by `C41`; the iCE40HX has no internal oscillator |
| 3.3 V | `U3` AMS1117-3.3 with 4 x 10 uF |
| 1.2 V core | `U4` Texas Instruments TLV70012DDCR, TSOT-23-5, with 1 uF in and 1 uF + 4.7 uF out; PLL supply `VCCPLL` through the ferrite bead `FB1` (Murata BLM18AG601SN1D) and `C20` to `C22` |
| FPGA | `U1` Lattice iCE40HX4K-BG121 (`VPP_FAST` unconnected), every supply ball decoupled |
| Configuration | `U2` Winbond W25Q128JVSIQ, SOIC-8 208 mil (5.3 mm body), master SPI, with the 6-pin header `J2` for an external programmer; `CRESET_B` pulled up, `CDONE` with its LED driven through N-FET `Q2` so the open-drain `CDONE` net sees only a gate |
| I/O | `J3` JXTCONN JXT-PZ2.54-1X12P-A1, 12 positions (3.3 V, ground and four FPGA I/O through 100 ohm series resistors) |
| Indicators and test points | four LEDs in one row, test points on 5 V, 3.3 V, 1.2 V, `CDONE` and `CRESET_B`, two M2.5 mounting holes at opposite corners (bottom left and top right) |

The header `J2` programs the flash in circuit. Its six pins are `CFG_SS_B` (1, chip select), `CFG_SCK` (2), `CFG_SDI` (3), `CFG_SDO` (4), `CRESET_B` (5) and ground (6). The names are from the FPGA's side: pin 3 is the flash output, so it is MISO at the programmer, and pin 4 is the flash input, so it is MOSI. The header has no 3.3 V pin; a programmer that senses target voltage needs a tap on `TP2`, which is on the 3.3 V rail. Hold `CRESET_B` low with an external programmer, write the flash `U2` over SPI, then release `CRESET_B` so the FPGA loads the bitstream.

The iCE40HX4K bitstream is 135,183 bytes (1,081,464 bits) according to Lattice's iCE40 Programming and Configuration technical note FPGA-TN-02001-3.4, Table 9.2. The 128 Mbit flash `U2` holds it. The board carries no firmware and has no USB data path.

The flash's package was checked against the manufacturer listings (LCSC, Arrow, RS, JITX): the W25Q128JVSIQ is an 8-pin SOIC of 208 mil, and the board's footprint is `Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`. The TLV70012 is listed at LCSC only in the DDC (thin) package, which the board's footprint `TSOT-23-5` matches; its pinout is the same as the DBV package (1 IN, 2 GND, 3 EN, 4 NC, 5 OUT).

## Not verified by Synth

- **USB data:** the board has no USB data path. `J1` is a power-only USB-C input.
- **Return paths:** the return-path rule is applied by the export gate, and no per-via check is recorded here.
- **ERC and DFM:** `kicad-cli sch erc` (10.0.8) reports 0 errors, 8 `lib_symbol_issues` and 5 `lib_symbol_mismatch` warnings. No JLCDFM or BOM-matching run is recorded.
