# iCE40HX4K BGA node with a USB-C input and an MCU (Class C, case 1)

An iCE40HX4K FPGA in a 121-ball, 0.8 mm-pitch BGA with its supply chain, an SPI boot flash, one 12-pin header and four status LEDs, powered from a USB-C receptacle, with an STM32F103C8 companion MCU that programs the flash over USB, on six copper layers. It answers #328 under #40: a board with a BGA part, escaped under the pads, with via and fanout rules, made by an agent driving `synth mcp`. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request.

**Status:** the board is routed and released (42 of 42 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and the STEP model are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/)). The numbers, hashes and files below all come from one export, made with the Synth binary built from commit `3a7a596b` of branch `fix-bga-synth` (see [`PROVENANCE.md`](PROVENANCE.md)).

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 56.0 x 45.0 mm, 2,519 mm2, 77 parts (70 excluding mounting holes and test points) |
| Courtyard fill | **35.8 %** of the board area, measured on the exported board (`layout_metrics.py`) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"` (the JLCPCB tier that offers the 0.2 mm drill and via-in-pad on six layers), `requires via_in_pad`, 1.37 mm in the STEP model |
| Routing | 42 of 42 nets, 393 segments, 186 vias (0.35/0.2 mm: 22, 0.46/0.2 mm: 87, 0.6/0.3 mm: 77), 983.2 mm of track; the router's report and the delivered board agree ([`reports/delivered_board_counts.json`](reports/delivered_board_counts.json)) |
| Plane | `+3V3` is poured on the inner layers (`plane` on its net class); ground on the outer layers; the 1.2 V core rail, the PLL supply and the MCU supply `+3V3_MCU` are routed |
| BGA escape | `escape { method underpad }` on `U1`; the board has 22 vias at the 0.35/0.2 mm size inside pads. A filled-and-capped via in a pad is the process JLCPCB's via-in-pad guide describes: land of the drill plus 0.15 mm |
| USB-C | `J1` sits on the left edge with its mating opening facing the edge (rotation 270, courtyard 0.05 mm from Edge.Cuts); `escape { off }` on `J1` and on the MCU `U5`, which are not ball grids |
| USB pair | D+ and D- run from `J1` through 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11`, declared as 90 ohm differential pairs (the connector side and the MCU side) in the `USB` class (0.12 mm tracks on `F.Cu` and `B.Cu`, `edge_rate fast`). Measured on the delivered board: D+ is 34.0 mm (28.2 mm to `R25`, 5.8 mm beyond) and D- is 44.2 mm (33.0 mm to `R26`, 11.2 mm beyond), a 10.2 mm difference; the pair is neither length-matched nor field-solved |
| DRC with the library tables | 0 errors, 0 unconnected, 74 warnings, all `lib_footprint_mismatch` (the exporter cuts silkscreen back from vias and pads, #425); 0 `silk_overlap` |
| ERC with the library tables | 23 errors, 32 warnings. 21 errors are on `U1` (20 unused I/O balls and `VPP_FAST`) and 2 on power flags. Synth has no statement to declare a no-connect, and the multi-unit flattened symbol's no-connect crosses fall off its pins: #431 |
| Return-via advisories | 46 non-blocking `W-SYNTH-DRC-016` findings on 18 slow nets (`+3V3` 12, `+1V2` 6, `VCCPLL` 3, `CFG_SDI` 3, `FA4` 3, and 19 more on the rest); each net class declares `edge_rate slow` except the `USB` class, which declares `edge_rate fast`, so a USB via that lacks a return via blocks the release; none does. They are listed in the routing report |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs with its strict-size policy, which rejects any via or track narrower than the width its net class declares. The delivered board is judged by Synth's own checks: connectivity (42 of 42), the process floor, KiCad DRC with the library tables, and the return-via rule |

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS) with `R_CC1` and `R_CC2`, 5.1 k pull-downs on CC1 and CC2; `F1` Bourns MF-NSMF050-2 (0.5 A hold, 1 A trip, 13.2 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown, 10.3 V clamp at 38.8 A); `Q1` Vishay SI2301CDS P-MOSFET reverse-polarity stage; `R3`, `C1` |
| USB data | `R25` and `R26`, 22 ohm series resistors in D+ and D- |
| MCU | `U5` STMicroelectronics STM32F103C8T6 (LQFP-48) on `+3V3_MCU`, which comes from `+3V3` through the ferrite bead `FB2`; supply decoupling `C27` to `C32` (five 100 nF and 4.7 uF); reset network `R21`, `C33` and the tactile switch `SW1`; `R22` pulls BOOT0 low; the crystal `Y1` with `C34` and `C35`; the 5-pin SWD header `J4` (3V3, SWDIO, SWCLK, NRST, GND) |
| 3.3 V | `U3` AMS1117-3.3 with 4 x 10 uF |
| 1.2 V core | `U4` Texas Instruments TLV70012DDCR, TSOT-23-5, with 1 uF in and 1 uF + 4.7 uF out; PLL supply `VCCPLL` through the ferrite bead `FB1` (Murata BLM18AG601SN1D) and `C20` to `C22` |
| FPGA | `U1` Lattice iCE40HX4K-BG121 (the registry part with `VPP_FAST` unconnected), every supply ball decoupled |
| Configuration | `U2` Winbond W25Q128JVSIQ, SOIC-8 208 mil (5.3 mm body), master SPI; `CRESET_B` pulled up, `CDONE` with its LED |
| I/O | `J3` JXTCONN JXT-PZ2.54-1X12P-A1, 12 positions (3.3 V, ground and four FPGA I/O through 100 ohm series resistors) |
| Indicators and test points | four LEDs in one row, test points on 5 V, 3.3 V, 1.2 V, `CDONE` and `CRESET_B`, two M2.5 mounting holes at opposite corners (bottom left and top right) |

The MCU's job is to program the flash. `U5` streams the bitstream from the host over USB to `U2` over SPI1 (`PA4` to `PA7` to the flash's chip select, clock and data pins) and drives `CRESET_B` and `CDONE` from `PB0` and `PB1`. The iCE40HX4K bitstream is 135,183 bytes (1,081,464 bits) according to Lattice's iCE40 Programming and Configuration technical note FPGA-TN-02001-3.4, Table 9.2, which is more than the STM32F103C8's 64 KB of flash, so the 128 Mbit flash `U2` stays the bitstream store. The board carries no firmware.

The crystal `Y1` is the JYJE 3TJ480000FYLBC (LCSC `C2924620`): 8 MHz, load capacitance 8 pF, ESR 150 ohm maximum and C0 5 pF maximum, from the manufacturer's specification ([`reports/jyje_3tj480000fylbc_specification_2021-11-18.pdf`](reports/jyje_3tj480000fylbc_specification_2021-11-18.pdf)). The load capacitors are `C = 2 x (CL - Cstray) = 2 x (8 - 3) = 10 pF` each, with 3 pF of stray capacitance assumed. The start-up criterion `gm_crit = 4 x ESR x (2 pi f)^2 x (C0 + CL)^2` is 0.26 mA/V at the maximum ESR and C0.

The flash's package was checked against the manufacturer listings (LCSC, Arrow, RS, JITX): the W25Q128JVSIQ is an 8-pin SOIC of 208 mil, and the board's footprint is `Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`. The TLV70012 is listed at LCSC only in the DDC (thin) package, which the board's footprint `TSOT-23-5` matches; its pinout is the same as the DBV package (1 IN, 2 GND, 3 EN, 4 NC, 5 OUT).

## Layout and fill

The placement is pinned in [`ice40_bga_node.placement.layout.toml`](ice40_bga_node.placement.layout.toml): the USB-C receptacle on the left edge with its opening facing out, the MCU group between the receptacle and the FPGA, the 12-pin header `J3` on the top edge with nothing beyond it, the two mounting holes at opposite corners, and the power parts along the bottom. The courtyard fill of 35.8 % and the outline are measured on the exported board.

## Sourcing

Every BOM line of [`fab/ice40_bga_node_bom_orderable.csv`](fab/ice40_bga_node_bom_orderable.csv) has a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv); 77 lines: 70 parts and 7 board features (2 mounting holes, 5 test points). Several LCSC pages show different stock figures between snapshots and are noted in the table; the part pages were read in 2026-11 and the stock is not firm. `J3` and `J4` are hand-fit through-hole parts.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`fab/SHA256SUMS`](fab/SHA256SUMS). The Gerber content hash is `0729fcfd37` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. [`agent-run/tools/step_export.sh`](agent-run/tools/step_export.sh) exports the STEP model with KiCad; [`reports/step_verify.txt`](reports/step_verify.txt) reopens it in OpenCASCADE and compares it with the PCB: 75 components with a body, 2 mounting holes drilled in the board solid, identities and positions match (worst difference 0.0000 mm), outline 55.99 x 44.99 mm against Edge.Cuts 55.99 x 44.99 mm, no body beyond its courtyard by more than 0.25 mm. The geometry hash is `39e9373f6b`. The USB-C body is the model KiCad's own `royalblue54L_feather` demo ships for this footprint, since the stock 3D library has none; the test points use [`agent-run/step-models/`](agent-run/step-models/).

## JLCPCB

BOM matching of this package in the JLCPCB assembly flow ([`reports/jlcpcb_assembly_bom_matching_2026-11-06.md`](reports/jlcpcb_assembly_bom_matching_2026-11-06.md), original [`.xlsx`](reports/jlcpcb_assembly_bom_matching_2026-11-06.xlsx)): 69 of 70 lines match a JLCPCB part; `U1` (Lattice ICE40HX4K-BG121, `C1552775`, 0 in stock against 5 needed) and `Y1` (3 in stock against 5) have stock below the quantity for 5 boards, and the 1x5 SWD header `J4` has no match and is hand-fitted. Quote with assembly ([`reports/jlcpcb_quote_2026-11-06.md`](reports/jlcpcb_quote_2026-11-06.md), Standard PCBA, no order placed): PCB $32.00 and PCBA $114.79, $146.79 in total.

PCB DFM of the package of this README (JLCDFM, 2026-11-06 13:03, Gerber content hash `0729fcfd37`, six layers, 5.6 x 4.5 cm, report in [`reports/`](reports/jlcdfm_pcb_dfm_2026-11-06_13-03_0729fcfd37.pdf)): 34 Danger items in four checks: annular ring 25 (0.07 mm, the 0.35/0.2 mm vias in the BGA pads; JLCPCB's via-in-pad process for six layers lists "Annular ring: 0.05 mm minimum, 0.075 mm preferred", quoted with its pages in [`reports/jlcpcb_via_in_pad_specification.md`](reports/jlcpcb_via_in_pad_specification.md), and the decision to carry them is Jeff Mboya's), via to pad 4 (0 mm, vias whose land overlaps a component pad), slot width 4 (0.6 mm, the USB-C shield slots; JLCPCB's capabilities page lists a plated slot minimum of 0.35 mm for multilayer boards, quoted in [`reports/jlcpcb_slot_specification.md`](reports/jlcpcb_slot_specification.md), and the decision to carry them is Jeff Mboya's) and through-hole to SMD pad 1 (1.6 mm, the SWD header `J4` to `FB2` and `C32`).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The traces are in [`agent-run/traces/`](agent-run/traces/).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_bga_node.kicad_sch` (one sheet) | a57a09d937 | a57a09d937 | a57a09d937 | byte-identical |
| `ice40_bga_node.kicad_pro` | b16abc366f | b16abc366f | b16abc366f | byte-identical |
| `ice40_bga_node.kicad_sym` | 5103fb8457 | 5103fb8457 | 5103fb8457 | byte-identical |
| `bom.csv` | 0153fda01c | 0153fda01c | 0153fda01c | byte-identical |
| `pnp.csv` | e03f19f5ed | e03f19f5ed | e03f19f5ed | byte-identical |
| `ice40_bga_node.synth.kicad_pcb` (un-routed) | 9177d7e2be | 9177d7e2be | 9177d7e2be | byte-identical |
| `ice40_bga_node.kicad_pcb` | e51186cd74 | 3f35fe817c | ff7725265d | not byte-identical; identical after the router's and KiCad's random ids are masked (`2bd5d05c5b`) |
| Gerber and drill content | 0729fcfd37 | 0729fcfd37 | 0729fcfd37 | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP traces in [`agent-run/`](agent-run/) (see [`PROVENANCE.md`](PROVENANCE.md)).

## Human review

Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. The PCB review sign-off is JeffMboya's own and is recorded below when he states it.

### PCB review sign-off

Not yet recorded.
