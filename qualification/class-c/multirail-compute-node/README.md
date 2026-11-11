# Multi-rail compute node with USB-C and a 5 to 12 V input (Class C, case 4)

An STM32F103C8 microcontroller with a 128 Mbit SPI flash and a USB-C receptacle, powered from a 5 to 12 V DC header or from USB VBUS, with a 4 A buck converter, three linear rails, a load switch that sequences them and an expansion header, on four copper layers. It answers #331 under #40: a board whose thermal paths, keepouts, assembly access and DFM limits are declared and visible in the exported output, made by an agent driving `synth mcp`. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request.

**Status:** the board is routed and released (42 of 42 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and the STEP model are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/)). Two parts of the claim are not met by what Synth can declare today: the keepout statement cannot ring a hot part ([#449](https://github.com/absmach/synth/issues/449)), and there is no statement for a thermal-via array, so the array is written as hand-placed vias in the placement sidecar; both are described below.

## What was measured

All figures come from the files of the first of the three exports (KiCad 10.0.6).

| | |
|---|---|
| Outline | 85.0 x 63.0 mm, 5,354 mm2, 86 parts (72 excluding mounting holes, fiducials and test points) |
| Courtyard fill | **20.5 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,606 mm2 of courtyard (the board holds about 1,098 mm2) |
| Stackup | four copper layers, `manufacturer "jlc-filled-via"` with `requires via_in_pad` (resin-filled and capped vias, needed for the thermal vias in the buck's exposed pad), 1.52 mm in the STEP model. The declared stack is 0.035 mm copper, 0.21 mm prepreg (er 4.4), 0.0175 mm copper, 1.065 mm core (er 4.6), 0.0175 mm copper, 0.21 mm prepreg, 0.035 mm copper, which is JLCPCB's 7628 four-layer stack ([JLCPCB stackup page](https://jlcpcb.com/quote/pcbOrderFaq/PCB%20Stackup): 7628 prepreg, 1.065 mm core; the 0.21 mm figure is from a community copy of that stack) |
| Routing | 42 of 42 nets, 473 segments, 125 vias (0.46/0.2 mm: 23, 0.6/0.3 mm: 102), 1297.1 mm of track; the router's report and the delivered board agree ([`reports/delivered_board_counts.json`](reports/delivered_board_counts.json)) |
| Copper | `GND` is poured on all four layers (a zone on each); the rails are routed as tracks, not planes |
| DRC with the library tables | 0 errors, 0 unconnected, 77 warnings, all `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; [`reports/footprint_diff.txt`](reports/footprint_diff.txt) shows no difference in pads, courtyard or fabrication layers) |
| ERC with the library tables | 0 violations |
| Return-via advisories | 22 non-blocking `W-SYNTH-DRC-016` findings on nets that carry no fast edge (`V3V3` 6, `VBUS` 3, `VCC_BUCK` 2, `NRST` 2, `SEQ_ON` 2, `V3V6_SW` 2, `FL_HOLD`, `CC2`, `V3V6`, `FB`, `V1V8` 1 each), 0 blockers |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The delivered board has 10 such narrowings: 8 vias from 0.6 to 0.46 mm at pads (the board's via floor) and 2 tracks narrowed at a terminal (0.6 to 0.5458 mm and to 0.3458 mm). The engine reports no rule below the board floor ([`export/multirail_compute_node.routing.json`](export/multirail_compute_node.routing.json)). The delivered board is judged by the KiCad DRC above |

## The board

| Group | Parts |
|---|---|
| DC input (5 to 12 V) | `J2` 1x2 header; `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 16 V); `D1` Littelfuse SMAJ15A TVS (15 V standoff above the 12.6 V maximum input at +5 %, 24.4 V clamp at 16.4 A, below the buck's 36 V input maximum); `D2` Vishay SS36 Schottky reverse-blocking (60 V, 0.75 V at 3 A) |
| USB input | `J1` Korean Hroparts TYPE-C-31-M-12 receptacle, flush on the left edge, 5 V only (D+ and D- go to the MCU); `R11` and `R12` 5.1 k CC pull-downs; `F2` Bourns MF-MSMF110/16-2; `D3` Nexperia PTVS6V0S1UR (6.0 V standoff above the 5.25 V maximum VBUS); `D4` Diodes 1N5819HW-7-F ORs VBUS into the buck input (40 V, 450 mV at 1 A); `D5` and `D6` Nexperia PESD5V0S1BA on D+ and D-; `D11` and `D12` the same part on CC1 and CC2; `C30` |
| Buck 3.6 V | `U1` Texas Instruments LMR33640ADDA (3.8 to 36 V in, 4 A, internal compensation, EN tied to VIN); `L1` Bourns SRN6045TA-6R8M 6.8 uH (Isat 5.7 A, Idc 3.6 A); input `C1`, `C2` 10 uF 25 V and `C3` 220 nF; output `C4`, `C5` 22 uF 25 V; `C6` 100 nF bootstrap and `C7` 1 uF VCC bypass; the divider `R2` 10 k and `R3` 3.83 k from the 1.0 V feedback reference gives 1.0 V x (1 + 10 / 3.83) = 3.611 V; `R1` pulls PG up. The inductor ripple at 12 V in, 400 kHz is (12 - 3.611) x (3.611 / 12) / (6.8 uH x 400 kHz) = 0.93 A |
| Linear rails and sequencing | `U2` TI LP5907MFX-3.3 (3.3 V, 250 mA) with `C8`, `C9`; `U5` TI TPS22918DBVR load switch (53 mohm at 3.3 V) with `C12`, `C13` (1 nF, rise time), `C14`, and the enable delay `R16` 100 k x `C17` 100 nF = 10 ms; `U3` AMS1117-1.8 behind the switch with `C10`, `C11` and `R4` 330 ohm (5.45 mA minimum load); `U4` TI TLV70012DDCR (1.2 V, 200 mA) enabled by the 1.8 V rail with `C15`, `C16`. Sequence: 3.3 V first, then the switch passes 3.6 V to the 1.8 V regulator, then 1.8 V enables the 1.2 V regulator |
| Compute core | `U6` STMicroelectronics STM32F103C8T6 (LQFP-48) with decoupling `C20` to `C25`; `Y1` JYJE 3TJ480000FYLBC 8 MHz crystal with `C26`, `C27` 10 pF; reset `R9`, `C28`, `SW1`; `R10` pulls BOOT0 low; `R13` 1.5 k USB D+ pull-up; `J3` 1x5 SWD header |
| Memory | `U7` Winbond W25Q128JVSIQ, SOIC-8 208 mil, on SPI1 (`PA4` to `PA7`) with `C29`, `R14`, `R15` |
| Indicators, expansion and fabrication | `D7` red (3.6 V), `D8` green (3.3 V), `D9` and `D10` yellow user LEDs with `R5` to `R8`; `J4` Harwin M20-9990846 1x8 expansion header (3.6 V gated, 3.3 V, 1.8 V, 1.2 V, two GPIOs, two grounds); test points `TP1` to `TP9`; fiducials `FID1` to `FID3`; two M2.5 mounting holes |

The processor was picked from the registry's MCUs (ATmega328P, ESP32-C3 and WROOM-32, nRF52840, RP2350, STM32F103C8, STM32G030 variants) because the board needs a USB full-speed device, a 72 MHz core and an external SPI flash on a hand-solderable 0.5 mm LQFP, which the registry entry `stm32f103c8_usb` (a copy of `stm32f103c8` with the USB pin capabilities) provides. The flash is the registry's 128 Mbit W25Q128 with the 208 mil outline the JVSIQ part has, and the regulator is the DDC package that LCSC lists.

## Thermal

The rails run from the 3.611 V buck output. The dissipation of each linear regulator is its input-output drop times its current; the junction temperature is Tj = TA + P x RthetaJA, with the datasheet's junction-to-ambient resistance (a JEDEC figure that depends on the board; TI states its LMR33640 table value is not a design value). The last two columns give the load current at which Tj reaches 125 C at 50 C and at 85 C ambient.

| Part | Drop | RthetaJA used | At its rated current | Current limit, TA 50 C / 85 C |
|---|---|---|---|---|
| `U2` LP5907 3.3 V | 0.311 V | 193.4 C/W (TI datasheet, DBV) | 78 mW at 250 mA, +15 K | thermally not limiting (1.25 A / 0.67 A against the 250 mA rating) |
| `U3` AMS1117-1.8 | 1.811 V | 90 C/W (original AMS datasheet note) to 150 C/W (vendor datasheets of the part LCSC lists) | 1.81 W at its 1 A rating, +163 to +272 K, beyond 125 C | 0.46 A / 0.25 A (90 C/W); 0.28 A / 0.15 A (150 C/W) |
| `U4` TLV70012 1.2 V | 2.411 V | 262.8 C/W (TI datasheet, DDC) | 482 mW at 200 mA, +127 K, beyond 125 C | 0.118 A / 0.063 A |
| `U5` TPS22918 | 53 mohm x I2 | 183.2 C/W (TI datasheet, DBV) | 53 mW at 1 A, +9.7 K; 212 mW at 2 A, +38.8 K | not limiting |

The board's own loads on the 1.8 V and 1.2 V rails are `R4` (5.45 mA) and nothing else; the rails are exposed on `J4`, so the limits above are the currents an external load may draw from them. `U1`'s junction temperature is not estimated: the efficiency curve of the datasheet was not read, so its dissipation is not known here. The STM32F103C8 junction-to-ambient figure for LQFP-48 is 55 C/W (read from datasheet mirrors).

**Thermal vias.** `U1`'s exposed pad (pin 9, 2.41 x 3.1 mm, tied to `GND`) carries a 2 x 3 array of six 0.46/0.2 mm ground vias at 1.2 mm pitch (x 35.4 and 36.6 mm, y 8.8, 10.0 and 11.2 mm), measured on the export ([`reports/placement_measurements.json`](reports/placement_measurements.json)), inside the pad. Synth has no statement for a thermal array: the vias are `[[vias]]` entries of [`multirail_compute_node.placement.layout.toml`](multirail_compute_node.placement.layout.toml), written before routing so the router sees them as existing `GND` copper. TI's LMR33640 datasheet (section 9.5.1.1, as the Class B buck README cites it) asks for at least 4 x 3 thermal vias; a pad of this size holds six at this pitch. The `GND` pour on all four layers is the copper area; no copper area per dissipating part is declared because Synth has no statement for it.

**Keepouts.** The source declares `keepout hot_buck { radius 5mm }`. The statement takes only a radius, with no part or position, so the export holds no zone for it; the only keepouts in the exported board are the two mounting-hole screw-head zones (`H1 screw head`, `H2 screw head`, 2.75 mm radius, tracks, vias and pours not allowed on all four layers). A keepout ringing a hot part cannot be declared today ([#449](https://github.com/absmach/synth/issues/449)).

## Assembly access and DFM

Measured on the export ([`reports/placement_measurements.json`](reports/placement_measurements.json)):

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

[`fab/multirail_compute_node_bom_orderable.csv`](fab/multirail_compute_node_bom_orderable.csv) has 86 lines: 72 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv), and 14 board features (2 mounting holes, 3 fiducials, 9 test points) that are not parts and carry no identity. Notes that carry a decision: the registry's LCSC number for the LP5907 pointed at a different part, so the table names `C80670`; `C6188`, the registry's number for the AMS1117-1.8, is the adjustable AMS1117-ADJ, so the table names the UMW AMS1117-1.8 (`C347220`), whose pinout against its own datasheet was not read; LCSC has no UNI-ROYAL 3.83 k, so the table names a Yageo (`C482840`); the Bourns inductor is listed by one distributor as being discontinued; the two registry "amber" LEDs are the yellow KENTO KT-0603Y (584 to 594 nm); the PESD5V0S1BA junction capacitance on the USB lines was not read; `J2`, `J3` and `J4` are through-hole hand-fit parts, and `J4` (Harwin) has no LCSC listing, so it is sourced from DigiKey. The part pages were read in 2026-11 and the stock is not firm.

## Fabrication package

[`fab/`](fab/): the Gerbers and drill file (13 files, four copper layers), the placement file, the orderable BOM and the STEP model, with [`fab/SHA256SUMS`](fab/SHA256SUMS). The Gerber content hash is `89d4497fd0` ([`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256)); the hash masks the creation time KiCad writes into the headers. [`agent-run/tools/step_export.sh`](agent-run/tools/step_export.sh) exports the STEP model with KiCad; [`reports/step_verify.txt`](reports/step_verify.txt) reopens it in OpenCASCADE and compares it with the PCB: 81 components with a body, 2 mounting holes and 3 fiducials that carry no body, identities and positions agree (worst difference 0.0000 mm and 0.000 degrees), the outline is 84.99 x 62.99 mm as in Edge.Cuts, and no body leaves its courtyard by more than 0.25 mm. The STEP geometry hash is `f5fe2d3c18`. Models the KiCad library lacks are written by [`agent-run/tools/step_models.py`](agent-run/tools/step_models.py) from datasheet dimensions (the test points as 1 oz copper discs, the 1812 fuses as the Bourns datasheet's mid-range block).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The traces are in [`agent-run/traces/`](agent-run/traces/).

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

Everything committed comes from export 1. The authoritative evidence of how the board was made is the prompts and the agent and MCP traces in [`agent-run/`](agent-run/) (see [`PROVENANCE.md`](PROVENANCE.md)).

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/` (Gerber content hash `89d4497fd0`, STEP geometry hash `f5fe2d3c18`). The sign-off is JeffMboya's own. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. It covers, each verified and approved by JeffMboya, once Jeff states it:

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

Every part of the board that carries no other reviewer in `registry/parts` is recorded as reviewed by JeffMboya (2026-11-11), with the review date and the datasheet or product page where one is sourced, in the part files of `reproduce/user-registry/` (`reviewed_by`, `reviewed_at`, `datasheet_url`).
