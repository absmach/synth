# iCE40HX4K HDMI (DVI-style TMDS) node (Class C, case 3)

An iCE40HX4K FPGA in a 144-pin TQFP (0.5 mm pitch) driving an HDMI Type-A receptacle with four TMDS-style differential pairs (three data lanes and the clock) through 270 ohm source resistors, with the DDC lines level-shifted, a hot-plug-detect divider and a fused +5 V pin, an SPI boot flash and four status LEDs, powered from a 5 V terminal, on six copper layers. It answers #330 under #40: a board whose high-speed differential interface declares impedance, pair spacing and skew, with the delivered copper measured against those declarations. The interface is DVI-style: the iCE40 cannot source TMDS levels, so each line is an LVCMOS33 output through a series resistor into the sink's 50 ohm termination to 3.3 V. No HDMI compliance is claimed; the board demonstrates only what is measured below.

**Status:** the board is routed and released (46 of 46 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and the STEP model are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/)). The delivered differential pairs meet their declared 25 mm intra-pair skew budget; the measured numbers are in the interface table below.

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 80.3 x 72.0 mm, 5,781 mm2, 77 parts (68 excluding mounting holes and test points) |
| Courtyard fill | **22.7 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,730 mm2 of courtyard (the board holds about 1,312 mm2) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"`, JLCPCB's `JLC06161H-3313` stackup as declared in the source (prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um; [JLCPCB impedance tool](https://jlcpcb.com/impedance)), 1.47 mm in the STEP model |
| Routing | 46 of 46 nets, 644 segments, 155 vias (0.46/0.2 mm: 89, 0.6/0.3 mm: 66), 1650.2 mm of track; the router's report and the delivered board agree ([`reports/delivered_board_counts.json`](reports/delivered_board_counts.json)) |
| DRC with the library tables | 0 errors, 0 unconnected, 74 warnings, all `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; [`reports/footprint_diff.txt`](reports/footprint_diff.txt) shows no difference in pads, courtyard or fabrication layers) |
| ERC with the library tables | 0 errors, 4 warnings: 2 `lib_symbol_mismatch` on `IRLML6244` (`Q1`, `Q2`; the exported symbol differs from KiCad's `Transistor_FET` copy, #428) and 2 `pin_to_pin` on the configuration-select pins `CBSEL0` and `CBSEL1`, tied to ground on purpose |
| Return-via advisories | 2 non-blocking `W-SYNTH-DRC-016` findings on rails that carry no fast edge (`+1V2` at (41.2, 12.2) mm and `+3V3` at (32.0, 33.7) mm), 0 blockers |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The delivered board has 107 such narrowings: 63 tracks narrowed at a terminal ("terminal neck": 57 from 0.4 to 0.2998 mm, 2 to 0.3498, 2 to 0.326 and 2 to 0.3147 mm) and 44 vias from 0.6 to 0.46 mm at pads (the board's via floor). Each is a neck or a clamp at a pad of the part it serves, within the board floors; the engine reports no rule below the board floor ([`export/ice40_hdmi_node.routing.json`](export/ice40_hdmi_node.routing.json)). The delivered board is judged by the KiCad DRC above |

### Differential interface (measured on the delivered copper)

Each TMDS lane is two nets per leg: the FPGA pin to the 270 ohm source resistor (`Dn_P_SRC`, `Dn_N_SRC`) and the resistor to the connector (`HDMI_Dn_P`, `HDMI_Dn_N`). Lengths are the sum of the track segments of both nets of a leg, from [`reports/pair_measurements.json`](reports/pair_measurements.json) (`analyze3.py` in [`agent-run/tools/`](agent-run/tools/)).

| Lane | P: source + connector side | N: source + connector side | Intra-pair skew | Declared budget | Layer changes (P / N vias) | Pair gap as routed (min / median, centre to centre) |
|---|---|---|---|---|---|---|
| `D0` | 10.9 + 19.1 = 30.0 mm | 5.9 + 19.9 = 25.8 mm | 4.23 mm | 25 mm | 4 / 2 | 0.71 / 3.40 mm |
| `D1` | 27.0 + 21.6 = 48.6 mm | 5.9 + 19.1 = 25.0 mm | 23.61 mm | 25 mm | 2 / 0 | 1.08 / 1.57 mm |
| `D2` | 8.0 + 22.1 = 30.1 mm | 6.2 + 24.3 = 30.6 mm | 0.48 mm | 25 mm | 2 / 2 | 0.90 / 1.00 mm |
| `CK` | 15.1 + 21.4 = 36.4 mm | 5.5 + 24.9 = 30.4 mm | 6.06 mm | 25 mm | 2 / 2 | 1.11 / 3.54 mm |

- **Inter-pair mismatch:** the mean pair lengths are 27.9 (`D0`), 36.8 (`D1`), 30.3 (`D2`) and 33.4 mm (`CK`), a mismatch of 8.89 mm; no inter-pair budget is declared in the source.
- **Skew budget.** Every `diff_pair` declares `max_skew 25mm`. The TMDS transmitter rule is 0.15 Tbit between the two halves of a pair, with Tbit = 0.1 x the pixel period (secondary sources, not the HDMI 1.4 text: [Texas Instruments SLVA747](https://ti.com/lit/pdf/SLVA747) and the layout notes of Analog Devices and Chrontel). The 25 MHz oscillator gives a 40 ns pixel period (VGA timing, 252 Mbit/s per lane), so Tbit = 4.0 ns and 0.15 Tbit = 0.6 ns, which is 85 mm at 7.0 ps/mm (stripline, er 4.4); 25 mm is 0.175 ns, 29 % of that limit. The delivered skew is 0.48 mm at best (`D2`) and 23.61 mm at worst (`D1`) by the sum of segments above, and the DRC's own length-matching measure (`E-SYNTH-DRC-014`) reports 1.4, 1.9, 4.0 and 8.4 mm, all inside the declared budget with no blocking signal-integrity finding. The router wrapper never calls KiCadRoutingTools' pair router, so P and N are routed as independent nets ([#447](https://github.com/absmach/synth/issues/447)) and Synth's export gate does not carry the class skew budgets through ([#147](https://github.com/absmach/synth/issues/147)). The pairs are therefore not coupled: the gap between the two legs is 1 to 3.5 mm, not the 0.127 mm class clearance.
- **Width and impedance.** The class declares 0.11 mm tracks and a 0.127 mm clearance, which the closed-form microstrip estimate gives as 100.3 ohm differential on the declared stackup: Z0 = 87/sqrt(er+1.41) x ln(5.98 h / (0.8 w + t)) = 87/2.347 x ln(0.5944/0.123) = 58.4 ohm at h = 0.0994 mm, er = 4.1, t = 35 um, and Zdiff = 2 Z0 (1 - 0.48 exp(-0.96 s / h)) = 100.3 ohm at s = 0.127 mm. It is an estimate, not a field solve. The delivered tracks are 0.127 to 0.13 mm (0.2 mm in places), not 0.11 mm, so the delivered single-ended impedance is about 54 ohm (w = 0.13 mm); the delivered differential impedance is not established because the pairs are not coupled. The validator's `W-SYNTH-IMPEDANCE-001` (four warnings, one per pair) says the same for the inner layers: the router put the halves of the pairs on different layers (for example `HDMI_D1_P` runs 19.4 mm on `In4.Cu` and `HDMI_D1_N` 19.1 mm on `F.Cu`; `HDMI_D2_P` 19.5 mm on `In1.Cu` and `HDMI_D2_N` 22.4 mm on `B.Cu`), so the estimate above, made for an outer-layer microstrip over the 0.0994 mm prepreg, does not describe those runs and no impedance is claimed for them.
- **Drive levels.** With the iCE40 output at 0 V behind 270 ohm into the sink's 50 ohm pull-up to 3.3 V, the line sits at 3.3 V x 270 / (270 + 50) = 2.78 V; at 3.3 V it sits at 3.3 V, a swing of 0.52 V single-ended. The output impedance of the iCE40 pin is not included.

### Return paths and the edge rate of the TMDS nets

The rule `W-SYNTH-DRC-016` asks that a layer-change via on a fast net have a ground via within a window of the thickest dielectric; it blocks the release for a fast net. A declared `diff_pair` and a differential pin type are inferred fast. The TMDS and source classes declare `edge_rate medium` because the nets are driven only by the iCE40 through a 270 ohm resistor, and Lattice's IBIS model `ice40.ibs` rev 2.5 gives 20 to 80 % rise and fall times of at least 0.976 ns (banks 0 to 2, `lvc330io`) and 0.728 ns (bank 3, `lvc330_b3io`) at the fast corner, so no edge on these nets is faster than 0.3 ns. Synth's `edge_rate medium` tier ([#445](https://github.com/absmach/synth/issues/445)) widens the window to 6 mm; the Synth commit named in [`PROVENANCE.md`](PROVENANCE.md) lets a declared `medium` replace the inference from a `diff_pair` or a differential pin, and a clock, RF or analog pin, a crystal or a switching node still prove a fast edge. Measured on the delivered board, centre to centre, the TMDS layer-change vias are within 0.63 to 4.75 mm (`D0`), 3.55 to 3.91 mm (`D1`), 1.94 to 6.63 mm (`D2`) and 1.18 to 5.44 mm (`CK`) of a ground via.

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` JST B2B-XH-A 2-pin header (5 V in); `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 2.2 A trip, 16 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff above the 5.5 V maximum input, 6.67 V minimum breakdown); `D2` Vishay SS36 Schottky (60 V, 3 A) as the reverse-polarity stage; `C1`, `TP1` |
| 3.3 V | `U1` AMS1117-3.3 with 10 uF capacitors `C2` to `C5` |
| 1.2 V core | `U2` Texas Instruments TLV70012DDCR, TSOT-23-5, with `C6` and `C7`; the PLL supplies `VCCPLL0` and `VCCPLL1` through the ferrite beads `L1` and `L2` and `C21`, `C22` |
| FPGA | `U3` Lattice iCE40HX4K-TQ144, supply decoupling 100 nF `C10` to `C20` and `C25` to `C27`; `X1` SiTime SiT8008 25 MHz oscillator with `C23` |
| Configuration | `U4` Winbond W25Q128JVSIQ, SOIC-8 208 mil, master SPI; `R30` and `R31` pull `CDONE` and `CRESET_B` up, `R32` and `R33` the write-protect and hold pins |
| HDMI | `J2` Amphenol 10029449-111RLF HDMI Type-A receptacle; `R1` to `R8` 270 ohm source resistors; `D3` onsemi SRV05-4 4-line ESD array on the DDC and HPD lines; `F2` Bourns MF-NSMF050-2 (0.5 A hold) on the +5 V pin; DDC level shifters `Q1` and `Q2` (IRLML6244) with `R9` and `R10` (2 k to +5 V) and `R11` and `R12` (10 k to 3.3 V); the HPD divider `R13` (10 k) and `R14` (20 k), which gives 5 V x 20 / (10 + 20) = 3.33 V at the FPGA |
| Indicators and test points | `D4` and `D5` green, `D6` and `D7` red LEDs with 470 ohm resistors `R20` to `R23`; test points `TP1` to `TP7` (+5 V, +3.3 V, +1.2 V, GND, CDONE, CRESET_B, +5 V_HDMI); two M2.5 mounting holes |

The iCE40HX4K-TQ144 has 107 user I/O (Lattice DS1040); the part model in [`reproduce/user-registry/ice40hx4k_tq144.synth.toml`](reproduce/user-registry/ice40hx4k_tq144.synth.toml) is built from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of that datasheet. The four lanes use the four adjacent pin pairs on the FPGA's left side (pins 1 and 2, 3 and 4, 7 and 8, 9 and 10), in the same top-to-bottom order as the connector's lanes, so no pair crosses another; the eight source resistors sit in one column between the FPGA and the connector.

## 3.3 V regulator thermal budget

`U1` (AMS1117-3.3, SOT-223) drops at most 5 V to 3.3 V, so it dissipates `P = (5.0 - 3.3) x I` and its junction sits at `Tj = TA + P x RθJA`, limited to 125 &deg;C ([Advanced Monolithic Systems AMS1117 datasheet](https://files.waveshare.com/upload/b/b8/AMS1117-datasheet.pdf), a third-party print of the manufacturer's document: "Maximum junction temperature must not exceed 125 °C"; SOT-223 `RθJA = 90 °C/W`, which "can vary from 46 °C/W to >90 °C/W depending on the size of the copper area"). The board uses 90 °C/W. `W-SYNTH-THERMAL-001` comes from the registry part's declared 1200 mW rating at 90 °C/W (25 + 1.2 x 90 = 133 °C), the package limit, not this board's load.

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U2` TLV70012 1.2 V core regulator | 200 mA (its rated output current; a linear regulator draws its output current from the input) | TI TLV700 datasheet SBVS173, recommended operating conditions IOUT 0 to 200 mA |
| `U4` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Four LEDs, 470 ohm | 28 mA (3.3 V / 470 ohm each, which ignores the LED forward drop) | the design's own arithmetic |
| Total | 253 mA | |

At 253 mA the regulator dissipates 1.7 V x 0.253 A = 0.43 W, a 39 &deg;C rise: Tj is 64 &deg;C at 25 &deg;C ambient and 109 &deg;C at 70 &deg;C ambient, under 125 &deg;C. Tj reaches 125 &deg;C at `(125 - 25) / 90 / 1.7 = 0.65 A` in total at 25 &deg;C ambient and at `(125 - 70) / 90 / 1.7 = 0.36 A` at 70 &deg;C ambient, which leaves about 0.40 A and 0.11 A for the FPGA's 3.3 V I/O banks, whose current depends on the design and is not counted above. The HDMI 5 V output is fed from the 5 V rail through `F2`, not through `U1`. The load currents are datasheet maxima added together, which no operating state reaches at once.

## Layout and fill

The placement is pinned in [`ice40_hdmi_node.placement.layout.toml`](ice40_hdmi_node.placement.layout.toml): the HDMI receptacle on the left edge, the eight source resistors in one column beside the FPGA's left pins, the power entry on the right edge. The courtyard fill of 22.7 % and the outline are measured on the exported board; the parts occupy an 80.3 x 69.0 mm box (96 % of the board), with 2 of the 56 10 x 10 mm cells empty and a mean nearest-neighbour gap of 2.9 mm.

## Sourcing

[`fab/ice40_hdmi_node_bom_orderable.csv`](fab/ice40_hdmi_node_bom_orderable.csv) has 77 lines: 68 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv), and 9 board features (2 mounting holes, 7 test points) that are not parts and carry no identity. `J1` is a hand-fit through-hole part. Two lines carry a sourcing note: the 2 k resistors (`R9`, `R10`, UNI-ROYAL 0603WAF2001T5E, LCSC `C22975`) showed 0 in stock on the LCSC page read, and the onsemi SRV05-4MR6T1G (`D3`) has no onsemi listing at LCSC, so it is sourced from DigiKey. The part pages were read in 2026-11 and the stock is not firm.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`fab/SHA256SUMS`](fab/SHA256SUMS). The Gerber content hash is `92d05ff69b` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. [`agent-run/tools/step_export.sh`](agent-run/tools/step_export.sh) exports the STEP model with KiCad; [`reports/step_verify.txt`](reports/step_verify.txt) reopens it in OpenCASCADE and compares it with the PCB: 75 components with a body, 2 mounting holes, identities and positions agree (worst difference 0.0000 mm and 0.000 degrees), the outline is 80.30 x 71.99 mm as in Edge.Cuts, and no body leaves its courtyard by more than 0.25 mm. The STEP geometry hash is `0ab9ee69cb`. Models the KiCad library lacks are written by [`agent-run/tools/step_models.py`](agent-run/tools/step_models.py) from datasheet dimensions: the test points (1 oz copper discs), the fuse `F1` (the Bourns datasheet's mid-range 4.55 x 3.24 x 0.70 mm) and the oscillator `X1` (the SiT8008's 5.0 x 3.2 x 0.75 mm package, SiT8008B datasheet, page 11 of 18).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The traces are in [`agent-run/traces/`](agent-run/traces/).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_hdmi_node.kicad_sch` | a37c889e53 | a37c889e53 | a37c889e53 | byte-identical |
| `ice40_hdmi_node.kicad_pro` | 0167270aa9 | 0167270aa9 | 0167270aa9 | byte-identical |
| `ice40_hdmi_node.kicad_sym` | 178c392342 | 178c392342 | 178c392342 | byte-identical |
| `bom.csv` | c152da5500 | c152da5500 | c152da5500 | byte-identical |
| `pnp.csv` | 37179498a3 | 37179498a3 | 37179498a3 | byte-identical |
| `ice40_hdmi_node.synth.kicad_pcb` | 28c6dd9da1 | 28c6dd9da1 | 28c6dd9da1 | byte-identical |
| `ice40_hdmi_node.kicad_pcb` | d8648ae55c | d607683b8d | ef5d807e5d | not byte-identical; identical after the router's and KiCad's random ids are masked (`e045b457d7`) |
| the 6 sub-sheet `.kicad_sch` files | | | | byte-identical in all three exports |
| Gerber and drill content | 92d05ff69b | 92d05ff69b | 92d05ff69b | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP traces in [`agent-run/`](agent-run/) (see [`PROVENANCE.md`](PROVENANCE.md)).

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/` (Gerber content hash `92d05ff69b`, STEP geometry hash `0ab9ee69cb`). The sign-off is JeffMboya's own. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. It covers, each verified and approved by JeffMboya, once Jeff states it:

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Differential interface | |
| Return paths | |
| ERC | |
| DRC | |
| JLCPCB reports | |
| Placement | |
| Parts and values | |

Every part of the board that carries no other reviewer in `registry/parts` is recorded as reviewed by JeffMboya (2026-11-11), with the review date and the datasheet or product page where one is sourced, in the part files of `reproduce/user-registry/` (`reviewed_by`, `reviewed_at`, `datasheet_url`).
