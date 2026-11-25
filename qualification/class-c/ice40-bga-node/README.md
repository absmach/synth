# iCE40HX4K BGA node with a USB-C power input (Class C, case 1)

The board carries an iCE40HX4K FPGA in a 121-ball, 0.8 mm-pitch BGA with its supply chain, an SPI boot flash, one 12-pin header and four status LEDs. A USB-C receptacle powers it (power only) and a 6-pin header programs it. It has six copper layers and measures 59.9 x 45.7 mm.

It answers #328 under #40: a board with a BGA part, escaped under the pads, with via and fanout rules, made by an agent driving `synth mcp`. The board carries no firmware and has no USB data path. The figures below are measured on the board file or calculated from datasheets; nothing was built or measured on hardware.

The design is summarised in [`PROMPT.md`](PROMPT.md), written after the board was finished. The prompts the agent ran are in the history at `0daaed69`.

## The board

### Parts

| Group                   | Parts                                                                                                                                     |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Power entry             | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS) with `R_CC1` and `R_CC2`, 5.1 k pull-downs on CC1 and CC2.           |
| Power entry             | `F1` Bourns MF-NSMF050-2 (0.5 A hold, 1 A trip, 13.2 V).                                                                                  |
| Power entry             | `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown, 10.3 V clamp at 38.8 A), and `C1`.                                   |
| Power entry             | No reverse-polarity stage, since a Type-C receptacle cannot be mated reversed.                                                            |
| CC protection           | `D8` and `D9` TPD1E10B06 bidirectional TVS diodes, one on each CC line at the receptacle, with no rail connection.                        |
| FPGA clock              | `X1`, 25 MHz oscillator on global buffer input `GBIN1` (`iot_197_gbin1`), decoupled by `C41`. The iCE40HX has no internal oscillator.     |
| 3.3 V                   | `U3` AMS1117-3.3 with 4 x 10 uF.                                                                                                          |
| 1.2 V core              | `U4` Texas Instruments TLV70012DDCR, TSOT-23-5, with 1 uF in and 1 uF + 4.7 uF out.                                                       |
| 1.2 V core              | PLL supply `VCCPLL` through the ferrite bead `FB1` (Murata BLM18AG601SN1D) and `C20` to `C22`.                                            |
| FPGA                    | `U1` Lattice iCE40HX4K-BG121 (`VPP_FAST` unconnected), every supply ball decoupled.                                                       |
| Configuration           | `U2` Winbond W25Q128JVSIQ, SOIC-8 208 mil (5.3 mm body), master SPI, with the 6-pin header `J2` for an external programmer.               |
| Configuration           | `CRESET_B` pulled up. `CDONE` drives its LED through N-FET `Q2`, so the open-drain `CDONE` net sees only a gate.                          |
| Programming header `J2` | Pins: `CFG_SS_B` (1, chip select), `CFG_SCK` (2), `CFG_SDI` (3), `CFG_SDO` (4), `CRESET_B` (5), ground (6).                               |
| Programming header `J2` | Names are from the FPGA's side: pin 3 is the flash output (MISO at the programmer) and pin 4 is the flash input (MOSI at the programmer). |
| Programming header `J2` | No 3.3 V pin. A programmer that senses target voltage needs a tap on `TP2`, which is on the 3.3 V rail.                                   |
| Programming procedure   | Hold `CRESET_B` low with an external programmer, write the flash `U2` over SPI, then release `CRESET_B` so the FPGA loads the bitstream.  |
| I/O                     | `J3` JXTCONN JXT-PZ2.54-1X12P-A1, 12 positions (3.3 V, ground and four FPGA I/O through 100 ohm series resistors).                        |
| Indicators              | Four LEDs in one row.                                                                                                                     |
| Test points             | `TP1` to `TP5` on 5 V, 3.3 V, 1.2 V, `CDONE` and `CRESET_B`.                                                                              |
| Mounting                | Two M2.5 mounting holes at opposite corners (bottom left and top right).                                                                  |

### Layout

| Item                    | Value                                                                                                                                                        |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Outline                 | 59.9 x 45.7 mm                                                                                                                                               |
| Copper layers           | 6                                                                                                                                                            |
| Stackup                 | `manufacturer "jlc-advanced"` (the JLCPCB tier with the 0.2 mm drill and via-in-pad on six layers) and `requires via_in_pad`.                                |
| Parts in the BOM        | 63 lines: 56 parts with manufacturer, MPN, rating, datasheet and order number, and 7 board features with no part identity (2 mounting holes, 5 test points). |
| Parts placed in the CPL | 56                                                                                                                                                           |
| Nets                    | 31, all routed                                                                                                                                               |
| Track segments          | 372                                                                                                                                                          |
| Vias                    | 86: 19 at 0.35/0.2 mm, 10 at 0.46/0.2 mm, 57 at 0.6/0.3 mm.                                                                                                  |
| Track length            | 1003.8 mm                                                                                                                                                    |
| Zones and planes        | `+3V3` is poured on the inner layers and ground on the outer layers. The 1.2 V core rail and the PLL supply are routed.                                      |
| USB-C placement         | `J1` sits on the left edge with its opening facing out (rotation 270). `escape { off }` is set on `J1`, which is not a ball grid.                            |
| BGA escape              | `escape { method underpad }` on `U1`. The 19 vias at 0.35/0.2 mm are filled and capped vias in the ball pads.                                                |
| Fab package             | `fab/gerbers.zip`, `fab/bom.csv`, `fab/cpl.csv`. `J2` and `J3` are hand-fit through-hole parts.                                                              |

### Checks

| Check                              | Result                                             | Notes                                                                                                                                                                                                                                    |
| ---------------------------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| KiCad DRC (10.0.8, --refill-zones) | 0 errors, 0 unconnected, 3 warnings                | Warnings: 2 `silk_overlap`, 1 `silk_over_copper` (silkscreen cut back from vias and pads by the exporter, #425).                                                                                                                         |
| KiCad ERC (10.0.8)                 | 0 errors, 0 unconnected, 13 warnings               | Warnings: 8 `lib_symbol_issues`, 5 `lib_symbol_mismatch`.                                                                                                                                                                                |
| DRC rules file                     | Required for a clean DRC                           | `kicad/ice40_bga_node.kicad_dru` lowers the board floors (0.46 mm via diameter, 0.13 mm annular ring) to 0.35 mm and 0.075 mm for vias inside the courtyard of `U1` only.                                                                |
| DRC without the rules file         | `via_diameter` and `annular_width` errors          | Reported on the 19 via-in-pad vias. KiCad loads the rules file when it sits beside the board.                                                                                                                                            |
| Zone fill                          | Refilled                                           | The board is committed with refilled zones, and the Gerbers are plotted from that fill.                                                                                                                                                  |
| Dog-bone escape                    | Does not fit                                       | An export with `escape { method channel }` and without `requires via_in_pad` stopped at the plane stage: `net +3V3: no legal stitching-via position beside the pad at (38.00, 36.40) mm on Top`.                                         |
| Routing engine policy              | `engine_policy_verdict: rejected`                  | Under its strict-size policy, with no rule below the board floor.                                                                                                                                                                        |
| Routing engine via counts          | `claimed_total_vias: 29`, `claimed_via_in_pad: 36` | These count only the vias its own routing and fanout steps placed. Synth's later steps add plane, return and stitch vias and move vias out of pads that do not declare an under-pad escape. The Layout table gives the delivered values. |

### Design figures

| Area          | Item                                      | Value                                                                                                                                        | Basis                                                                     |
| ------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Configuration | iCE40HX4K bitstream size                  | 135,183 bytes (1,081,464 bits)                                                                                                               | Lattice iCE40 Programming and Configuration, FPGA-TN-02001-3.4, Table 9.2 |
| Configuration | Flash capacity                            | 128 Mbit, holds the bitstream                                                                                                                | Winbond W25Q128JV datasheet                                               |
| Thermal       | `U3` dissipation formula                  | `(5.0 - 3.3) x I`                                                                                                                            | calculated                                                                |
| Thermal       | AMS1117 junction limit                    | 125 degC                                                                                                                                     | AMS1117 datasheet                                                         |
| Thermal       | AMS1117 SOT-223 theta-JA                  | 46 to more than 90 degC/W, depending on copper area; the budget uses 90 degC/W                                                               | AMS1117 datasheet                                                         |
| Power         | `+3V3` load: `U4` TLV70012                | 200 mA (rated output; a linear regulator draws it from the input)                                                                            | TI TLV700 datasheet SBVS173, recommended operating conditions             |
| Power         | `+3V3` load: `U2` W25Q128JV               | 25 mA (`ICC5` and `ICC7` maximum, program and erase)                                                                                         | Winbond W25Q128JV datasheet, DC electrical characteristics                |
| Power         | `+3V3` load: four LEDs with 1 k resistors | 1.3 mA                                                                                                                                       | calculated in the source                                                  |
| Power         | `+3V3` total load                         | 226 mA. The loads are datasheet maxima added together; no operating state reaches all of them at once.                                       | calculated                                                                |
| Thermal       | `U3` at 226 mA                            | 0.38 W, a 35 degC rise: Tj 60 degC at 25 degC ambient, 105 degC at 70 degC ambient                                                           | calculated                                                                |
| Thermal       | Headroom for `J3`                         | About 0.43 A more at 25 degC ambient, or 0.13 A at 70 degC, before Tj reaches 125 degC                                                       | calculated                                                                |
| Thermal       | `F1` bound                                | 0.5 A hold bounds `U3` dissipation at 0.85 W (Tj 101 degC at 25 degC ambient)                                                                | calculated                                                                |
| Thermal       | Warning `W-SYNTH-THERMAL-001`             | Comes from the registry part's 1200 mW package rating at 90 degC/W, not from this board's load                                               | Synth registry part                                                       |
| Protection    | `VBUS`                                    | `D1` (6.0 V standoff) protects `VBUS`.                                                                                                       | Nexperia PTVS6V0S1UR datasheet                                            |
| Protection    | CC lines                                  | `D8` and `D9` (5.5 V standoff) have no connection to `VBUS`, so a source that drives CC before VBUS cannot back-power the rail through them. | TI TPD1E10B06 datasheet                                                   |
| Protection    | `J3`                                      | Carries FPGA I/O through 100 ohm series resistors and has no protection device.                                                              | measured on the board file                                                |
| Assembly      | Via-in-pad land                           | JLCPCB's via-in-pad guide gives the land as the drill plus 0.15 mm.                                                                          | JLCPCB via-in-pad guide                                                   |

### Sourcing

| Item                      | Status                    | Notes                                                                                                                                       |
| ------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `U2` W25Q128JVSIQ package | Checked                   | Manufacturer listings (LCSC, Arrow, RS, JITX) give an 8-pin SOIC of 208 mil. The footprint is `Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`.        |
| `U4` TLV70012 package     | Checked                   | Listed at LCSC only in the DDC (thin) package, which the footprint `TSOT-23-5` matches. Pinout same as DBV: 1 IN, 2 GND, 3 EN, 4 NC, 5 OUT. |
| `D8`, `D9`                | Ordered as TPD1E10B06DYAR | SOD-523, LCSC C3712135, not the DPY X1SON package.                                                                                          |
| `U1` stock                | Unclear                   | The LCSC page for `U1` does not show clear stock. DigiKey 220-2147-ND is the alternative.                                                   |
| LCSC stock figures        | Not firm                  | Stock figures changed between page reads in 2026-11.                                                                                        |

## Not verified by Synth

| Area                 | What is not verified                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------ |
| USB data             | The board has no USB data path. `J1` is a power-only USB-C input.                          |
| Return paths         | The return-path rule is applied by the export gate, and no per-via check is recorded here. |
| CC attach            | No attach test on a 3 A charger was run with the CC diodes `D8` and `D9` fitted.           |
| ESD level            | The board is a bench programming node. No IEC 61000-4-2 level is claimed.                  |
| Thermal              | The 3.3 V budget is calculated from datasheet figures. No temperature was measured.        |
| DFM                  | No JLCDFM or BOM-matching run is recorded.                                                 |
| Independent sign-off | The independent PCB review required by #40 is still open.                                  |
