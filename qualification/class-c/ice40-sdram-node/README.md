# iCE40HX4K SDR SDRAM node with a USB-C power input (Class C, case 2)

An iCE40HX4K FPGA in a 144-pin TQFP (0.5 mm pitch) with a 64 Mbit SDR SDRAM (ISSI IS42S16400J, TSOP-II-54), an SPI boot flash, a 1x6 programming header and three status LEDs, powered from a USB-C receptacle (5 V only), on six copper layers. It answers #329 under #40: a board whose memory interface is length- and impedance-sensitive, made by an agent driving `synth mcp`. The interface is a single-data-rate SDRAM driven by a soft controller in the FPGA fabric: there is no DQS strobe, no differential pair and no DDR claim. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request.

**Status:** the board is routed and released (60 of 60 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and the STEP model are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/)). The numbers, hashes and files below all come from one export, made with the Synth binary of the commit named in [`PROVENANCE.md`](PROVENANCE.md).

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 70.4 x 69.0 mm, 4,858 mm2, 73 parts (64 excluding mounting holes and test points) |
| Courtyard fill | **28.2 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,457 mm2 more courtyard |
| Stackup | six copper layers, `manufacturer "jlc-advanced"`, JLCPCB's `JLC06161H-3313` stackup as declared in the source (prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um; [JLCPCB impedance tool](https://jlcpcb.com/impedance)), 1.47 mm in the STEP model |
| Routing | 60 of 60 nets, 1029 segments, 205 vias (0.46/0.2 mm: 68, 0.6/0.3 mm: 137), 1838.4 mm of track; the router's report and the delivered board agree ([`reports/delivered_board_counts.json`](reports/delivered_board_counts.json)) |
| Impedance | `SDCLK` declares 50 ohm: 0.15 mm tracks on the outer layers over 0.0994 mm of prepreg give Z0 = 87/sqrt(er+1.41) x ln(5.98 h / (0.8 w + t)) = 87/2.347 x ln(0.5944/0.155) = 49.8 ohm (closed-form microstrip estimate, not a field solve). `SDDATA` and `SDADDR` declare no impedance target: the ISSI IS42S16400J-7 AC characteristics retrieved for this board give timing only, the nets are LVTTL with 22 ohm series resistors, and their 0.15 mm width is the class width on every layer |
| Data bus | 8-bit use of the 16-bit part: the SDRAM's upper byte `DQ8` to `DQ15` (the pin row that faces the FPGA) carries data; `SD_LDQM` is held high by the FPGA, which masks the lower byte, and `DQ0` to `DQ7` are tied to ground through one 10 k resistor `RPD1`. IS42S16400J behaviour with a masked byte: ignored on a write, high-impedance on a read |
| Data bus lengths | `DQ8` to `DQ15`: 11.4, 11.6, 11.7, 13.7, 14.4, 16.1, 16.2 and 17.7 mm; spread 6.34 mm against the class budget `max_skew 10 mm` and `max_length 60 mm` |
| Clock | `SD_CK` 7.0 mm from the FPGA to the series resistor plus 10.8 mm beyond it, 17.8 mm in all, against `max_length 40 mm` |
| Address, bank and control group | lengths from 10.9 mm (`SD_CKE`) to 57.5 mm (`SD_BA0`) by the pad-to-pad measure above (62.1 mm and 63.5 mm for `SD_BA0` and `SD_BA1` by the DRC's routed-copper measure), a spread of 46.6 mm. The `SDADDR` class declares `max_skew 60 mm` and `max_length 80 mm`, derived from the IS42S16400J-7 AC characteristics (CAS latency 3): 7 ns minimum clock cycle, address and command setup 1.5 ns, hold 0.8 ns, so 7 - 1.5 - 0.8 = 4.7 ns of window at 143 MHz. A 60 mm group skew is 0.42 ns at 7.0 ps/mm (stripline, er 4.4), 9 % of that window; an 80 mm run is 0.56 ns of delay. Synth's export gate does not run the class skew check (`E-SYNTH-DRC-019`): [#147](https://github.com/absmach/synth/issues/147) (skew budgets are not carried through export) |
| Edge rate | `SDDATA` and `SDCLK` declare `edge_rate fast` (the SDRAM drives the data bus on a read, and no output-edge figure for the IS42S16400J was found, so no slower claim is made). `SDADDR` declares `edge_rate medium`: the FPGA alone drives it, and Lattice's IBIS model `ice40.ibs` rev 2.5 gives 20 to 80 % rise and fall times of at least 0.976 ns (banks 0 to 2, `lvc330io`) and 0.728 ns (bank 3, `lvc330_b3io`) at the fast corner, so no edge is faster than 0.3 ns. The medium window is 6 mm ([#445](https://github.com/absmach/synth/issues/445)) |
| Return-via advisories | 8 non-blocking `W-SYNTH-DRC-016` findings on nets that carry no fast edge (`+3V3` 6, `+1V2` 1, `SPI_SS_B` 1), 0 blockers, and no `E-SYNTH-DRC` signal-integrity finding on the routed board; see the return-path table below |
| DRC with the library tables | 0 errors, 0 unconnected, 71 warnings: 70 `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; [`reports/footprint_diff.txt`](reports/footprint_diff.txt) shows every difference is silkscreen) and 1 `track_dangling` on `SD_DQ_LO`, the tie of the masked lower byte |
| ERC with the library tables | 0 errors, 4 warnings: `lib_symbol_issues` (the `Si2301CDS` symbol is not in KiCad's library, #96), `lib_symbol_mismatch` (`AO3400A`, #428) and 2 `pin_to_pin` on the configuration-select pins `CBSEL0` and `CBSEL1`, tied to ground on purpose (select the default configuration) |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The delivered board has 279 such narrowings: 168 tracks narrowed at a terminal ("terminal neck"; 145 of them from 0.4 to 0.3458 mm, the rest to 0.3736, 0.3738, 0.3958, 0.4458 and 0.5458 mm), 10 tracks from 0.4 to 0.2 mm at fine-pitch taps, 2 from 0.4 to 0.2 mm rescuing a net, and 99 vias from 0.6 mm to 0.46 or 0.56 mm inside pads (the board's via floor is 0.35 mm, drill 0.2 mm, ring 0.075 mm). Each is a neck or a via-in-pad at a pad of the part it serves, within the board floors; the engine reports no rule below the board floor ([`export/ice40_sdram_node.routing.json`](export/ice40_sdram_node.routing.json)). The delivered board is judged by the KiCad DRC above |

### Return paths

The rule `W-SYNTH-DRC-016` asks that a layer-change via on a fast net have a ground via within a window of the thickest dielectric (floor: pad plus clearance); it blocks the release for a fast net and is an advisory for a slow one. Measured on the delivered board, centre to centre:

| Net | Layer change at (mm) | Nearest ground via (mm) | Net edge |
|---|---|---|---|
| `+1V2` | 23.4, 30.2 | 2.72 | slow, advisory |
| `SPI_SS_B` | 47.9, 34.4 | 2.80 | slow, advisory |
| `+3V3` | 49.4, 35.3 | 1.40 | slow, advisory |
| `+3V3` | 51.9, 26.5 | 3.47 | slow, advisory |
| `+3V3` | 27.3, 19.4 | 0.91 | slow, advisory |
| `+3V3` | 20.9, 14.9 | 1.90 | slow, advisory |
| `+3V3` | 16.2, 12.0 | 3.31 | slow, advisory |
| `+3V3` | 58.4, 48.5 | 3.76 | slow, advisory |

The 68 vias on the SDRAM signal nets: every `SD_DQ` via is within 0.99 mm of a ground via, the clock vias within 0.67 mm, and the address and control vias within 6.46 mm (median over all 68: 2.2 mm). Ground is poured on all six layers (a `GND` zone on each), and the supply nets are routed as tracks. The supply and configuration nets named in the table carry no fast edge (the net classes `PWR33`, `PWR12` and `LOWSPD` declare `edge_rate slow`), so their findings are advisories, with the nearest ground via at the distances listed.

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS only, D+, D- and SBU unconnected) with `R2` and `R3`, 5.1 k pull-downs on CC1 and CC2; `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 2.2 A trip, 16 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown) above the 5.25 V maximum input; `Q1` Vishay SI2301CDS P-MOSFET reverse-polarity stage with `R1`, `C1` |
| 3.3 V | `U1` AMS1117-3.3 with 10 uF capacitors `C2` to `C5` |
| 1.2 V core | `U2` Texas Instruments TLV70012DDCR, TSOT-23-5, with `C6` and `C7`; the PLL supply `VCCPLL` through the ferrite bead `L1` (Murata BLM18AG601SN1D) and `C23` |
| FPGA | `U3` Lattice iCE40HX4K-TQ144, supply decoupling 100 nF `C10` to `C24` |
| SDRAM | `U4` ISSI IS42S16400J-7TLI (64 Mbit, 143 MHz, 3.0 to 3.6 V), decoupling `C30` to `C37`, six 22 ohm series resistors `RS1` to `RS6` at the FPGA end of the command, clock and control lines |
| Configuration | `U5` Winbond W25Q128JVSIQ, SOIC-8 208 mil, master SPI; `R10` and `R11` pull `CRESET_B` and `CDONE` up, `R12` the chip select, `R13` and `R14` the write-protect and hold pins; `J2` 1x6 programming header (SS_B, SCK, SDI, SDO, CRESET_B, GND) |
| Indicators and test points | `D2` red (3.3 V present), `D3` green (CDONE, buffered by the N-MOSFET `Q2` AO3400A because CDONE is open-drain), `D4` green user LED; test points `TP1` to `TP7`; two M2.5 mounting holes at opposite corners |

The iCE40HX4K-TQ144 has 107 user I/O (Lattice DS1040), the part model in [`reproduce/user-registry/ice40hx4k_tq144.synth.toml`](reproduce/user-registry/ice40hx4k_tq144.synth.toml) is built from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of that datasheet. The memory pins sit on the FPGA's left side facing the SDRAM, in the order of the SDRAM's near pin row, so the data bus runs straight across with no crossings.

## Layout and fill

The placement is pinned in [`ice40_sdram_node.placement.layout.toml`](ice40_sdram_node.placement.layout.toml): the SDRAM and the FPGA face each other, the series resistors between them, the USB-C receptacle and the power chain along the bottom, the flash and the programming header on the right. The courtyard fill of 28.2 % and the outline are measured on the exported board; the parts occupy a 70.4 x 51.5 mm box (75 % of the board) and 9 of the 42 10 x 10 mm cells are empty.

## Sourcing

Every BOM line of [`fab/ice40_sdram_node_bom_orderable.csv`](fab/ice40_sdram_node_bom_orderable.csv) has a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv); 73 lines: 64 parts and 9 board features (2 mounting holes, 7 test points). `J2` is a hand-fit through-hole part. The part pages were read in 2026-11 and the stock is not firm. The IS42S16400J datasheet at Mouser could not be retrieved for the AC timing, so no AC figure of the SDRAM is claimed.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`fab/SHA256SUMS`](fab/SHA256SUMS). The Gerber content hash is `ca99712f38` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. [`agent-run/tools/step_export.sh`](agent-run/tools/step_export.sh) exports the STEP model with KiCad; [`reports/step_verify.txt`](reports/step_verify.txt) reopens it in OpenCASCADE and compares it with the PCB: 71 components with a body, 2 mounting holes, identities and positions agree (worst difference 0.0000 mm and 0.000 degrees), the outline is 70.42 x 68.99 mm as in Edge.Cuts, and no body leaves its courtyard by more than 0.25 mm. The STEP geometry hash is `4d2110ae32`. The test points are 1 oz copper discs and the fuse is a block of the Bourns datasheet's mid-range dimensions (A 4.55, B 3.24, C 0.70 mm), both written by [`agent-run/tools/step_models.py`](agent-run/tools/step_models.py).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The traces are in [`agent-run/traces/`](agent-run/traces/).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_sdram_node.kicad_sch` | f8ef51b042 | f8ef51b042 | f8ef51b042 | byte-identical |
| `ice40_sdram_node.kicad_pro` | 2793337457 | 2793337457 | 2793337457 | byte-identical |
| `ice40_sdram_node.kicad_sym` | dd8c3573b5 | dd8c3573b5 | dd8c3573b5 | byte-identical |
| `bom.csv` | 03cec484c2 | 03cec484c2 | 03cec484c2 | byte-identical |
| `pnp.csv` | fdcbfe1dda | fdcbfe1dda | fdcbfe1dda | byte-identical |
| `ice40_sdram_node.synth.kicad_pcb` | acd0ac47ce | acd0ac47ce | acd0ac47ce | byte-identical |
| `ice40_sdram_node.kicad_pcb` | 5a87f8e3bc | 7bd09cf3e9 | 2fc6158c2f | not byte-identical; identical after the router's and KiCad's random ids are masked (`398b8079ec`) |
| the 10 sub-sheet `.kicad_sch` files | | | | byte-identical in all three exports |
| Gerber and drill content | ca99712f38 | ca99712f38 | ca99712f38 | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP traces in [`agent-run/`](agent-run/) (see [`PROVENANCE.md`](PROVENANCE.md)).

## Human review

Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. The PCB review sign-off is JeffMboya's own and is recorded below once given.

### PCB review sign-off

PCB review sign-off required by #39: JeffMboya, on the package in `fab/` (Gerber content hash `ca99712f38`, STEP geometry hash `4d2110ae32`). The sign-off is JeffMboya's own: it is recorded per item as "verified and approved by JeffMboya" when Jeff states it.

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Return paths | |
| ERC | |
| DRC | |
| JLCPCB reports | |
| Placement | |
| Parts and values | |
