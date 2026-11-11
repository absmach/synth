# Qualification: iCE40HX4K BGA node with a USB-C input and an MCU (Class C, case 1)

**Status:** the board is routed and released (42 of 42 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC results, a fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)). The numbers, hashes and files below all come from one export, made with the Synth binary built from `upstream/main` `fbfd8d60`, the merge of pull request #440 (see [`PROVENANCE.md`](PROVENANCE.md)).

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 56.0 x 45.0 mm, 2,519 mm2, 77 parts (70 excluding mounting holes and test points) |
| Courtyard fill | **35.8 %** of the board area, measured on the exported board (`layout_metrics.py`) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"` (the JLCPCB tier that offers the 0.2 mm drill and via-in-pad on six layers), `requires via_in_pad`, 1.37 mm in the STEP model |
| Routing | 42 of 42 nets, 424 segments, 208 vias (0.35/0.2 mm: 18, 0.46/0.2 mm: 18, 0.6/0.3 mm: 172), 1025.6 mm of track; the router's report and the delivered board agree (the delivered-board count) |
| Plane | `+3V3` is poured on the inner layers (`plane` on its net class); ground on the outer layers; the 1.2 V core rail, the PLL supply and the MCU supply `+3V3_MCU` are routed |
| BGA escape | `escape { method underpad }` on `U1`; the board has 18 vias at the 0.35/0.2 mm size inside pads. A filled-and-capped via in a pad is the process JLCPCB's via-in-pad guide describes: land of the drill plus 0.15 mm |
| USB-C | `J1` sits on the left edge with its mating opening facing the edge (rotation 270, courtyard 0.05 mm from Edge.Cuts); `escape { off }` on `J1` and on the MCU `U5`, which are not ball grids |
| USB pair | D+ and D- run from `J1` through 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11`, declared as 90 ohm differential pairs (the connector side and the MCU side) in the `USB` class (0.12 mm tracks on `F.Cu` and `B.Cu`, `edge_rate fast`). Measured on the delivered board: D+ is 34.3 mm (28.5 mm to `R25`, 5.8 mm beyond) and D- is 44.2 mm (33.0 mm to `R26`, 11.2 mm beyond), a 9.9 mm difference; the pair is not length-matched and its impedance is an analytic estimate, not a field solve. The supported operation is USB full-speed (12 Mbit/s) only: the STM32F103 `U5` is a USB 2.0 full-speed device (ST datasheet DS5319, "USB 2.0 full-speed interface"), and `PA11`/`PA12` carry no high-speed PHY. Full-speed drivers have 4 to 20 ns rise and fall times and a 90 ohm differential +/-15 % target (the USB section below cites the pages and shows the arithmetic: 94 ohm differential estimated from the stackup, and 9.9 mm of skew, about 55 ps, against a 4 ns minimum rise time) |
| DRC with the library tables | 0 errors, 0 unconnected, 74 warnings, all `lib_footprint_mismatch` (the exporter cuts silkscreen back from vias and pads, #425); 0 `silk_overlap` |
| ERC with the library tables | 0 errors, 6 warnings: `Q1`'s `Si2301CDS` symbol is absent from KiCad 10.0.6's `Transistor_FET` library, and the embedded `ICE40HX4K-BG121` symbol of `U1` does not match its library copy in each of its five units ([absmach/synth#428](https://github.com/absmach/synth/issues/428)). The unused FPGA I/O balls and `VPP_FAST` carry no-connect markers on their own units |
| Return-via advisories | 37 non-blocking `W-SYNTH-DRC-016` findings on 15 slow nets (`+3V3` 11, `+1V2` 4, `VCCPLL` 4, `FA4` 3, `+3V3_MCU` 3, and 12 more on the rest); each net class declares `edge_rate slow` except the `USB` class, which declares `edge_rate fast`, so a USB via that lacks a return via blocks the release; none does. They are listed in the routing report |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs with its strict-size policy and reports, in `ice40_bga_node.kicadroutingtools.log`, that it lowered the board's via diameter from 0.46 to 0.35 mm, the via annular ring from 0.13 to 0.075 mm and the copper-to-hole clearance from 0.2 to 0.127 mm, the floors of the declared `jlc-advanced` process, and that 20 of 51 objects at its last step are below the original values. These are the 0.35/0.2 mm vias in pads: a filled and capped via in a pad has the land of its drill plus 0.15 mm. The engine reports no rule below the board floor (`rules_below_board_floor: none reported`). Its `claimed_total_vias: 72` and `claimed_via_in_pad: 59` count the vias its own routing and fanout steps placed; Synth's later steps add the plane, return and stitch vias and move vias out of the pads of components that do not declare an under-pad escape, so the board's own counts (208 vias, 18 of them 0.35/0.2 mm in the pads of `U1`) are the ones this README uses, and the `20 of 51 objects` above are the engine's count at its last step. The delivered board is judged by Synth's own checks: connectivity (42 of 42), the process floor, KiCad DRC with the library tables, and the return-via rule; [`fab/dfm/jlcpcb_via_in_pad_specification.md`](fab/dfm/jlcpcb_via_in_pad_specification.md) quotes JLCPCB's land for that via |

## 3.3 V regulator thermal budget

`U3` (AMS1117-3.3, SOT-223) drops 5 V to 3.3 V, so it dissipates `P = (5.0 - 3.3) x I` and its junction sits at `Tj = TA + P x RθJA`, which the datasheet limits to 125 &deg;C ([Advanced Monolithic Systems AMS1117 datasheet](https://files.waveshare.com/upload/b/b8/AMS1117-datasheet.pdf), a third-party print of the manufacturer's document: "Maximum junction temperature must not exceed 125 °C"; SOT-223 `RθJA = 90 °C/W`, which "can vary from 46 °C/W to >90 °C/W depending on the size of the copper area"). The board uses 90 °C/W, the top of that range. The Synth warning `W-SYNTH-THERMAL-001` comes from the registry part's declared 1200 mW rating at 90 °C/W (25 + 1.2 x 90 = 133 °C); it is the package limit, not this board's load.

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U4` TLV70012 1.2 V core regulator | 200 mA (its rated output current; a linear regulator draws its output current from the input) | TI TLV700 datasheet SBVS173, recommended operating conditions IOUT 0 to 200 mA |
| `U5` STM32F103C8 | 50 mA (72 MHz, all peripherals on, maximum run-mode supply current) | ST DS5319 as listed on a mirror ([radiolocman](https://www.radiolocman.com/datasheet/pdf.html?di=184185)); not read on st.com |
| `U2` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Four LEDs with 1 k resistors | 1.3 mA | the design's own arithmetic in the source |
| Total | 276 mA | |

At 276 mA the regulator dissipates 1.7 V x 0.276 A = 0.47 W, a 42 &deg;C rise: Tj is 67 &deg;C at 25 &deg;C ambient and 112 &deg;C at 70 &deg;C ambient, under the 125 &deg;C limit. The 3.3 V of `J3` can feed an external load: with that load Tj reaches 125 &deg;C at 25 &deg;C ambient at `(125 - 25) / 90 / 1.7 = 0.65 A` in total and at 70 &deg;C ambient at `(125 - 70) / 90 / 1.7 = 0.36 A`, so `J3` can supply about 0.37 A and 0.08 A respectively on top of the 276 mA. The input fuse `F1` holds 0.5 A, which bounds the sustained 5 V current and with it the regulator's dissipation at 0.85 W (Tj 101 &deg;C at 25 &deg;C ambient). The load currents are datasheet maxima added together, which no operating state reaches at once.

## ESD on the connector signals

`E-SYNTH-ESD-001` reports the four USB-C data pins (`D+` and `D-` on both receptacle rows). The board has no ESD protection device on them: they reach `U5` through the 22 ohm series resistors `R25` and `R26`, and `D1` (a 6.0 V standoff TVS) protects only `VBUS`. The MCU's own pins are rated 2 kV human-body-model (class 2) and 500 V charge-device-model according to ST DS5319, section 5.3.11, as listed on mirrors ([example](https://www.alldatasheet.net/html-pdf/516843/STMICROELECTRONICS/STM32F103C8T7/117354/60/STM32F103C8T7.html)); the datasheet itself was not read on st.com. The board is a bench programming node, and no IEC 61000-4-2 level is claimed for it. The 12-pin header `J3` carries FPGA I/O through 100 ohm series resistors and carries no protection device either.

## Layout and fill

The placement is pinned in [`ice40_bga_node.placement.layout.toml`](ice40_bga_node.placement.layout.toml): the USB-C receptacle on the left edge with its opening facing out, the MCU group between the receptacle and the FPGA, the 12-pin header `J3` on the top edge with nothing beyond it, the two mounting holes at opposite corners, and the power parts along the bottom. The courtyard fill of 35.8 % and the outline are measured on the exported board.

## Sourcing

[`fab/ice40_bga_node_bom_orderable.csv`](fab/ice40_bga_node_bom_orderable.csv) has 77 lines: 70 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`fab/sourcing/sourcing.csv`](fab/sourcing/sourcing.csv), and 7 board features (2 mounting holes, 5 test points) that are not parts and carry no identity. Several LCSC pages show different stock figures between snapshots and are noted in the table; the part pages were read in 2026-11 and the stock is not firm. `J3` and `J4` are hand-fit through-hole parts. The BOM-matching findings (`U1` and `Y1` below the 5-board quantity, `J4` without a match) are dispositioned in [`fab/dfm/assembly_dispositions_ceeb7444dd.md`](fab/dfm/assembly_dispositions_ceeb7444dd.md).

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`SHA256SUMS`](SHA256SUMS). The Gerber content hash is `ceeb7444dd` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. the STEP model is exported with KiCad; the STEP check reopens it in OpenCASCADE and compares it with the PCB: 75 components with a body, 2 mounting holes drilled in the board solid, identities and positions match (worst difference 0.0000 mm), outline 55.99 x 44.99 mm against Edge.Cuts 55.99 x 44.99 mm, no body beyond its courtyard by more than 0.25 mm. The geometry hash is `604ace35b8`. The USB-C body is the model KiCad's own `royalblue54L_feather` demo ships for this footprint, since the stock 3D library has none; the test points use the discs in [`models/`](models/).

JLCDFM PCB DFM of the Gerber zip of `fab/` (hash `ceeb7444dd`, report of 2026-11-10: [`fab/dfm/jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf`](fab/dfm/jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf)): 27 Danger items in four checks (pad spacing 1, annular ring 19, via to pad 3, slot width 4), each dispositioned in [`fab/dfm/pcb_dfm_dispositions_ceeb7444dd.md`](fab/dfm/pcb_dfm_dispositions_ceeb7444dd.md) with JLCPCB's own pages. The SMT DFM was not run. The report of the earlier package (`jlcdfm_pcb_dfm_2026-11-06_14-42_99da25a419.pdf`) carries the same Danger counts. The BOM-matching run of 2026-11-06 ([`fab/dfm/jlcpcb_assembly_bom_matching_2026-11-06_99da25a419.md`](fab/dfm/jlcpcb_assembly_bom_matching_2026-11-06_99da25a419.md)) was made on a BOM and a CPL that are byte-identical to the files of `fab/` (SHA-256 `cb76c951ddd3e35b` and `313a1423707ae869`). The [via-in-pad specification](fab/dfm/jlcpcb_via_in_pad_specification.md), the [slot specification](fab/dfm/jlcpcb_slot_specification.md) and the [spacing specification](fab/dfm/jlcpcb_pad_spacing_specification.md) cite JLCPCB's pages for the geometry the process checks.

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The exports are recorded in [`PROVENANCE.md`](PROVENANCE.md).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_bga_node.kicad_sch` (one sheet) | b59dd17df7 | b59dd17df7 | b59dd17df7 | byte-identical |
| `ice40_bga_node.kicad_pro` | 7a05f02f74 | 7a05f02f74 | 7a05f02f74 | byte-identical |
| `ice40_bga_node.kicad_sym` | 5103fb8457 | 5103fb8457 | 5103fb8457 | byte-identical |
| `bom.csv` | 0153fda01c | 0153fda01c | 0153fda01c | byte-identical |
| `pnp.csv` | 96842926ac | 96842926ac | 96842926ac | byte-identical |
| `ice40_bga_node.synth.kicad_pcb` (un-routed) | c16c3b6a9c | c16c3b6a9c | c16c3b6a9c | byte-identical |
| `ice40_bga_node.kicad_pcb` | f55d4b5313 | 642ae4da43 | d04f9bf373 | not byte-identical; identical after the router's and KiCad's random ids are masked (`8d63ecef0e`) |
| Gerber and drill content | ceeb7444dd | ceeb7444dd | ceeb7444dd | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP runs described in [`PROVENANCE.md`](PROVENANCE.md).

## How to check

From this directory, with the KiCad 10 footprint and symbol library tables configured:

```bash
sha256sum -c SHA256SUMS
kicad-cli pcb drc --severity-error kicad/ice40_bga_node.kicad_pcb
kicad-cli sch erc kicad/ice40_bga_node.kicad_sch
SYNTH_USER_REGISTRY_DIR=registry synth validate ice40_bga_node.synth
```

The first command verifies every file below `SHA256SUMS`; the Gerber content hash is in `fab/GERBER_CONTENT_SHA256` and the STEP geometry hash is stated in the sign-off table. `registry/` holds the parts this board needs that the shipped registry does not carry.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, 2026-11-10, on the package in `fab/` (Gerber content hash `ceeb7444dd`). The sign-off is JeffMboya's own. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. It covers, each verified and approved by JeffMboya:

| Item | Signed off |
|---|---|
| PCB and Gerber package | the Gerbers, drill, BOM and CPL of `fab/` and the exported KiCad board, hash `ceeb7444dd`, and the STEP model of `fab/` (geometry hash `604ace35b8`, the STEP check above) |
| Return paths | the 37 non-blocking `W-SYNTH-DRC-016` advisories of the routing report on the slow nets, as listed under "What was measured" |
| ERC | the 6 warnings of the KiCad ERC run |
| DRC | the 74 `lib_footprint_mismatch` warnings of the KiCad DRC run, each a footprint that differs from the library copy while 0 violations and 0 unconnected items remain |
| JLCPCB BOM matching | the BOM matching of 2026-11-06, made on a BOM and a CPL byte-identical to those of `fab/`, with the via-in-pad, slot and spacing specifications in `fab/dfm/` attached to the items they cover |
| JLCDFM PCB DFM | the JLCDFM report of 2026-11-10 for Gerber hash `ceeb7444dd` in `fab/dfm/`, with its 27 Danger items (pad spacing 1, annular ring 19, via to pad 3, slot width 4) as dispositioned in `fab/dfm/pcb_dfm_dispositions_ceeb7444dd.md` against the specifications in `fab/dfm/` |
| Placement | the placement on 55.99 x 44.99 mm, 35.8 % courtyard fill, as pinned in `ice40_bga_node.placement.layout.toml` |
| Parts and values | the BOM identities of `fab/ice40_bga_node_bom_orderable.csv`, including `Y1` (JYJE 3TJ480000FYLBC, `C2924620`) and `U1` (Lattice ICE40HX4K-BG121, `C1552775`) |

Every part of the board is recorded as reviewed by JeffMboya, with the review date and the datasheet or product page, in the part files of `registry/` (`reviewed_by`, `reviewed_at`, `datasheet_url`).

## USB pair: supported speed, impedance estimate and skew

The USB data pair runs from `J1` through the 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11` of `U5` (STM32F103C8). The delivered lengths are 34.3 mm (D+) and 44.2 mm (D-), a 9.9 mm difference. The pair is in the `USB` net class (0.12 mm tracks, 0.127 mm clearance, on `F.Cu` and `B.Cu`), declared 90 ohm differential.

### Supported speed

USB full-speed, 12 Mbit/s, only. The STM32F103x8/xB datasheet (DS5319) lists a USB 2.0 full-speed interface among its features and has a section "USB: Full-speed electrical characteristics" ([ST datasheet copies indexed at radiolocman.com](https://www.radiolocman.com/datasheet/pdf.html?di=184185)); the part has no high-speed PHY, so the board is not specified or tested for 480 Mbit/s.

### What full-speed asks of the pair

- Edge rates: USB transceiver data sheets give the full-speed rise and fall times as 4 to 20 ns, measured between 10 % and 90 % (onsemi/Fairchild USB1T20: [datasheet](https://www.mouser.com/datasheet/2/149/USB1T20-117246.pdf); TI TUSB1210: [datasheet](https://www.ti.com.cn/document-viewer/cn/lit/html/ZHCSKM0J/GUID-1003578D-34E0-47DB-BF8D-514B4C9072EC)). The bit time at 12 Mbit/s is 83.3 ns.
- Impedance: ST's application note AN4879 asks a full-speed platform to route 90 ohm +/-15 % differential and 30 ohm +/-30 % common mode ([AN4879, Rev 1 copy](https://github.com/PaxInstruments/labwiz-board/files/560836/ST.AN4879.pdf)). That is 76.5 to 103.5 ohm.

These are secondary copies of the figures (transceiver data sheets and an application-note copy), not the USB-IF specification text, which I did not read.

### Impedance estimate (analytic, not field-solved)

Edge-coupled microstrip on the outer layer of the declared stackup: dielectric height h = 0.1 mm, relative permittivity 4.4, track width w = 0.12 mm, copper t = 0.035 mm, gap s = 0.127 mm.

- Single-ended, Z0 = 87 / sqrt(er + 1.41) x ln(5.98 h / (0.8 w + t)) = 54.8 ohm.
- Differential, Zdiff = 2 x Z0 x (1 - 0.48 exp(-0.96 s / h)) = 94 ohm.

94 ohm is inside 76.5 to 103.5 ohm. The formulas are the common IPC-2141 approximations; they carry several ohms of error, the solder-mask and the real etched width are not modelled, and no field solver or TDR measurement was run.

### Skew

Effective permittivity 0.475 x 4.4 + 0.67 = 2.76 gives a propagation delay of sqrt(2.76) / 299.8 mm/ns = 5.5 ps/mm. The 9.9 mm difference is about 55 ps, 1.4 % of the 4 ns minimum rise time and 0.07 % of the bit time. At 12 Mbit/s that skew is far inside what the receiver and the 22 ohm series resistors absorb, so the pair is left unmatched; it would matter at 480 Mbit/s, which the part does not run.
