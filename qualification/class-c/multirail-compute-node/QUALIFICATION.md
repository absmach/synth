# Qualification: Multi-rail compute node with USB-C and a 5 to 12 V input (Class C, case 4)

**Status:** the board is routed and released (42 of 42 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC results, a fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)). Two parts of the claim are not met by what Synth can declare today: the keepout statement cannot ring a hot part ([#449](https://github.com/absmach/synth/issues/449)), and there is no statement for a thermal-via array, so the array is written as hand-placed vias in the placement sidecar; both are described below. The thermal array is six vias (2 x 3) against the 4 x 3 TI recommends, because the exposed pad cannot hold 12 vias at 1.2 mm pitch, and `U1`'s own junction temperature is not estimated (its efficiency curve was not read). The board is a multi-rail contribution, not a pass of case 4 on keepout or thermal.

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 85.0 x 63.0 mm, 5,354 mm2, 86 parts (72 excluding mounting holes, fiducials and test points) |
| Courtyard fill | **20.5 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,606 mm2 of courtyard (the board holds about 1,098 mm2) |
| Stackup | four copper layers, `manufacturer "jlc-filled-via"` with `requires via_in_pad` (resin-filled and capped vias, needed for the thermal vias in the buck's exposed pad), 1.52 mm in the STEP model. The declared stack is 0.035 mm copper, 0.21 mm prepreg (er 4.4), 0.0175 mm copper, 1.065 mm core (er 4.6), 0.0175 mm copper, 0.21 mm prepreg, 0.035 mm copper, which is JLCPCB's 7628 four-layer stack ([JLCPCB stackup page](https://jlcpcb.com/quote/pcbOrderFaq/PCB%20Stackup): 7628 prepreg, 1.065 mm core; the 0.21 mm figure is from a community copy of that stack) |
| Routing | 42 of 42 nets, 473 segments, 125 vias (0.46/0.2 mm: 23, 0.6/0.3 mm: 102), 1297.1 mm of track; the router's report and the delivered board agree (the delivered-board count) |
| Copper | `GND` is poured on all four layers (a zone on each); the rails are routed as tracks, not planes |
| DRC with the library tables | 0 errors, 0 unconnected, 77 warnings, all `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; the footprint comparison shows no difference in pads, courtyard or fabrication layers) |
| ERC with the library tables | 0 violations |
| Synth validation | `synth validate` exits 0 with 48 warnings (the `synth validate` run); the 19 schematic-layout findings are among them: 9 `E-SYNTH-SCHEM-003` (decoupling capacitors in another schematic sheet group than their IC, up to 455 mm apart on the sheet), 7 `E-SYNTH-SCHEM-001` (a power flag drawn pointing down), 2 `E-SYNTH-SCHEM-004` (a wire longer than 100 mm) and 1 `E-SYNTH-SCHEM-005` (a node with 4 wire lines). They concern the drawing of the generated schematic, not the netlist or the board, and are accepted. `E-SYNTH-CONNECT-009` is explained under "Power good" |
| Return-via advisories | 22 non-blocking `W-SYNTH-DRC-016` findings on nets that carry no fast edge (`V3V3` 6, `VBUS` 3, `VCC_BUCK` 2, `NRST` 2, `SEQ_ON` 2, `V3V6_SW` 2, `FL_HOLD`, `CC2`, `V3V6`, `FB`, `V1V8` 1 each), 0 blockers |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The delivered board has 10 such narrowings: 8 vias from 0.6 to 0.46 mm at pads (the board's via floor) and 2 tracks narrowed at a terminal (0.6 to 0.5458 mm and to 0.3458 mm). The engine reports no rule below the board floor ([`kicad/multirail_compute_node.routing.json`](kicad/multirail_compute_node.routing.json)). The engine's own counters (`claimed_total_vias: 62`, `claimed_via_in_pad: 50`) count the vias of its routing and fanout steps; Synth's later steps move and add vias; the 8 narrowings above are what its strict-size policy measured on the delivered board, whose own counts (125 vias, 23 of them 0.46/0.2 mm) are the ones this README uses. The delivered board is judged by the KiCad DRC above |

## Thermal

The rails run from the 3.611 V buck output. The dissipation of each linear regulator is its input-output drop times its current; the junction temperature is Tj = TA + P x RthetaJA, with the datasheet's junction-to-ambient resistance (a JEDEC figure that depends on the board; TI states its LMR33640 table value is not a design value). The last two columns give the load current at which Tj reaches 125 C at 50 C and at 85 C ambient.

| Part | Drop | RthetaJA used | At its rated current | Current limit, TA 50 C / 85 C |
|---|---|---|---|---|
| `U2` LP5907 3.3 V | 0.311 V | 193.4 C/W (TI datasheet, DBV) | 78 mW at 250 mA, +15 K | thermally not limiting (1.25 A / 0.67 A against the 250 mA rating) |
| `U3` AMS1117-1.8 | 1.811 V | 90 C/W (original AMS datasheet note) to 150 C/W (vendor datasheets of the part LCSC lists; the UMW datasheet of the part the BOM names gives 120 C/W) | 1.81 W at its 1 A rating, +163 to +272 K, beyond 125 C | 0.46 A / 0.25 A (90 C/W); 0.28 A / 0.15 A (150 C/W) |
| `U4` TLV70012 1.2 V | 2.411 V | 262.8 C/W (TI datasheet, DDC) | 482 mW at 200 mA, +127 K, beyond 125 C | 0.118 A / 0.063 A |
| `U5` TPS22918 | 53 mohm x I2 | 183.2 C/W (TI datasheet, DBV) | 53 mW at 1 A, +9.7 K; 212 mW at 2 A, +38.8 K | not limiting |

The board's own loads on the 1.8 V and 1.2 V rails are `R4` (5.45 mA) and nothing else; the rails are exposed on `J4`, so the limits above are the currents an external load may draw from them. `U1`'s junction temperature is not estimated: the efficiency curve of the datasheet was not read, so its dissipation is not known here. The STM32F103C8 junction-to-ambient figure for LQFP-48 is 55 C/W (read from datasheet mirrors).

**Thermal vias.** `U1`'s exposed pad (pin 9, 2.41 x 3.1 mm, tied to `GND`) carries a 2 x 3 array of six 0.46/0.2 mm ground vias at 1.2 mm pitch (x 35.4 and 36.6 mm, y 8.8, 10.0 and 11.2 mm), measured on the export (the placement measurements), inside the pad. Synth has no statement for a thermal array: the vias are `[[vias]]` entries of [`multirail_compute_node.placement.layout.toml`](multirail_compute_node.placement.layout.toml), written before routing so the router sees them as existing `GND` copper. TI's LMR33640 datasheet (section 9.5.1.1, as the Class B buck README cites it) asks for at least 4 x 3 thermal vias; a pad of this size holds six at this pitch. The `GND` pour on all four layers is the copper area; no copper area per dissipating part is declared because Synth has no statement for it.

**Keepouts.** The source declares `keepout hot_buck { radius 5mm }`. The statement takes only a radius, with no part or position, so the export holds no zone for it; the only keepouts in the exported board are the two mounting-hole screw-head zones (`H1 screw head`, `H2 screw head`, 2.75 mm radius, tracks, vias and pours not allowed on all four layers). A keepout ringing a hot part cannot be declared today ([#449](https://github.com/absmach/synth/issues/449)).

## Assembly access and DFM

Measured on the export (the placement measurements):

| Rule | Measured |
|---|---|
| Connector flush on its edge | `J1` courtyard 0.0 mm from Edge.Cuts |
| Headers inboard | `J3` 1.68 mm, `J4` 1.77 mm, `J2` 3.68 mm from the edge |
| Other parts from the edge | the four LEDs 3.81 mm, everything else further |
| Through-hole to SMD pad | at least 2.58 mm (`J3` pin 5 to `TP8`); the JLCPCB rule is 2.1 mm |
| Fiducials | `FID1` to `FID3`, 1 mm with a 2 mm mask opening |
| Mounting hardware | two M2.5 holes, each with its screw-head keepout; the nearest part courtyard is 11.2 mm from `H1` and 11.4 mm from `H2` |
| Vias | 0.46/0.2 mm (ring 0.13 mm) and 0.6/0.3 mm (ring 0.15 mm); the `jlc-filled-via` profile's floors are 0.46 mm via, 0.2 mm drill and 0.13 mm ring, as in the exported project; the 0.20 mm ring of the review checklist applies to 0.6/0.2 mm vias and is not met by these sizes |
| Silkscreen and DRC | 0 silkscreen overlaps; the only DRC warnings are the library-footprint mismatches above |

The net classes set the widths (all declare `edge_rate slow` except `BUCK_SW`, `USB` and `SPI`): `PWR` 0.6 mm (IPC-2221 external 1 oz at 10 K rise: about 1.6 A), `PWR_LOW` 0.3 mm (about 1.0 A), `GNDC` 0.4 mm, `BUCK_SW` 0.6 mm on `F.Cu` (fast, with the switch node), `SIG_SLOW` 0.2 mm, `SPI` (no edge rate declared, so counted fast) and `XTAL` (the crystal nets) 0.2 mm locked to `F.Cu` so they make no layer change and need no return via, and `USB` 0.2 mm.

**USB.** The pair runs `J1` to `PA12` and `PA11` of `U6`, full speed (12 Mbit/s) only, with the 1.5 k pull-up `R13` on D+. Delivered lengths are 84.2 mm (D+) and 78.7 mm (D-), a 5.5 mm difference, on `F.Cu`, `In1.Cu` and `In2.Cu`; at the 5.5 ps/mm used for the case 1 estimate that is about 30 ps, a small fraction of the 4 ns minimum full-speed edge. The `USB` class declares `edge_rate medium`: transceiver datasheets give the full-speed rise and fall times as 4 to 20 ns (10 to 90 %; onsemi USB1T20, TI TUSB1210), far above 0.3 ns. Synth's rule treats a differential pin as fast; the Synth commit named in [`PROVENANCE.md`](PROVENANCE.md) lets a declared `medium` replace that inference. No `diff_pair` is declared for the pair.

**Power good.** `U1`'s `PG` pin (open drain) is the net `PG_3V6`, which goes to `PA0` of `U6` and is pulled up by `R1`, 10 k, to the `V3V3` rail: 3.3 V / 10 k = 0.33 mA when `PG` pulls low. The validator warning `E-SYNTH-CONNECT-009` (open-drain net without a pull-up) names this net because the source declares `V3V3` with `net`, not `power`, and the rule counts only a resistor to a declared power net as a pull-up; the resistor is on the board.

## Sourcing

[`fab/multirail_compute_node_bom_orderable.csv`](fab/multirail_compute_node_bom_orderable.csv) has 86 lines: 72 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`fab/sourcing/sourcing.csv`](fab/sourcing/sourcing.csv), and 14 board features (2 mounting holes, 3 fiducials, 9 test points) that are not parts and carry no identity. Notes that carry a decision: the registry's LCSC number for the LP5907 pointed at a different part, so the table names `C80670`; `C6188`, the registry's number for the AMS1117-1.8, is the adjustable AMS1117-ADJ, so the table names the UMW AMS1117-1.8 (`C347220`), whose own datasheet ([`fab/sourcing/umw_ams1117_datasheet.pdf`](fab/sourcing/umw_ams1117_datasheet.pdf), UMW AMS1117, "Pinning information") gives the SOT-223 pinout 1 GND/ADJ, 2 VOUT, 3 VIN and lists the UMW AMS1117-1.8 in SOT-223; the registry part `ams1117_1v8` matches it (the same datasheet gives 18 V absolute maximum input, 12 V recommended, and 120 &deg;C/W junction-to-ambient for the SOT-223); LCSC has no UNI-ROYAL 3.83 k, so the table names a Yageo (`C482840`); the Bourns inductor is listed by one distributor as being discontinued; the two registry "amber" LEDs are the yellow KENTO KT-0603Y (584 to 594 nm); the PESD5V0S1BA junction capacitance on the USB lines was not read; `J2`, `J3` and `J4` are through-hole hand-fit parts, and `J4` (Harwin) has no LCSC listing, so it is sourced from DigiKey. The part pages were read in 2026-11 and the stock is not firm.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (13 files, four copper layers), the placement file, the orderable BOM and the STEP model, with [`SHA256SUMS`](SHA256SUMS). The Gerber content hash is `89d4497fd0` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. the STEP model is exported with KiCad; the STEP check reopens it in OpenCASCADE and compares it with the PCB: 81 components with a body, 2 mounting holes and 3 fiducials that carry no body, identities and positions agree (worst difference 0.0000 mm and 0.000 degrees), the outline is 84.99 x 62.99 mm as in Edge.Cuts, and no body leaves its courtyard by more than 0.25 mm. The STEP geometry hash is `f5fe2d3c18`. Models the KiCad library lacks are written from datasheet dimensions (the test points as 1 oz copper discs, the 1812 fuses as the Bourns datasheet's mid-range block).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The exports are recorded in [`PROVENANCE.md`](PROVENANCE.md).

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `multirail_compute_node.kicad_sch` | 755bb317dd | 755bb317dd | 755bb317dd | byte-identical |
| `multirail_compute_node.kicad_pro` | 86e0d6fc00 | 86e0d6fc00 | 86e0d6fc00 | byte-identical |
| `multirail_compute_node.kicad_sym` | 692e828452 | 692e828452 | 692e828452 | byte-identical |
| `bom.csv` | 3c09c9ee8b | 3c09c9ee8b | 3c09c9ee8b | byte-identical |
| `pnp.csv` | c9336f28ac | c9336f28ac | c9336f28ac | byte-identical |
| `multirail_compute_node.synth.kicad_pcb` | 1ddeac5ca3 | 1ddeac5ca3 | 1ddeac5ca3 | byte-identical |
| `multirail_compute_node.kicad_pcb` | 37f474efeb | 89094a8305 | 1ba9ed1c4a | not byte-identical; identical after the router's and KiCad's random ids are masked (`639faf6090`) |
| Gerber and drill content | 89d4497fd0 | 89d4497fd0 | 89d4497fd0 | identical after the creation time in the headers is masked |

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP runs described in [`PROVENANCE.md`](PROVENANCE.md).

## How to check

From this directory, with the KiCad 10 footprint and symbol library tables configured:

```bash
sha256sum -c SHA256SUMS
kicad-cli pcb drc --severity-error kicad/multirail_compute_node.kicad_pcb
kicad-cli sch erc kicad/multirail_compute_node.kicad_sch
synth validate multirail_compute_node.synth
```

The first command verifies every file below `SHA256SUMS`; the Gerber content hash is in `fab/GERBER_CONTENT_SHA256` and the STEP geometry hash is stated in the sign-off table. The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/` (Gerber content hash `89d4497fd0`, STEP geometry hash `f5fe2d3c18`). The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review. The independent PCB expert sign-off required by #40 is open, and no JLCPCB DFM or BOM-matching run is recorded for this Gerber hash. The self-review covers, each verified and approved by JeffMboya, once Jeff states it:

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Power and protection | |
| Thermal | |
| Return paths | |
| ERC | |
| DRC | |
| JLCPCB reports | |
| Placement | |
| Parts and values | |

The parts this board adds to `registry/parts/` (`tlv70012_ddc`, `w25q128jvsiq_208mil`, `xtal_8mhz_3225_jyje_4p`) carry the author's review record (`reviewed_by`, `reviewed_at`, `datasheet_url`); `stm32f103c8` gains the `usb_dn`, `usb_dp` and `spi_cs` capability tags the board's checks read, with PA11 and PA12 typed as the differential USB pair; every other part keeps its registry record.
