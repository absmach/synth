# Irrigation controller

A two-zone irrigation controller: 12 V in through a polyfuse, an AO3401A
reverse blocker with a gate clamp and an SMAJ13A TVS, an MP2307 buck to 3.3 V,
an STM32G030, two low-side solenoid drivers, two switched resistive soil probes,
a flow-pulse input, an I2C header, an ambient light sensor and an EEPROM, an SWD
header, and rail decoupling, bleeders, snubbers, indicator LEDs and test points.
Two layers, JLCPCB, 75.1 x 65.4 mm, 126 components. There is no humidity sensor
on the board: a breakout attaches to the I2C header at address 0x44.

**Status:** the board is routed and released (45 of 45 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, the DRC and ERC reports, a fabrication package and a reproduction script are committed ([`export/`](export/), [`reports/`](reports/), [`fab/`](fab/), [`reproduce/`](reproduce/)). The numbers, hashes, renders and files below all come from one export, made with `absmach/synth` main at `a12aa289` (the merge of Synth pull request #269, whose router fix counts the routing report's statistics from the delivered board), the same revision as `PROVENANCE.md`, `reproduce/pins.env` and the end of this file.

The board is the output of an agent driving `synth mcp` from a prompt. [`PROVENANCE.md`](PROVENANCE.md) is the record of that path: the prompts, the agent and tool-call traces, the Synth revisions, the registry inputs, the artifact hashes and the validation commands with their results. The agent runs, the scripts and the small scripted edits are described in [`agent-run/HOW_THIS_BOARD_WAS_MADE.md`](agent-run/HOW_THIS_BOARD_WAS_MADE.md).

## What was measured

All figures come from the files of the first of the three exports below
(KiCad 10.0.6, Synth commit `a12aa289`).

| | |
|---|---|
| Board | 75.1 x 65.4 mm, 4,912 mm2, two layers, 126 components (106 excluding holes and test points), 45 nets |
| Routing | state `routed`, 45 of 45 nets connected, 698 segments and 152 vias in the file: 151 of 0.6 mm over a 0.2 mm drill (ring 0.20 mm) and 1 of 0.46 mm over 0.2 mm (ring 0.13 mm) on `+3V3` at (56.5, 47.5); the project's `min_through_hole_diameter` is 0.2 and `min_via_diameter` 0.46 |
| KiCad DRC, 10.0.6, `--refill-zones --severity-all`, with the footprint library table configured | 0 errors, 0 unconnected, 121 `lib_footprint_mismatch` warnings (below); 0 `silk_overlap`, 0 `track_dangling`. Report: [`reports/kicad_drc.json`](reports/kicad_drc.json) |
| Router state | state `routed`, `fabrication_ready` true, no blocking reason, no via in a pad, 6 non-blocking return-via findings ([`export/irrigation_controller.routing.json`](export/irrigation_controller.routing.json)) |
| Trace width, as routed | `VIN_FUSED`, `VALVE1_SW`, `VALVE2_SW` 0.8 mm; `+12V_VALVE` 0.2 to 0.8 mm; `+3V3` 0.2 to 0.4 mm; the other signals 0.2 mm |
| Fine-pitch escape | `U1` (SOIC-8, exposed pad) and `U2` (LQFP-32) declare `escape { off }` in the source: their pads are routed from the surface, and no via sits in a pad |
| BOM | 106 fitted lines, each with a manufacturer, MPN, distributor, order number and page, and 20 board features (mounting holes and test points) that are not parts (below) |
| Schematic | a root sheet and 13 group sheets, one per named group, no `OTHER` region; rendered from this export with `kicad-cli`. KiCad's ERC ([`reports/kicad_erc.json`](reports/kicad_erc.json)), symbol and footprint library tables configured: 0 errors and 6 warnings, explained below. The `+12V_VALVE` rail carries a `PWR_FLAG` |

The 121 `lib_footprint_mismatch` warnings are footprints whose silkscreen the exporter cut back from a via or a pad (0.25 mm clearance, above JLCPCB's 0.15 mm minimum; [absmach/synth#425](https://github.com/absmach/synth/issues/425)), so they differ from the library copy; the pads, courtyard and fabrication outline of every one are identical to the library file ([`reports/footprint_diff.txt`](reports/footprint_diff.txt), from [`footprint_diff.py`](agent-run/tools/footprint_diff.py)). DRC and ERC were run by [`library_check.sh`](agent-run/tools/library_check.sh) with the tables [`reports/fp-lib-table`](reports/fp-lib-table) and [`reports/sym-lib-table`](reports/sym-lib-table). The five ERC library warnings: `Q1`, `Q_E1` and `Q_E2` are P-channel MOSFETs whose registry part names the symbol `Si2301CDS`, which KiCad 10.0.6's `Transistor_FET` library does not contain, so the exporter draws its own symbol ([absmach/synth#96](https://github.com/absmach/synth/issues/96)); `Q_V1` and `Q_V2` carry the `AO3400A` symbol, which the library defines as an extension of `Q_NMOS_GSD`, and the schematic embeds it with the parent's graphics included ([absmach/synth#428](https://github.com/absmach/synth/issues/428)). 

## Layout

Measured on the exported file:

| | |
|---|---|
| Courtyard fill | 30.9 % of the board; 1 of 42 10 x 10 mm cells is empty |
| Connectors | all eight courtyards 0.00 to 0.23 mm from the board edge; the pin header `J_SWD` sits 0.23 mm inboard |
| Mounting holes | one in each corner: from the left or right edge and from the top or bottom edge, `H1` 5.95 and 4.95 mm, `H2` 5.15 and 4.95, `H3` 5.45 and 4.95, `H4` 5.15 and 4.95 |
| Per-pin decoupling, pad to pin | `C_IN1` 3.3 and `C_IN2` 3.5 mm from `U1.2` (centre to pin 3.60 and 3.87 mm; they were 5.3 and 8.4), `C_U4` 2.8 mm from `U4.1`, `C_U5` 3.2 mm from `U5.8`, `C_VDD1` 3.6 mm from `U2.4`, `C_VDD2` 7.8 mm from `U2.4` (the 4.7 uF bulk capacitor of ST's reference supply scheme) |
| Regulator | `C_OUT2` 11.6 mm and `C_OUT1` 8.8 mm from `U1`, origin to origin |
| Crystal, reset button | 9.0 mm and 11.6 mm from the MCU (the reset button was 16.7 mm) |
| SWD header | 43.8 mm from the MCU, at the right edge |
| Through-hole to SMD pad | the nearest pairs: `C_BULK` to `R_SN1` 2.28 mm and `C_BULK` to `R_B1` 2.48 mm (JLCPCB's DFM treats 2.1 mm as a Danger; the nearest pair was 1.9 mm); `TP_SWDIO` and `TP_SWCLK` were moved from 2.05 mm to at least 2.9 mm of `J_SWD` |
| Test points | 2.1 to 7.1 mm from the nearest part on their net, except `TP_SCL` 12.1 and `TP_3V3` 16.0 mm |
| Rail capacitors | the capacitors that sit far from an IC (mean 12.5 mm over 37) are rail bulk and filter parts, not per-pin decoupling |
| Isolated parts | `F1`, `J1` and `R_N6`, 4.1 to 4.8 mm from their neighbours |

The SWD header is 43.8 mm from the MCU on purpose: it is a pin header on the
board edge so a probe cable reaches it without crossing the board, and the MCU
sits mid-board. The reset button is 11.6 mm from the MCU: reset is a slow
input behind a pull-up and a capacitor, so the track length does not affect its
timing. `C_VDD2`, the 4.7 uF capacitor of ST's reference supply scheme, is 7.8 mm from `U2.4`; the placement review flags no decoupling capacitor.

## The review of this board

SammyOina's review of 2026-10-29 (13:51), then the earlier ones, point by point:

| Review point | State |
|---|---|
| DFM findings fixed, package regenerated | In the file: 151 of the 152 vias are 0.6 mm over a 0.2 mm drill (ring 0.20 mm) and 1 is 0.46 mm over 0.2 mm (ring 0.13 mm); the nearest through-hole to SMD pad pair is 2.28 mm. The package in `fab/` is generated from this export (Gerber content hash `1c84c56d98`). |
| Stale schematic evidence | The schematic is a root sheet and 13 group sheets, one per named group, with no `OTHER` region, rendered from the final export; renders, hashes, numbers and the fab package all come from the same export (below). The renders of this export (schematic and PCB, top, bottom and angled) are generated from it and are posted by the repository owner; they are not committed. |
| Input protection rating | **Input requirement.** The supply is 12 V +/-5 %, 10.8 to 12.6 V in operation, and the qualified maximum is 12.6 V; the TVS does not conduct at its 13.0 V standoff, which the checks below use as an upper bound. The surge requirement is at most 18.6 A peak (10/1000 us), which the source impedance sets: the open-circuit surge the clamp covers is 21.5 V + 18.6 A x R_source, 40.1 V at 1 ohm and 58.7 V at 2 ohm. **Protection.** `D_TVS` is an SMAJ13A (Littelfuse SMAJ series datasheet: standoff 13.0 V; breakdown 14.40 to 15.90 V at 1 mA; 1 uA maximum leakage at the standoff; 21.5 V clamp at 18.6 A) on `VIN_FUSED`, ahead of `Q1`, so a transient is clamped before it reaches the MOSFET. The operating maximum is 0.4 V (3.2 %) below the standoff; the clamp is below the MP2307's 23 V recommended input and its 26 V absolute maximum (MPS datasheet) and below `Q1`'s 30 V drain-source rating. A reversed supply forward-biases `D_TVS` and `F1` opens. The 5.1 V gate-source zener `D_QG` is inside the 12 V gate rating. |
| Decouplers at their pins | Fixed: `C_IN1` 3.3 mm and `C_IN2` 3.5 mm pad to `U1.2` (the placement review measures centre to pin: 3.60 and 3.87 mm against its 4 mm limit), `C_U5` 3.2 mm, `C_U4` 2.8 mm, `C_VDD1` 3.6 mm; the review now flags no decoupling capacitor. The input capacitors are 0805 parts. `C_VDD2`, the 4.7 uF capacitor of ST's reference scheme, is 7.8 mm from `U2.4`; the datasheet asks the 100 nF `C_VDD1` (3.6 mm) to be close, and the placement review does not flag `C_VDD2`. |
| LED, bleeder and crystal values | Datasheets opened (2026-10-30). **LED:** Lite-On LTST-C190KGKT, Spec DS22-2000-074 Rev N, via LCSC's copy: AlInGaP green, VF 1.9 to 2.4 V at 20 mA (no typical; bins 4 to 8 in 0.1 V steps, each +/-0.1 V), 30 mA DC, 80 mA peak, 75 mW, 5 V reverse, peak 574 nm, 18 to 71 mcd at 20 mA. The green LEDs were a NationStar NCD0603G1 (Vf 2.6 to 3.6 V on its LCSC page), which cannot light from 3.3 V through 680 ohm. `D_PWR`, `D_N2`: (3.3 - 2.4) / 680 = 1.3 mA to (3.3 - 1.9) / 680 = 2.1 mA, from the 20 mA limits (the datasheet gives VF only at 20 mA); 14 times under the 30 mA maximum. The datasheet specifies brightness only at 20 mA; Fig. 4 puts 1 to 2 mA at roughly a tenth of it (graph read, approximate), about 1 to 11 mcd. **Crystal:** JYJE 3TJ480000FYLBC (LCSC C2924620), the manufacturer's specification sheet dated 2021-11-18, [`reports/jyje_3tj480000fylbc_specification_2021-11-18.pdf`](reports/jyje_3tj480000fylbc_specification_2021-11-18.pdf) (`8588ccc71462`): 8.000 MHz fundamental, CL 8 pF, +/-10 ppm at 25 C, +/-20 ppm over -40 to +85 C, ESR 150 ohm maximum, **shunt capacitance C0 5 pF maximum**, drive 100 uW, 3.2 x 2.5 mm, pads 1 and 3 the crystal and 2 and 4 ground (the sheet's connection drawing), suggested land 1.4 x 1.2 mm at a 2.2 x 1.7 mm pitch, the pad centres of the library footprint. The LCSC page lists +/-30 ppm stability where the sheet says +/-20 ppm, and 56 pieces in stock on 2026-11-05. The sheet gives no stray capacitance. **Stray capacitance:** the STM32G030 datasheet (DS12991 Rev 4, Table 36 note, p. 52) says PCB and MCU pin capacitance must be included and that 10 pF can be used as a rough estimate of the combined pin and board capacitance; its I/O pin capacitance is 5 pF typical. Read as 10 pF at each oscillator pin, 6.2 pF load capacitors give CL = (6.2 + 10) / 2 = 8.1 pF, the same 8.1 pF as the source notes' 6.2 / 2 + 5 (5 pF in the series formula is 10 pF per pin). The datasheet does not say which reading it means; ST's application note AN2867, which would, could not be downloaded (st.com refuses it), so the 5 pF is consistent with ST's figure but not confirmed. **Start-up margin:** ST's maximum critical crystal transconductance is 1.5 mA/V (Table 36). With the crystal's own maximum ESR of 150 ohm and maximum C0 of 5 pF, the effective load CL = 8.1 pF and 8 MHz, the standard formula gm_crit = 4 ESR (2 pi f)^2 (C0 + CL)^2 (from AN2867, not opened) is 0.26 mA/V, a margin to ST's 1.5 mA/V of 5.8 times, and gm_crit stays under 1.5 mA/V for any C0 up to 23 pF, 4.6 times the stated maximum. The ESR and C0 are those of this crystal's own specification, not of a sibling part. No oscillator start-up measurement with this board's STM32G030, crystal and load capacitors has been made. `D_VALVE` (KENTO KT-0603Y, Vf 2.4 V on its page) from an MCU pin through 680 ohm: 1.3 mA, the pin's own drop not counted, against the 15 mA the STM32G030 datasheet (DS12991, Table 19) allows any I/O pin to source. `D_N1` from 13.0 V through 2.2 k: (13.0 - 2.4) / 2.2 k = 4.8 mA, 51 mW in a 100 mW resistor. Bleeders are at 13.0 V in the source notes (`R_B1` 100 k: 0.13 mA). |
| 5 V clamps on 3.3 V signals | `FLOW_PULSE` is clamped by `D_FZ`, a 3.3 V zener, behind `R_FS` 2.2 k: (13.0 V - 3.3 V) / 2.2 k = 4.4 mA, 14 mW in a 500 mW part. |
| SHT30 footprint and escape | A breakout attaches to `J_I2C` at 0x44. There is no fine-pitch part on the board. |
| BOM and fabrication output | 126 of 126 lines sourced; Gerbers, drill and pick-and-place generated and hashed (below). |
| DRC with the library table | 0 errors, 0 unconnected, 121 `lib_footprint_mismatch` warnings and no `silk_overlap` or `track_dangling`, above; machine-readable in [`reports/kicad_drc.json`](reports/kicad_drc.json). |
| Return-path advisories | The routing report lists 6 non-blocking `W-SYNTH-DRC-016` findings, on `I2C_SDA` (2), `+3V3`, `SWCLK` and `+12V_VALVE` (2), checked in "Return paths" below. |
| SWD and reset distance | see the layout table: the SWD header is described above; the reset button is 11.6 mm from the MCU (it was 16.7 mm). |
| Crystal load | the JYJE 3TJ480000FYLBC has an 8 pF load. With 5 pF assumed for stray capacitance, CL = C1 x C2 / (C1 + C2) + 5 pF; C1 = C2 = 2 x (8 - 5) = 6 pF; the nearest E24 value 6.2 pF gives 8.1 pF. `C_X1` and `C_X2` are 6.2 pF. The 8 pF is from the JYJE specification sheet; the 5 pF is the series-formula equivalent of the 10 pF per pin ST's STM32G030 datasheet gives as a rough estimate, not confirmed (see "LED, bleeder and crystal values" above). |
| Valve current | stated in the source as a 12 V solenoid at most 0.5 A: the `PWR12` class is 0.8 mm; the AO3400A (5.7 A on its listing) and the 1N5819HW flyback diode (1 A) are above it. Calculated from listing figures, not measured. |
| Schematic references and groups | every part is in a named group; the `OTHER` region is gone. |
| MP2307 divider | VFB 0.925 V (0.900 to 0.950 V, MPS datasheet); the divider and compensation are calculated from it. |

### Input protection margins

Figures from the Littelfuse SMAJ series datasheet (`D_TVS`, SMAJ13A), the MPS MP2307 datasheet (`U1`) and the Alpha & Omega AO3401A datasheet (`Q1`). The supply is a nominal 12 V source, 12 V +/-5 % (10.8 to 12.6 V) in operation and qualified to 12.6 V (the checks use the 13.0 V standoff as an upper bound); the surge requirement is at most 18.6 A peak, set by the source impedance.

| Stress point | Worst case | Limit | Margin |
|---|---|---|---|
| `D_TVS` standing voltage | 12.6 V qualified maximum, 13.0 V checked as an upper bound | standoff 13.0 V; leakage 1 uA maximum at the standoff; breakdown 14.40 V minimum at 1 mA | no conduction at 12.6 V (0.4 V, 3.2 % below the standoff) or at 13.0 V; breakdown starts 1.8 V (14 %) above the operating maximum and 1.4 V (10.8 %) above 13.0 V |
| `U1` input in operation | 12.6 V (13.0 V checked) | 4.75 to 23 V recommended | 10.4 V below the recommended maximum at 12.6 V (10.0 V at 13.0 V) |
| `U1` input in a surge | 21.5 V, the SMAJ13A clamp at its rated 18.6 A peak pulse (10/1000 us); the clamp is lower at smaller surge currents | 23 V recommended, 26 V absolute maximum | 1.5 V (6.5 %) below the recommended maximum, 4.5 V (17 %) below the absolute maximum |
| `Q1` drain-source in a surge | `D_TVS` is ahead of `Q1`, so `Q1` sees at most 21.5 V | 30 V | 8.5 V (28 %) |
| `Q1` gate-source | `D_QG` holds the gate within 5.1 V of the source | 12 V | 6.9 V |
| Surge current | the TVS current is (V_open-circuit - 21.5 V) / R_source: at most 18.6 A for a source of 1 ohm up to 40.1 V open-circuit or 2 ohm up to 58.7 V | 18.6 A, 10/1000 us | at the required 18.6 A the clamp holds the rail at 21.5 V, so `U1` sees 21.5 V and `Q1` 21.5 V; both stay inside the margins above |

JeffMboya accepts these margins for the stated source: 12 V +/-5 % (12.6 V maximum), a surge of at most 18.6 A (10/1000 us), TVS leakage of at most 1 uA at the standoff, and the MP2307's 23 V recommended and 26 V absolute maximum input; the standoff margin is 0.4 V (3.2 %) and the clamp margin 1.5 V (6.5 %), and a source or surge outside them is outside the qualified envelope.

The SMAJ13A is the largest standard SMAJ standoff whose clamp stays under the MP2307's recommended input: the SMAJ14A clamps at 23.2 V (17.2 A), above 23 V. A source that can exceed 12.6 V in operation, or that cannot be held to 18.6 A, needs the regulator input moved to a higher-rated part.

## JLCPCB

PCB DFM of the package of this README (JLCDFM, 2026-11-04 16:46, Gerber content hash `1c84c56d98`, two layers, 7.51 x 6.54 cm, report in [`reports/`](reports/jlcdfm_pcb_dfm_2026-11-04_16-46_1c84c56d98.pdf)): 0 Danger items. Trace width 0.2 mm, pad spacing 0.16 mm, through-hole to SMD 3.25 mm, silkscreen to pad 0.25 mm and silkscreen line width 0.18 mm are good.

JLCDFM with SMT DFM ticked, on the package of this README (report in [`reports/`](reports/jlcdfm_pcb_and_smt_dfm_2026-11-06_13-59_1c84c56d98.pdf), summary in [`reports/jlcdfm_smt_dfm_2026-11-06_1c84c56d98.md`](reports/jlcdfm_smt_dfm_2026-11-06_1c84c56d98.md), dispositions in [`reports/jlcdfm_smt_dispositions_1c84c56d98.md`](reports/jlcdfm_smt_dispositions_1c84c56d98.md)): the PCB DFM has 0 Danger items and the SMT DFM has 58 Danger items in nine checks. The report's samples show the through-hole JST connectors and the SOT-23 parts beside them, and the dispositions record what the package's own evidence shows for each check; the PDF does not say which item belongs to which component, so the 58 are not all attributed. Each no-match line and each line with stock below the quantity for 5 boards has its disposition against the Gerber content hash `1c84c56d98` in [`reports/assembly_dispositions_1c84c56d98.md`](reports/assembly_dispositions_1c84c56d98.md). Quote with assembly of 2026-11-04, kept as a historical figure (it was taken before the crystal `Y1` became the JYJE 3TJ480000FYLBC; the BOM-matching run of 2026-11-06 estimates $90.9978 for 5 assembled boards and is not a quote) ([`reports/jlcpcb_quote_2026-11-04.md`](reports/jlcpcb_quote_2026-11-04.md), Standard PCBA, no order placed, Gerber content hash `1c84c56d98`): PCB $4.00 and PCBA $129.95, $133.95 in total.

BOM matching of the same package in the JLCPCB assembly flow ([`reports/jlcpcb_assembly_bom_matching_2026-11-04.md`](reports/jlcpcb_assembly_bom_matching_2026-11-04.md), original [`.xlsx`](reports/jlcpcb_assembly_bom_matching_2026-11-04.xlsx)): 100 of 106 lines match a part in JLCPCB stock, including the 10 uF 25 V 0805 capacitors `C_IN1`, `C_IN2`, `C_N5` and `C_N7` (Murata GRM21BR61E106KA73L, LCSC `C84416`, 28 in stock). `R_COMP` (ROHM MCR03EZPFX9101, `C162000`) matches with 0 in stock, and so did `Y1` in that run, which carried the NDK NX3225GD-8MHZ-STD-CRA-3; `C_BULK`, `J_SWD`, `U4` and `U5` have no valid match: the tool suggests `C2941301` for `C_BULK`, an SMD can, while the board uses the radial through-hole `CP_Radial_D6.3mm_P2.50mm`, so `C_BULK` is a hand-fitted through-hole part. A second match of the same package ([`reports/jlcpcb_assembly_bom_matching_2026-11-06.md`](reports/jlcpcb_assembly_bom_matching_2026-11-06.md), original [`.xlsx`](reports/jlcpcb_assembly_bom_matching_2026-11-06.xlsx)) carries `Y1` as the JYJE 3TJ480000FYLBC (`C2924620`, Extended, 3 in stock against 5 needed for 5 boards): 102 of 106 lines match; `R_COMP` (0 in stock), `L1` (1 in stock against 7) and `Y1` have stock below the quantity for 5 boards; `C_BULK` (as above), `J_SWD`, `U4` and `U5` have no valid match. The Gerber content hash `1c84c56d98` of the matched package is the hash of this package.

## Power copper and the routing report

The routing report ([`export/irrigation_controller.routing.json`](export/irrigation_controller.routing.json)) records `engine_policy_verdict: rejected` next to `fabrication_ready: true`. The two statements are about different checks. The routing engine runs with its strict-size policy, which rejects any track narrower than the width its net class declares, and it reported 12 features on 3 nets below the requested size, the narrowest track 0.2 mm and the narrowest via 0.5 mm. Synth records that verdict and then checks the delivered board itself: connectivity (45 of 45 nets), the process floor of 0.127 mm for track width and clearance, KiCad DRC with the library tables (0 errors, 0 unconnected) and the return-via rule. The board passes all of them, so the delivered board is valid; the narrowed sections are the ones listed here, each with the current it carries. The 77 vias the engine claimed are its routing vias; the file holds 152 because Synth's exporter and repair passes add the ground vias. The routing report is written after Synth's repair passes, so it describes the delivered board: 698 segments, 152 vias and 1,434.0 mm of track, with 45 of 45 nets connected, in both. [`reports/delivered_board_counts.json`](reports/delivered_board_counts.json), made by [`delivered_counts.py`](agent-run/tools/delivered_counts.py) from the delivered board (hash `7ed9ef4255`), counts the board itself and sets it beside the report: 698 segments, 152 vias (151 of 0.6 mm over a 0.2 mm drill and 1 of 0.46 mm), 1,434.0 mm of track, 0 DRC errors and 0 unconnected. The router reads the counts again after the repairs in Synth commit `a12aa289` ([absmach/synth#424](https://github.com/absmach/synth/issues/424)).

| Net | Declared | Narrower segments | Narrowest | Current the narrowest carries (IPC-2221, 1 oz external) | Load |
|---|---|---|---|---|---|
| `+12V_VALVE` | 0.8 mm | 49, from 0.2 to 0.72 mm, 20.8 mm in all | 0.2 mm (4 segments, 5.8 mm, carrying at most 0.01 A); the 0.4 mm sections (13 segments, 12.0 mm) carry the most, 1.08 A | 7.4 C at 1.08 A in the 0.4 mm sections (IPC-2221, 1 oz external); 0.74 A gives a 10 C rise in a 0.2 mm track | two valves of at most 0.54 A each at 13.0 V and 0.1 A for the rest of the rail (1.18 A in all) |
| `+3V3` | 0.4 mm | 13 of 0.2 mm, 4.3 mm in all, the longest 0.90 mm | 0.2 mm | 0.74 A at a 10 C rise | the MCU, the light sensor, the EEPROM and two LEDs of about 2 mA each |

**Qualification limitation, recorded.** The delivered board does not meet the declared routing policy for `+12V_VALVE`: 49 of its 137 segments (20.8 mm) are narrower than the declared 0.8 mm, between 0.2 and 0.72 mm, and the routing engine's strict-size verdict is `rejected`. The board is therefore not an ordinary pass of the declared policy. The deviation is qualified by current: the worst-case rise is 7.4 C (IPC-2221, 1 oz external, against a 10 C basis), at the assumed simultaneous load of both valves at 0.54 A each and 1.18 A in all, with no duty-cycle credit.

Every section of `+12V_VALVE` is qualified at its own current, not at the net's total. [`agent-run/tools/valve_path.py`](agent-run/tools/valve_path.py) solves the DC copper network of the delivered board (1 oz, 35 um copper, 1.72e-8 ohm m; every segment a resistor of its width and length, copper that overlaps joined, vias and pads shorts) from the source, `Q1` pad 2, to the loads, and prints the current and the IPC-2221 temperature rise of each of the 137 segments ([`reports/valve_path_current.json`](reports/valve_path_current.json)). The loads are the worst case for simultaneous valve operation: 0.54 A at each valve connector (a 0.5 A coil at the 13.0 V upper bound, stated assumption), 0.055 A at the buck input, 0.03 A at the flow-sensor supply and 3 x 0.005 A of indicator resistors, 1.18 A in all. The result: the 4 segments of 0.2 mm (5.8 mm in all) carry at most 0.01 A; the tapered necks of 0.32 to 0.72 mm at pad entries (32 segments, 3.0 mm) carry no DC current; the 13 segments of 0.4 mm (12.0 mm) carry up to 1.08 A and rise 7.4 C; the 88 segments of 0.8 mm carry up to 1.135 A and rise 2.7 C. No section rises more than 7.4 C against a 10 C basis, with both valves on and no duty-cycle credit: the valves are held open for irrigation intervals of minutes, so the steady-state current is the design case.

## Valve current path

Each valve output drives a 12 V DC solenoid through a low-side AO3400A (AOS datasheet: 30 V drain-source, 5.7 A). The load assumed is a coil of at most 0.5 A at 12 V, a resistance of at least 24 ohm, and 0.54 A at the 13.0 V upper bound. The path is `+12V_VALVE`, the coil, the output connector, `VALVEn_SW` (0.8 mm) and the AO3400A. The narrowest copper that carries the valve current is the 0.4 mm section, 1.08 A with both valves on and a 7.4 C rise (above). When the AO3400A turns off, the coil current flows through the flyback diode (`D_FLY1` and `D_FLY2`, 1N5819HW-7-F, Diodes Inc.: 40 V, 1 A average, 25 A non-repetitive surge for an 8.3 ms half sine, from the [Diodes datasheet](https://sierraic.com/buy_online/download/datasheet/252694/1N5819HW7F) as listed by [JLCPCB](https://jlcpcb.com/partdetail/1N5819HW-7-F/C82544)). The coil current is 0.54 A at turn-off and decays with the coil's L / R, so the diode carries at most 0.54 A, under its 1 A rating, and the drain of the AO3400A rises to the supply plus the diode drop, about 13.5 V, against 30 V. The coil's inductance is a property of the solenoid and sets only how long the decay takes. The flyback current is at most the 0.54 A the coil carries, always less than the 1.08 A of the analysis above, and it flows only between each diode and its connector; the energy of the coil, 1/2 L I^2, is dissipated in the diode and the coil's own resistance, and for the 0.54 A peak the diode's forward drop gives 0.45 V x 0.54 A = 0.24 W at the start of the decay, against 1 A average and 25 A surge ratings. The snubber on each coil connector (`R_SN1` and `C_SN1`, `R_SN2` and `C_SN2`: 100 ohm in series with 10 nF) damps the turn-off ring.

## Return paths

The routing report ([`export/irrigation_controller.routing.json`](export/irrigation_controller.routing.json)) lists 6 non-blocking return-via findings: a track changes layer at a via with no ground-return via within 1.53 mm and no spot in the GND pour for one. The rule treats a net as fast unless its net class declares `edge_rate slow`; `PWR12`, `PWR33`, `SIG` and `Default` declare it. Ground-return vias at layer changes are tracked in [absmach/synth#280](https://github.com/absmach/synth/issues/280). Each finding is checked against the nearest ground via on the exported board. Both layers carry a GND pour, so the return current stays on a continuous plane; the finding is that its path to a stitching via is longer than the rule's window.

| Net | Via position (mm) | Nearest GND via (mm) |
|---|---|---|
| `I2C_SDA` | (53.5, 38.1) | 2.06 |
| `I2C_SDA` | (51.9, 42.9) | 3.01 |
| `+3V3` | (56.5, 47.5) | 2.51 |
| `SWCLK` | (37.6, 29.6) | 3.14 |
| `+12V_VALVE` | (24.6, 14.1) | 3.94 |
| `+12V_VALVE` | (27.1, 16.3) | 6.81 |

The added loop is taken as 2 x the distance at about 1 nH per mm, and V = L x di/dt.

- `I2C_SDA` carries 3.3 V / 4.7 kohm = 0.7 mA with edges of 300 ns at most: 4.1 nH gives 9.6 uV and 6.0 nH gives 14 uV.
- `+3V3` is the rail of the MCU, the sensor, the EEPROM and two LEDs, held by `C_VDD1`, `C_U4` and `C_U5` at the load pins. For an assumed load step of 50 mA in 1 us, the 5.0 nH loop gives 0.25 mV.
- `SWCLK` switches only while a probe is attached. For an assumed 10 pF load and a 3.3 V edge in 5 ns, the current is C x dV/dt = 6.6 mA and the 6.3 nH loop gives 8.3 mV, under the 3.3 V logic's noise margin.
- `+12V_VALVE` carries the valve current, at most 0.5 A, switched by the low-side drivers. A 0.5 A step in 1 us across 7.9 nH gives 3.9 mV and across 13.6 nH gives 6.8 mV, held by the local bypass capacitors `C_B3` and `C_BULK` on the rail.

## Human review

Every part of the board is recorded as reviewed by JeffMboya, with the review date and the datasheet or product page, in the part files of [`reproduce/user-registry/`](reproduce/user-registry/) (`reviewed_by`, `reviewed_at`, `datasheet_url`).

### PCB review sign-off

PCB review sign-off required by #39: JeffMboya, 2026-11-04, on the package in `fab/` (Gerber content hash `1c84c56d98`). The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review; the independent PCB expert sign-off required by #39 is open. It covers, each verified and approved by JeffMboya:

| Item | Signed off |
|---|---|
| PCB and Gerber package | the Gerbers, drill, BOM and CPL of `fab/` and the exported KiCad board, hash `1c84c56d98`, and the STEP model of `fab/` (geometry hash `69edb9b10c`, complete-model check in `reports/step_verify.txt`) |
| Return paths | the 6 return-via findings of the routing report, `I2C_SDA` (2.06 and 3.01 mm to the nearest ground via), `+3V3` (2.51 mm), `SWCLK` (3.14 mm) and `+12V_VALVE` (3.94 and 6.81 mm), as checked in "Return paths" above |
| ERC | the 6 warnings: `Si2301CDS` absent from KiCad 10.0.6's `Transistor_FET` library (`Q1`, `Q_E1`, `Q_E2`), the `AO3400A` symbol embedded with its parent's graphics (`Q_V1`, `Q_V2`), and the label `BUCK_FB` placed on a wire junction ([absmach/synth#426](https://github.com/absmach/synth/issues/426)) |
| DRC | the 121 `lib_footprint_mismatch` warnings, each a footprint whose silkscreen differs from the library copy while its pads, courtyard and fabrication outline match it |
| Input protection | the explicit acceptance of the 0.4 V standoff margin and the 1.5 V clamp margin for the stated source and surge, and the input requirement (12 V +/-5 %, qualified to 12.6 V, surge at most 18.6 A) and the margins of the SMAJ13A, `Q1` and the MP2307 in "Input protection margins" |
| Placement | the placement of the 126 components on 75.1 x 65.4 mm, 30.9 % courtyard fill, with connectors flush on the edges, a mounting hole in each corner, and the distances listed under "Layout" |
| Parts and values | the LED, bleeder and valve-current values and the BOM identities of the table above, including the crystal `Y1` (JYJE 3TJ480000FYLBC, `C2924620`, with the C0, ESR and start-up margin of "Crystal"), verified and approved by JeffMboya on 2026-11-06 |

## Bill of materials and fabrication package

[`fab/`](fab/) holds the files a quote needs:

- `irrigation_controller_gerbers.zip`: the nine Gerber layers, the Excellon drill file, a drill map and the job file, from KiCad 10.0.6;
- `irrigation_controller_cpl.csv`: 106 placements in JLCPCB's column order;
- `irrigation_controller_bom_orderable.csv`: the exported BOM joined with [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv). Each of the 106 fitted lines carries refdes, value, KiCad footprint, package, manufacturer, manufacturer part number, rating, datasheet, distributor, order number and the product page; the other 20 lines are mounting holes and test points, which are board features with no part;
- `irrigation_controller.step`: the board's STEP model, below;
- `SHA256SUMS` and `GERBER_CONTENT_SHA256`: the Gerber headers carry the creation time, so the content is hashed with those lines masked; the three exports give the same hash, `1c84c56d98`. The mask expansion is the exported 0 mm (opening equal to the pad); [`fab_package.sh`](agent-run/tools/fab_package.sh) takes `MASK_EXPANSION=<mm>` to build a variant from a scratch copy of the board.

The STEP model covers every part and mechanical feature of the board. [`step_export.sh`](agent-run/tools/step_export.sh) exports it with KiCad 10.0.6's `kicad-cli pcb export step` from a scratch copy of the exported board that names a model for each footprint: KiCad's stock 3D library has none for `TestPoint_Pad_D1.5mm` (16 test points), `Fuse_1812_4532Metric` (`F1`) or the APDS-9301 (`U4`), so [`step_models.py`](agent-run/tools/step_models.py) writes models of them into [`agent-run/step-models/`](agent-run/step-models/): the test point as the footprint's 1.5 mm pad in 1 oz copper (a disc 0.035 mm thick); the fuse from the Littelfuse 1812L datasheet's dimensions table, row 1812L150/16, at the middle of its ranges (4.55 mm long, 3.24 mm wide, 1.00 mm thick, with 0.75 mm end terminations, as three solids; the datasheet gives A 4.37 to 4.73 mm, B 3.07 to 3.41 mm, C 0.75 to 1.25 mm and D 0.3 to 1.2 mm); and the APDS-9301 as a 2.60 x 2.20 x 0.55 mm body, its miniature ChipLED package. The MP2307 (`U1`) takes the library's `SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.81mm` model, whose body and leads are kept and whose exposed pad, the 0.05 mm slab under the body, is cut from 3.81 mm to the footprint's 3.3 mm. The four mounting holes have no body: each is a 2.5 mm hole in the board solid. The fuse and the APDS-9301 are modelled at their datasheets' dimensions, not from the manufacturers' CAD; the APDS-9301's dimensions are those of a copy of its datasheet, not the package drawing.

[`step_verify.py`](agent-run/tools/step_verify.py) reopens the STEP in OpenCASCADE (OCP, `pip install cadquery-ocp`) and compares it with the committed PCB ([`reports/step_verify.txt`](reports/step_verify.txt)): the 122 components of the STEP are the 122 footprints with a body, each under its reference and none repeated; every component origin is the footprint's position (worst difference 0.0000 mm) and every rotation the footprint's (0.000 deg); the board solid is 75.10 x 65.40 mm, the Edge.Cuts extent; its holes are the PCB's drills (13 of 0.75 mm, 2 of 0.8 mm, 11 of 1.0 mm and the 4 mounting holes of 2.5 mm; the 152 via drills are not part of KiCad's STEP board); and no body lies more than 0.25 mm outside its courtyard, connectors included. The board solid is 1.51 mm thick; the PCB's general thickness is 1.6 mm. [`step_check.py`](agent-run/tools/step_check.py) ([`reports/step_check.txt`](reports/step_check.txt)) checks the ISO 10303-21 framing and that all 76,051 entities resolve. KiCad's STEP writer numbers its entities differently on every run, so the file hash in `SHA256SUMS` is not reproducible; the geometry hash, the sorted point coordinates rounded to 0.001 mm, is `69edb9b10c` for all three exports.

Regenerate them with [`fab_package.sh`](agent-run/tools/fab_package.sh). Every part
identity was looked up by manufacturer part number and the page is in the table.
The design names generic parts for passives and LEDs, so the table picks one
orderable part per value and footprint. Where the exact part the design names
has no LCSC listing, the table gives its page on another distributor, as the
part itself: the MP2307DN, the AO3401A, the fuse and the
reset switch (DigiKey or Mouser), the EEPROM and the 100 uF capacitor (RS), and
the APDS-9301 and the 1x5 header (Farnell). The amber LED is the only part
chosen as a stand-in: a KENTO KT-0603Y, listed as yellow at 584 to 594 nm, which is
the amber range. The green LED is the registry's `ltst_c190kgkt`, Lite-On
LTST-C190KGKT (LCSC C125094). The APDS-9301 and the through-hole parts are not stocked for
JLCPCB assembly and are fitted by hand.

The rating, package and datasheet columns come from the manufacturer datasheets
(Yageo, NIC, NDK, Broadcom, AOS) or from the product page named in the `link`
column; the figures are listed in
[`extend_sourcing.py`](agent-run/tools/extend_sourcing.py).

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The server, the request, the response and the hash of every file written are recorded in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/). Each cell is the first ten characters of a SHA-256. All three exports are `routed`, with no blocking reason, 0 DRC errors and 0 unconnected pads.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `irrigation_controller.kicad_sch` (root sheet) | 7bd5b95889 | 7bd5b95889 | 7bd5b95889 | byte-identical |
| `irrigation_controller.kicad_pro` | a33f249cce | a33f249cce | a33f249cce | byte-identical |
| `irrigation_controller.kicad_sym` | 97b086aaed | 97b086aaed | 97b086aaed | byte-identical |
| `bom.csv` | 3e1f0ea17f | 3e1f0ea17f | 3e1f0ea17f | byte-identical |
| `pnp.csv` | ce04cd53cc | ce04cd53cc | ce04cd53cc | byte-identical |
| `irrigation_controller.synth.kicad_pcb` (un-routed) | ee150eecb6 | ee150eecb6 | ee150eecb6 | byte-identical |
| `irrigation_controller.kicad_pcb` | fddf1c4f41 | ab7dc8e6f3 | dcad63a7d0 | not byte-identical; identical after the router's and KiCad's random ids are masked (normalized) |
| Gerber and drill content | 1c84c56d98 | 1c84c56d98 | 1c84c56d98 | identical after the creation time in the headers is masked (normalized) |

Byte-identical across the three exports: `kicad_sch`, `kicad_pro`, `kicad_sym`, `bom.csv`, `pnp.csv` and the un-routed `synth.kicad_pcb`. Not byte-identical but equal after normalization: the routed PCB, which differs between runs only in the random ids the router gives its
tracks and vias and KiCad gives its zone fills (with those masked, all three are geometrically equivalent and
`bd380bffbb`; [`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking), and the Gerber, drill and job files, which differ in the creation time of their headers. The Gerber hash masks the creation time KiCad
writes into every Gerber, the drill file and the job file, and leaves out the drill-map PDF.

What is committed, from export 1 on the pinned binary: [`export/`](export/) (the routed board, the project, the root schematic sheet, the
BOM and placement files, Synth's routing and DRC JSON). The 13 group-sheet schematics and `irrigation_controller.kicad_sym` (`267dcbd1d9`) come from an export of the same source and
user registry on the tree of `upstream/main` `fbfd8d60`: the pinned binary writes group sheets and a symbol library without the power symbols (27 more ERC `lib_symbol_issues`), which that tree includes; the root sheet is byte-identical between the two (`7bd5b95889`), [`reports/kicad_drc.json`](reports/kicad_drc.json) and
[`reports/kicad_erc.json`](reports/kicad_erc.json) (KiCad 10.0.6 with the library tables), [`fab/`](fab/), and
[`reproduce/`](reproduce/): [`reproduce.sh`](reproduce/reproduce.sh) is supplemental tooling, not the evidence of how the board was made (see [`PROVENANCE.md`](PROVENANCE.md)); it builds the pinned Synth, KiCadRoutingTools and Python
packages ([`pins.env`](reproduce/pins.env)), exports the source three times, compares the hashes and rebuilds the fabrication
package. The parts the design takes from outside the Synth checkout's registry are in
[`reproduce/user-registry/`](reproduce/user-registry/) (33 files); the exports ran with that directory as the user registry (`reproduce.sh` sets `SYNTH_USER_REGISTRY_DIR` to it), so it and the pinned Synth commit are what reproduces them.
The Synth commit is `a12aa289b7b001e8d5a197b15d20302e4718da5c` of `absmach/synth` (`upstream/main`: the merge of Synth pull request #269, which carries the router fix that counts the routing report's statistics after the repair passes, on top of `0ceb7b80`, the merge of #388 with #370 and #394 before it). The three exports above were made with a binary built from the router-fix commit `7d418281`, whose Synth crates are identical to those of that commit, and these inputs.
