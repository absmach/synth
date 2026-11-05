# iCE40HX4K BGA node (Class C, case 1)

An iCE40HX4K FPGA in a 121-ball, 0.8 mm-pitch BGA with its supply chain, an SPI boot flash, one 12-pin header and four status LEDs, on six copper layers. It answers #328 under #40: a functional board with a BGA part, escaped under the pads, with via and fanout rules, made by an agent driving `synth mcp`. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request.

**Status:** the board is routed and released (29 of 29 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and the STEP model are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/)). The numbers, hashes and files below all come from one export, made with the Synth binary of branch `fix-bga-synth` (see [`PROVENANCE.md`](PROVENANCE.md)).

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 44.0 x 40.0 mm, 1,759 mm2, 57 parts (50 excluding mounting holes and test points) |
| Courtyard fill | **31.4 %** of the board area, measured on the exported board (`layout_metrics.py`); the compaction tool's own figure is 35.7 % |
| Stackup | six copper layers, `manufacturer "jlc-advanced"` (the JLCPCB tier that offers the 0.2 mm drill and via-in-pad on six layers), `requires via_in_pad`, 1.37 mm in the STEP model |
| Routing | 29 of 29 nets, 248 segments, 132 vias (0.35/0.2 mm: 20, 0.46/0.2 mm: 52, 0.6/0.3 mm: 60), 596.4 mm of track; the router's report and the delivered board agree ([`reports/delivered_board_counts.json`](reports/delivered_board_counts.json)) |
| Plane | `+3V3` is poured on the inner layers (`plane` on its net class); ground on the outer layers; the 1.2 V core rail and the PLL supply are routed |
| BGA escape | `escape { method underpad }` on `U1`; the router reports 20 escape vias at the 0.35/0.2 mm size inside ball pads. A filled-and-capped via in a pad is the process JLCPCB's via-in-pad guide describes: land of the drill plus 0.15 mm |
| DRC with the library tables | 0 errors, 0 unconnected, 56 warnings: 54 `lib_footprint_mismatch` (the exporter cuts silkscreen back from vias and pads, #425) and 2 `silk_overlap` |
| ERC with the library tables | 23 errors, all on `U1`: 21 `pin_not_connected` (19 unused I/O balls and `VPP_FAST`) and 2 `power_pin_not_driven` (`VPP_FAST` and a power flag), 33 warnings. Synth has no statement to declare a no-connect, and the multi-unit flattened symbol's no-connect crosses fall off its pins: #431 |
| Return-via advisories | 57 non-blocking `W-SYNTH-DRC-016` findings on 16 slow nets (`+3V3` 17, `+1V2` 10, `+5V` 6, `VCCPLL` 4, the rest 1 to 3 each); each net class declares `edge_rate slow`. They are listed in the routing report |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs with its strict-size policy, which rejects any via or track narrower than the width its net class declares. The delivered board is judged by Synth's own checks: connectivity (29 of 29), the process floor, KiCad DRC with the library tables, and the return-via rule |

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` 1x2 header (5 V), `F1` Bourns MF-NSMF050-2 (0.5 A hold, 1 A trip, 13.2 V), `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown, 10.3 V clamp at 38.8 A), `Q1` Vishay SI2301CDS P-MOSFET reverse-polarity stage, `R3`, `C1` |
| 3.3 V | `U3` AMS1117-3.3 with 4 x 10 uF |
| 1.2 V core | `U4` Texas Instruments TLV70012DDCR, TSOT-23-5, with 1 uF in and 1 uF + 4.7 uF out; PLL supply `VCCPLL` through the ferrite bead `FB1` (Murata BLM18AG601SN1D) and `C20` to `C22` |
| FPGA | `U1` Lattice iCE40HX4K-BG121 (the registry part with `VPP_FAST` unconnected), every supply ball decoupled |
| Configuration | `U2` Winbond W25Q128JVSIQ, SOIC-8 208 mil (5.3 mm body), master SPI; `CRESET_B` pulled up, `CDONE` with its LED |
| I/O | `J3` JXTCONN JXT-PZ2.54-1X12P-A1, 12 positions (3.3 V, ground and four FPGA I/O through 100 ohm series resistors) |
| Indicators and test points | four LEDs in one row, test points on 5 V, 3.3 V, 1.2 V, `CDONE` and `CRESET_B`, two M2.5 mounting holes |

The iCE40HX4K bitstream is 135,183 bytes (1,081,464 bits) according to Lattice's iCE40 Programming and Configuration technical note FPGA-TN-02001-3.4, Table 9.2, so the 128 Mbit flash holds it with room to spare. The flash's package was checked against the manufacturer listings (LCSC, Arrow, RS, JITX): the W25Q128JVSIQ is an 8-pin SOIC of 208 mil, and the board's footprint is `Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`. The TLV70012 is listed at LCSC only in the DDC (thin) package, which the board's footprint `TSOT-23-5` matches; its pinout is the same as the DBV package (1 IN, 2 GND, 3 EN, 4 NC, 5 OUT).

## Layout and fill

The placement was densified with Synth's own `synth_compact_layout` in scale mode (`step_percent` 4), called through the Synth MCP server ([`agent-run/tools/mcp_compact.py`](agent-run/tools/mcp_compact.py)), one attempt per call. Each attempt is a full export with the release gates, so a step passes only if every net is routed and KiCad DRC reports no error. Steps 1000, 960 and 920 passed (47.0 x 44.0, 44.99 x 41.99 and 43.99 x 40.99 mm); steps 900 and 880 failed their release checks. The board is the step 920 placement ([`reports/compaction_scale_mode.json`](reports/compaction_scale_mode.json)). The compaction was run on the board before the regulator's package was changed to the DDC part; the three exports below are of the step 920 placement with the DDC part, and are routed at the same outline.

## Sourcing

Every BOM line of [`fab/ice40_bga_node_bom_orderable.csv`](fab/ice40_bga_node_bom_orderable.csv) has a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv); 57 lines: 50 parts and 7 board features (2 mounting holes, 5 test points). Several LCSC pages show different stock figures between snapshots and are noted in the table; the part pages were read in 2026-11 and the stock is not firm. `J3` is a hand-fit through-hole part (a JLCPCB parts-library listing, an extended part).

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`fab/SHA256SUMS`](fab/SHA256SUMS). The Gerber content hash is `ea3bf2d72c` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. [`agent-run/tools/step_export.sh`](agent-run/tools/step_export.sh) exports the STEP model with KiCad; [`reports/step_verify.txt`](reports/step_verify.txt) reopens it in OpenCASCADE and compares it with the PCB: 55 components with a body, 2 mounting holes drilled in the board solid, identities and positions match (worst difference 0.0000 mm), outline 43.99 x 39.99 mm equal to Edge.Cuts, drills match, no body beyond its courtyard by more than 0.25 mm, no problems. STEP geometry hash `7fff11ce33`.

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The traces are in [`agent-run/traces/`](agent-run/traces/).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_bga_node.kicad_sch` (one sheet) | cbf1c379ca | cbf1c379ca | cbf1c379ca | byte-identical |
| `ice40_bga_node.kicad_pro` | ea76693264 | ea76693264 | ea76693264 | byte-identical |
| `ice40_bga_node.kicad_sym` | cac285d472 | cac285d472 | cac285d472 | byte-identical |
| `bom.csv` | f224ac597b | f224ac597b | f224ac597b | byte-identical |
| `pnp.csv` | 7c7420542d | 7c7420542d | 7c7420542d | byte-identical |
| `ice40_bga_node.synth.kicad_pcb` (un-routed) | 5088387c91 | 5088387c91 | 5088387c91 | byte-identical |
| `ice40_bga_node.kicad_pcb` | d900156128 | 887a8ebb70 | 7c396eb7e8 | not byte-identical; identical after the router's and KiCad's random ids are masked (`9018b12adf`) |
| Gerber and drill content | ea3bf2d72c | ea3bf2d72c | ea3bf2d72c | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP traces in [`agent-run/`](agent-run/) (see [`PROVENANCE.md`](PROVENANCE.md)).

## Human review

Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. The PCB review sign-off is JeffMboya's own and is recorded below when he states it.

### PCB review sign-off

Not yet recorded.
