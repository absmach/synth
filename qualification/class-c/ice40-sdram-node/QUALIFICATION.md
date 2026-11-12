# Qualification: iCE40HX4K SDR SDRAM node with a USB-C power input (Class C, case 2)

**Status:** the board is routed and released (60 of 60 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC results, a fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)). The numbers, hashes and files below all come from one export, made with the Synth binary of the commit named in [`PROVENANCE.md`](PROVENANCE.md).

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 72.0 x 67.0 mm, 4,823 mm2, 73 parts (64 excluding mounting holes and test points) |
| Courtyard fill | **28.4 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,447 mm2 of courtyard against about 1,370 mm2 now |
| Stackup | six copper layers, `manufacturer "jlc-advanced"`, JLCPCB's `JLC06161H-3313` stackup as declared in the source (prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um; [JLCPCB impedance tool](https://jlcpcb.com/impedance)), 1.47 mm in the STEP model |
| Routing | 60 of 60 nets, 924 segments, 182 vias (0.46/0.2 mm: 68, 0.6/0.3 mm: 114), 1785.8 mm of track; the router's report and the delivered board agree (the delivered-board count) |
| Impedance | `SDCLK` declares 50 ohm: 0.15 mm tracks on the outer layers over 0.0994 mm of prepreg give Z0 = 87/sqrt(er+1.41) x ln(5.98 h / (0.8 w + t)) = 87/2.347 x ln(0.5944/0.155) = 49.8 ohm (closed-form microstrip estimate, not a field solve). `SDDATA` and `SDADDR` declare no impedance target: the ISSI IS42S16400J-7 AC characteristics retrieved for this board give timing only, the nets are LVTTL with 22 ohm series resistors, and their 0.15 mm width is the class width on every layer |
| Data bus | 8-bit use of the 16-bit part: the SDRAM's upper byte `DQ8` to `DQ15` (the pin row that faces the FPGA) carries data; `SD_LDQM` is held high by the FPGA, which masks the lower byte, and `DQ0` to `DQ7` are tied to ground through one 10 k resistor `RPD1`. IS42S16400J behaviour with a masked byte: ignored on a write, high-impedance on a read |
| Data bus lengths | `DQ8` to `DQ15`: 11.4, 11.6, 11.7, 13.7, 14.4, 15.8, 16.1 and 17.7 mm; spread 6.34 mm against the class budget `max_skew 10 mm` and `max_length 60 mm` |
| Clock | `SD_CK` 7.0 mm from the FPGA to the series resistor plus 10.8 mm beyond it, 17.8 mm in all, against `max_length 40 mm` |
| Address, bank and control group | lengths from 11.2 mm (`SD_CKE`) to 56.3 mm (`SD_BA0`) by the pad-to-pad measure above, a spread of 45.0 mm. The `SDADDR` class declares `max_skew 60 mm` and `max_length 80 mm`, chosen against the IS42S16400J-7 clock cycle: 7 ns minimum at CAS latency 3 (143 MHz), from ISSI's key timing table as shown in a datasheet search result, since the PDF itself could not be fetched. A 60 mm group skew is 0.42 ns at 7.0 ps/mm (stripline, er 4.4), 6.0 % of that cycle; an 80 mm run is 0.56 ns of delay. The address and command setup and hold times of the IS42S16400J were not read, so no setup or hold margin is claimed. Synth's export gate does not run the class skew check (`E-SYNTH-DRC-019`): [#147](https://github.com/absmach/synth/issues/147) (skew budgets are not carried through export) |
| Edge rate | `SDDATA` and `SDCLK` declare `edge_rate fast` (the SDRAM drives the data bus on a read, and no output-edge figure for the IS42S16400J was found, so no slower claim is made). `SDADDR` declares `edge_rate medium`: the FPGA alone drives it, and Lattice's IBIS model `ice40.ibs` rev 2.5 gives 20 to 80 % rise and fall times of at least 0.976 ns (banks 0 to 2, `lvc330io`) and 0.728 ns (bank 3, `lvc330_b3io`) at the fast corner, so no edge is faster than 0.3 ns. The medium window is 6 mm ([#445](https://github.com/absmach/synth/issues/445)) |
| Return-via advisories | 4 non-blocking `W-SYNTH-DRC-016` findings on nets that carry no fast edge (`+3V3` 3, `+1V2` 1), 0 blockers, and no `E-SYNTH-DRC` signal-integrity finding on the routed board; see the return-path table below |
| DRC with the library tables | 0 errors, 0 unconnected, 71 warnings: 70 `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; the footprint comparison shows every difference is silkscreen) and 1 `track_dangling` on `SD_DQ_LO`, the tie of the masked lower byte (a 0.2 mm track end; the board has 0 unconnected items, so the tie reaches the resistor and the pads of its net) |
| ERC with the library tables | 0 errors, 4 warnings: `lib_symbol_issues` (the `Si2301CDS` symbol is not in KiCad's library, #96), `lib_symbol_mismatch` (`AO3400A`, #428) and 2 `pin_to_pin` on the configuration-select pins `CBSEL0` and `CBSEL1`, tied to ground on purpose (select the default configuration) |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The delivered board has 178 such narrowings: 100 tracks narrowed at a terminal ("terminal neck"; 94 of them from 0.4 to 0.3458 mm, 3 from 0.6 to 0.4458 mm, 2 from 0.4 to 0.2878 mm and 1 from 0.6 to 0.5458 mm), 40 tracks from 0.4 to 0.2 mm at fine-pitch taps, 2 from 0.4 to 0.2 mm rescuing a net, and 36 vias from 0.6 mm to 0.46 or 0.56 mm inside pads (the board's via floor is 0.35 mm, drill 0.2 mm, ring 0.075 mm). The engine's own counters (`claimed_total_vias: 137`, `claimed_via_in_pad: 0`) cover only the vias of its routing and fanout steps; Synth's later steps move vias out of the pads of parts that declare no under-pad escape and add plane, return and stitch vias, so the delivered board's own count, 182 vias of which 68 are 0.46/0.2 mm, is the one this README uses. Each is a neck or a via-in-pad at a pad of the part it serves, within the board floors; the engine reports no rule below the board floor . The delivered board is judged by the KiCad DRC above |

### Return paths

The rule `W-SYNTH-DRC-016` asks that a layer-change via on a fast net have a ground via within a window of the thickest dielectric (floor: pad plus clearance); it blocks the release for a fast net and is an advisory for a slow one. Measured on the delivered board, centre to centre:

| Net | Layer change at (mm) | Nearest ground via (mm) | Net edge |
|---|---|---|---|
| `+1V2` | 23.4, 30.2 | 4.13 | slow, advisory |
| `+3V3` | 49.4, 35.5 | 2.55 | slow, advisory |
| `+3V3` | 26.2, 19.2 | 2.00 | slow, advisory |
| `+3V3` | 5.0, 33.7 | 3.24 | slow, advisory |

The 64 vias on the SDRAM signal nets: every `SD_DQ` via is within 0.76 mm of a ground via, the clock vias within 0.73 mm, and the address and control vias within 5.08 mm (median over all 64: 2.2 mm). Ground is poured on all six layers (a `GND` zone on each), and the supply nets are routed as tracks. The supply and configuration nets named in the table carry no fast edge (the net classes `PWR33`, `PWR12` and `LOWSPD` declare `edge_rate slow`), so their findings are advisories, with the nearest ground via at the distances listed.

## Layout and fill

The placement is pinned in [`ice40_sdram_node.placement.layout.toml`](ice40_sdram_node.placement.layout.toml): the SDRAM and the FPGA face each other, the series resistors between them, the USB-C receptacle and the power chain along the bottom, the flash and the programming header on the right. The header `J2` has its courtyard 0.98 mm from the screw-head keepout of mounting hole `H2` (a 5.5 mm square around the hole) and 1.57 mm inside the board's right edge. The courtyard fill of 28.4 % and the outline are measured on the exported board; the parts occupy a 70.4 x 51.5 mm box (75 % of the board) and 9 of the 42 10 x 10 mm cells are empty.

## Sourcing

[`fab/ice40_sdram_node_bom_orderable.csv`](fab/ice40_sdram_node_bom_orderable.csv) has 73 lines: 64 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`fab/sourcing/sourcing.csv`](fab/sourcing/sourcing.csv), and 9 board features (2 mounting holes, 7 test points) that are not parts and carry no identity. `J2` is a hand-fit through-hole part. The part pages were read in 2026-11 and the stock is not firm. The IS42S16400J datasheet at Mouser could not be retrieved: its clock cycle (7 ns at CAS latency 3, -7 grade) is taken from a datasheet search result, and its setup and hold times were not read.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (15 files, including the four inner layers), the placement file, the orderable BOM and the STEP model, with [`SHA256SUMS`](SHA256SUMS). The Gerber content hash is `e4663ac189` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. the STEP model is exported with KiCad; the STEP check reopens it in OpenCASCADE and compares it with the PCB: 71 components with a body, 2 mounting holes, identities and positions agree (worst difference 0.0000 mm and 0.000 degrees), the outline is 71.99 x 66.99 mm as in Edge.Cuts, and no body leaves its courtyard by more than 0.25 mm. The STEP geometry hash is `1a98a33706`. The test points are 1 oz copper discs and the fuse is a block of the Bourns datasheet's mid-range dimensions (A 4.55, B 3.24, C 0.70 mm), both written from datasheet dimensions.

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The exports are recorded in [`PROVENANCE.md`](PROVENANCE.md).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `ice40_sdram_node.kicad_sch` | f8ef51b042 | f8ef51b042 | f8ef51b042 | byte-identical |
| `ice40_sdram_node.kicad_pro` | 2793337457 | 2793337457 | 2793337457 | byte-identical |
| `ice40_sdram_node.kicad_sym` | dd8c3573b5 | dd8c3573b5 | dd8c3573b5 | byte-identical |
| `bom.csv` | d9f2b24a64 | d9f2b24a64 | d9f2b24a64 | byte-identical |
| `pnp.csv` | bc86ec5243 | bc86ec5243 | bc86ec5243 | byte-identical |
| `ice40_sdram_node.synth.kicad_pcb` | d7fe586d67 | d7fe586d67 | d7fe586d67 | byte-identical |
| `ice40_sdram_node.kicad_pcb` | 4782d2ae97 | 53cabf4f37 | 05327a6820 | not byte-identical; identical after the router's and KiCad's random ids are masked (`be5e2fbcbc`) |
| the 10 sub-sheet `.kicad_sch` files | | | | byte-identical in all three exports |
| Gerber and drill content | e4663ac189 | e4663ac189 | e4663ac189 | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP runs described in [`PROVENANCE.md`](PROVENANCE.md).

## How to check

From this directory, with the KiCad 10 footprint and symbol library tables configured:

```bash
sha256sum -c SHA256SUMS
kicad-cli pcb drc --severity-error kicad/ice40_sdram_node.kicad_pcb
kicad-cli sch erc kicad/ice40_sdram_node.kicad_sch
synth validate ice40_sdram_node.synth
```

The first command verifies every file below `SHA256SUMS`; the Gerber content hash is in `fab/GERBER_CONTENT_SHA256` and the STEP geometry hash is stated in the sign-off table. The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/` (Gerber content hash `e4663ac189`, STEP geometry hash `1a98a33706`). The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review. The independent PCB expert sign-off required by #40 is open, and no JLCPCB DFM or BOM-matching run is recorded for this Gerber hash. The self-review covers, each verified and approved by JeffMboya, once Jeff states it:

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Return paths | |
| ERC | |
| DRC | |
| JLCPCB reports | |
| Placement | |
| Parts and values | |

The parts this board adds to `registry/parts/` (`ice40hx4k_tq144`, `header_1x6_prog`, `w25q128jvsiq_208mil`, `tlv70012_ddc`) carry the author's review record (`reviewed_by`, `reviewed_at`, `datasheet_url`); every other part keeps its registry record.
