# Qualification: Irrigation controller (Class A, case 1)

**Status:** the board is routed and released (46 of 46 nets; KiCad 10.0.6 reports 0 errors and 0 unconnected pads with the library tables), three fresh exports agree (the files listed below byte-identical, the routed board and the Gerbers equal after normalization), and the exported KiCad files, a fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)). The numbers, hashes and files below all come from one export, made with the Synth binary named in [`PROVENANCE.md`](PROVENANCE.md).

## What was measured

All figures come from the files of the first of the three exports below (KiCad 10.0.6).

| | |
|---|---|
| Board | 79.2 x 70.7 mm, 5599 mm2, two layers, 127 placed items (107 parts, and 20 mounting holes and test points), 46 nets; courtyard fill 27.8 % |
| Routing | state `routed`, 46 of 46 nets connected, 606 segments and 167 vias in the file (165 of 0.6 mm over a 0.2 mm drill, ring 0.20 mm, and 2 of 0.46 mm over 0.2 mm, ring 0.13 mm), 1414.6 mm of track; the routing report and the delivered board agree ([`delivered_board_counts`](#routing-report-and-power-copper)) |
| KiCad DRC, 10.0.6, `--refill-zones --severity-all`, with the footprint library table configured | 0 errors, 0 unconnected, 121 `lib_footprint_mismatch` warnings (below); 0 `silk_overlap`, 0 `track_dangling`. Result: [`kicad/drc_with_library_tables.json`](kicad/drc_with_library_tables.json) |
| Router state | state `routed`, `fabrication_ready` true, no blocking reason, no via in a pad, 5 non-blocking return-via findings ([`kicad/irrigation_controller.routing.json`](kicad/irrigation_controller.routing.json)) |
| Trace width, as routed | `VIN_FUSED` 0.2 to 0.8 mm; `VALVE1_SW` and `VALVE2_SW` 0.8 mm; `+12V_VALVE` 0.2 to 0.8 mm; `+3V3` 0.2 to 0.4 mm; the other signals 0.2 mm |
| Fine-pitch escape | `U1` (SOIC-8, exposed pad) and `U2` (LQFP-32) declare `escape { off }` in the source: their pads are routed from the surface, and no via sits in a pad |
| BOM | 107 fitted lines, each with a manufacturer, MPN, distributor, order number and page, and 20 board features (mounting holes and test points) that are not parts ([`fab/irrigation_controller_bom_orderable.csv`](fab/irrigation_controller_bom_orderable.csv)) |
| Schematic | a root sheet and 13 group sheets, one per named group, no `OTHER` region. KiCad's ERC ([`kicad/erc_with_library_tables.json`](kicad/erc_with_library_tables.json)), symbol and footprint library tables configured: 0 errors and 5 warnings, explained below. The `+12V_VALVE` rail carries a `PWR_FLAG` |
| STEP | 123 components, positions and rotations equal to the PCB's, outline equal to Edge.Cuts; geometry hash `e1a6d41ea9` ([`models/irrigation_controller.step`](models/irrigation_controller.step)) |

The 121 `lib_footprint_mismatch` warnings are footprints whose silkscreen the exporter cut back from a via or a pad (0.25 mm clearance, above JLCPCB's 0.15 mm minimum; [absmach/synth#425](https://github.com/absmach/synth/issues/425)), so they differ from the library copy; the pads, courtyard and fabrication outline of every one are identical to the library file (the footprint comparison lists `F.SilkS` as the only layer that differs). The five ERC library warnings: `Q_E1` and `Q_E2` are P-channel MOSFETs whose registry part names the symbol `Si2301CDS`, which KiCad 10.0.6's `Transistor_FET` library does not contain, so the exporter draws its own symbol ([absmach/synth#96](https://github.com/absmach/synth/issues/96)); `Q_V1` and `Q_V2` carry the `AO3400A` symbol, which the library defines as an extension of `Q_NMOS_GSD`, and the schematic embeds it with the parent's graphics included ([absmach/synth#428](https://github.com/absmach/synth/issues/428)); and the label `BUCK_FB` sits on a wire junction ([absmach/synth#426](https://github.com/absmach/synth/issues/426)).

## Layout

Measured on the exported file:

| | |
|---|---|
| Courtyard fill | 27.8 % of the board; 0 of 49 10 x 10 mm cells are empty |
| Connectors | the courtyards of `J1`, `J_VALVE1`, `J_VALVE2`, `J_SOIL1`, `J_SOIL2`, `J_FLOW` and `J_I2C` are flush with the board edge (0.00 mm); the pin header `J_SWD` sits 2.68 mm inboard |
| Mounting holes | one in each corner, distance from the left or right edge and from the top or bottom edge: `H1` 7.6 and 7.95 mm, `H2` 10.6 and 4.95, `H3` 7.1 and 7.25, `H4` 7.6 and 7.25 |
| Per-pin decoupling, pad to pin | `C_IN1` 3.81 mm and `C_IN2` 3.45 mm from `U1.2`, `C_U4` 2.81 mm from `U4.1`, `C_U5` 3.19 mm from `U5.8`, `C_VDD1` 3.6 mm from `U2.4`, `C_VDD2` 7.78 mm from `U2.4` (the 4.7 uF bulk capacitor of ST's reference supply scheme); at the ideal-diode controller `C_VCAP` 6.08 mm from `U_IDEAL.1` (VCAP) and 6.05 mm from `U_IDEAL.6` (ANODE), `C_VIN` 7.99 mm from `U_IDEAL.6` |
| Regulator | `C_OUT2` 11.6 mm and `C_OUT1` 11.0 mm from `U1`, origin to origin |
| Crystal, reset button | 9.0 mm and 11.6 mm from the MCU |
| SWD header | 42.9 mm from the MCU, at the right edge |
| Through-hole to SMD pad | the nearest pair is `J_SOIL1` to `R_N3`, 2.17 mm pad edge to pad edge, then `C_BULK` to `R_SN1` 2.37 mm (JLCPCB's DFM treats 2.1 mm as a Danger) |
| Isolated parts | [('J_I2C', 6.0), ('R_N6', 4.4), ('F1', 4.1)], more than 4 mm from a neighbouring body |

The SWD header is on the board edge so a probe cable reaches it without crossing the board, and the MCU sits mid-board. Its pins are, in order from pin 1: `+3V3`, `SWDIO`, `GND`, `SWCLK`, `NRST`. The reset button is 11.6 mm from the MCU: reset is a slow input behind a pull-up and a capacitor, so the track length does not affect its timing. `C_VDD2`, the 4.7 uF capacitor of ST's reference supply scheme, is 7.78 mm from `U2.4`; the 100 nF `C_VDD1` that the datasheet asks to be close is 3.6 mm from it. The capacitors that sit far from an IC (mean 12.5 mm over 39) are rail bulk and filter parts, not per-pin decoupling; `C_BULK` is the 100 uF output capacitor of the ideal-diode stage and sits 26.33 mm from `U_IDEAL.4`.

## Input protection

**Input requirement.** The supply is 12 V +/-5 %, 10.8 to 12.6 V in operation, and the qualified maximum is 12.6 V; the TVS does not conduct at its 13.0 V standoff, which the checks below use as an upper bound. The surge requirement is at most 14 A peak (10/1000 us), 75 % of the SMAJ13A's rated 18.6 A: the part is rated 400 W and 18.6 A at its 21.5 V clamp is 399.9 W, so the rating alone leaves no margin, while 14 A at 21.5 V is 301 W. The source impedance sets it: the open-circuit surge the clamp covers is 21.5 V + 14 A x R_source, 35.5 V at 1 ohm and 49.5 V at 2 ohm.

**Protection.** `F1` is a Littelfuse 1812L150/16DR resettable fuse; its hold current derates to 1.25 A at 40 C and 1.10 A at 50 C and 0.60 A at 85 C, against a load budget of 1.18 A at 12 V and 1.25 A at 13.0 V, so the qualified ambient maximum is 40 C. `D_TVS` is an SMAJ13A (Littelfuse SMAJ series datasheet: standoff 13.0 V; breakdown 14.40 to 15.90 V at 1 mA; 1 uA maximum leakage at the standoff; 21.5 V clamp at 18.6 A) on `VIN_FUSED`, ahead of the ideal-diode stage, so a transient is clamped before it reaches `Q1` or the controller. The operating maximum is 0.4 V (3.2 %) below the standoff; the clamp is below the MP2307's 23 V recommended input and its 26 V absolute maximum (MPS datasheet) and below `Q1`'s 60 V drain-source rating and the controller's 65 V absolute maximum. A reversed supply forward-biases `D_TVS` and `F1` opens.

**Reverse-polarity stage.** `U_IDEAL` is a Texas Instruments LM74700-Q1 (datasheet SNOSD17G; SOT-23-6, pins 1 VCAP, 2 GND, 3 EN, 4 CATHODE, 5 GATE, 6 ANODE) driving the N-channel MOSFET `Q1` (Diodes DMT6007LFG, 60 V, VGS +/-20 V, RDS(on) 6 mOhm maximum at 10 V and 8.5 mOhm maximum at 4.5 V, Ciss 2090 pF typical). ANODE and EN sit on `VIN_FUSED` with the three source pads of `Q1`, CATHODE on `+12V_VALVE` with its drain, GATE on its gate. The datasheet's requirements are met as follows: VCAP to ANODE at least 0.1 uF and 10 x the MOSFET's Ciss (20.9 nF): `C_VCAP` is 100 nF; at least 22 nF at ANODE: `C_VIN` is 100 nF; at least 100 nF at CATHODE: `C_BULK` is 100 uF. At the load budget of 1.18 A the conduction drop of `Q1` is 1.18 A x 8.5 mOhm = 10 mV and its dissipation 12 mW. A reversed supply leaves the body diode of `Q1` blocking and the controller off.

### Input protection margins

Figures from the Littelfuse SMAJ series datasheet (`D_TVS`), the MPS MP2307 datasheet (`U1`), the Texas Instruments LM74700-Q1 datasheet (`U_IDEAL`) and the Diodes DMT6007LFG datasheet (`Q1`). The supply is a nominal 12 V source, 12 V +/-5 % (10.8 to 12.6 V) in operation and qualified to 12.6 V (the checks use the 13.0 V standoff as an upper bound); the surge requirement is at most 14 A peak, set by the source impedance.

| Stress point | Worst case | Limit | Margin |
|---|---|---|---|
| `D_TVS` standing voltage | 12.6 V qualified maximum, 13.0 V checked as an upper bound | standoff 13.0 V; leakage 1 uA maximum at the standoff; breakdown 14.40 V minimum at 1 mA | no conduction at 12.6 V (0.4 V, 3.2 % below the standoff) or at 13.0 V |
| `U1` input in operation | 12.6 V (13.0 V checked) | 4.75 to 23 V recommended | 10.4 V below the recommended maximum at 12.6 V (10.0 V at 13.0 V) |
| `U1` input in a surge | 21.5 V, the SMAJ13A clamp at its rated 18.6 A peak pulse | 23 V recommended, 26 V absolute maximum | 1.5 V (6.5 %) below the recommended maximum, 4.5 V (17 %) below the absolute maximum |
| `U_IDEAL` ANODE in a surge | 21.5 V | 65 V absolute maximum ANODE to GND (4 to 60 V operating) | 43.5 V (67 %) |
| `Q1` drain-source in a surge | `D_TVS` is ahead of `Q1`, so `Q1` sees at most 21.5 V | 60 V | 38.5 V (64 %) |
| `Q1` gate-source | the controller's gate drive, 15 V at most (SNOSD17G) | +/-20 V | 5 V (25 %) |
| Surge current | the TVS current is (V_open-circuit - 21.5 V) / R_source: at most 14 A for a source of 1 ohm up to 35.5 V open-circuit or 2 ohm up to 49.5 V | 18.6 A, 10/1000 us (400 W); 14 A is 75 % of it | at the required 14 A the clamp holds the rail at 21.5 V |

JeffMboya accepts these margins for the stated source: 12 V +/-5 % (12.6 V maximum), a surge of at most 14 A (10/1000 us), TVS leakage of at most 1 uA at the standoff, and the MP2307's 23 V recommended and 26 V absolute maximum input; the standoff margin is 0.4 V and the clamp margin 1.5 V.

## Routing report and power copper

The routing report ([`kicad/irrigation_controller.routing.json`](kicad/irrigation_controller.routing.json)) records `engine_policy_verdict: rejected` next to `fabrication_ready: true`. The two statements are about different checks. The routing engine runs with its strict-size policy, which rejects any track narrower than the width its net class declares. Synth records that verdict and then checks the delivered board itself: connectivity (46 of 46 nets), the process floor of 0.127 mm for track width and clearance, KiCad DRC with the library tables (0 errors, 0 unconnected) and the return-via rule. The board passes all of them, so the delivered board is valid; the narrowed sections are the ones listed here, each with the current it carries. The engine reports 86 vias as its own routing vias; the file holds 167 because Synth's exporter and repair passes add the ground vias. The routing report is written after Synth's repair passes, so it describes the delivered board: 606 segments, 167 vias and 1414.6 mm of track, with 46 of 46 nets connected, equal to the count of the delivered board itself (606 segments, 167 vias, 1414.6 mm; the 0.6 mm vias over a 0.2 mm drill number 165 and the 0.46 mm vias 2), with 0 DRC errors and 0 unconnected pads.

| Net | Declared | Narrower segments | Narrowest | Current the narrowest carries (IPC-2221, 1 oz external) |
|---|---|---|---|---|
| `+12V_VALVE` | 0.8 mm | 31, from 0.2 to 0.72 mm, 20.9 mm in all | 0.2 mm | the narrowed sections carry at most 0.045 A; a 0.2 mm track carries 0.74 A at a 10 C rise |
| `+3V3` | 0.4 mm | 7, 1.7 mm in all, the longest 0.85 mm | 0.2 mm | the MCU, the light sensor, the EEPROM and two LEDs draw well under 0.1 A; a 0.2 mm track carries 0.74 A at a 10 C rise |
| `VIN_FUSED` | 0.8 mm | 31, from 0.2 to 0.72 mm, 13.7 mm in all | 0.2 mm | the narrowed sections carry at most 0.000 A and rise 0.0 C with the 1.18 A load shared equally by the three source pads of `Q1`; the whole net rises at most 2.9 C |
| `BUCK_SW` | 0.8 mm | 2 of 0.4 mm, 2.5 mm in all | 0.4 mm | the inductor current: at most 0.46 A (an assumed 0.1 A load plus half of the 0.72 A peak-to-peak ripple of 10 uH at 340 kHz from 12.6 V to 3.3 V); a 0.4 mm track carries 1.23 A at a 10 C rise |

**Qualification limitation, recorded.** The delivered board does not meet the declared routing policy for `+12V_VALVE`, `+3V3`, `VIN_FUSED` and `BUCK_SW`: 31 segments of `+12V_VALVE` (20.9 mm), 31 of `VIN_FUSED` (13.7 mm) and 2 of `BUCK_SW` are narrower than the declared 0.8 mm, and the routing engine's strict-size verdict is `rejected`. The board is therefore not an ordinary pass of the declared policy. The deviation is qualified by current, section by section.

Every section of `+12V_VALVE` is qualified at its own current, not at the net's total. `valve_path.py`, a script not kept in the repository, solves the DC copper network of the delivered board (1 oz, 35 um copper, 1.72e-8 ohm m; every segment a resistor of its width and length, copper that overlaps joined, vias and pads shorts, a via inside a pad joining that pad) from the source, `Q1` pad 5 (the drain), to the loads, and gives the current and the IPC-2221 temperature rise of each of the 127 segments. The loads are the worst case for simultaneous valve operation: 0.54 A at each valve connector (a 0.5 A coil at the 13.0 V upper bound, stated assumption), 0.055 A at the buck input, 0.03 A at the flow-sensor supply and 3 x 0.005 A of indicator resistors, 1.18 A in all. The result: the 31 segments narrower than 0.8 mm carry at most 0.045 A and rise 0.0 C; the segments of 0.8 mm carry up to 1.135 A and rise 2.7 C. No section rises more than 2.7 C against a 10 C basis, with both valves on and no duty-cycle credit: the valves are held open for irrigation intervals of minutes, so the steady-state current is the design case. The same solve on `VIN_FUSED`, from `F1` pad 2 to the three source pads of `Q1` with the 1.18 A shared equally (0.393 A each, a stated assumption), gives 61 segments, a rise of at most 2.9 C on the whole net, and no more than 0.000 A and 0.0 C in the 38 segments narrower than 0.8 mm, which are pad-entry necks.

## Valve current path

Each valve output drives a 12 V DC solenoid through a low-side AO3400A (AOS datasheet: 30 V drain-source, 5.7 A). The load assumed is a coil of at most 0.5 A at 12 V, a resistance of at least 24 ohm, and 0.54 A at the 13.0 V upper bound. The path is `+12V_VALVE`, the coil, the output connector, `VALVEn_SW` (0.8 mm) and the AO3400A. When the AO3400A turns off, the coil current flows through the flyback diode (`D_FLY1` and `D_FLY2`, 1N5819HW-7-F, Diodes Inc.: 40 V, 1 A average, 25 A non-repetitive surge for an 8.3 ms half sine, from the [Diodes datasheet](https://sierraic.com/buy_online/download/datasheet/252694/1N5819HW7F) as listed by [JLCPCB](https://jlcpcb.com/partdetail/1N5819HW-7-F/C82544)). The coil current is 0.54 A at turn-off and decays with the coil's L / R, so the diode carries at most 0.54 A, under its 1 A rating, and the drain of the AO3400A rises to the supply plus the diode drop, about 13.5 V, against 30 V. The coil's inductance is a property of the solenoid and sets only how long the decay takes. The flyback current flows only between each diode and its connector; the energy of the coil, 1/2 L I^2, is dissipated in the diode and the coil's own resistance, and for the 0.54 A peak the diode's forward drop gives 0.45 V x 0.54 A = 0.24 W at the start of the decay, against 1 A average and 25 A surge ratings. The snubber on each coil connector (`R_SN1` and `C_SN1`, `R_SN2` and `C_SN2`: 100 ohm in series with 10 nF) damps the turn-off ring.

## Return paths

The routing report lists 5 non-blocking return-via findings: a track changes layer at a via with no ground-return via within 1.53 mm and no spot in the GND pour for one. The rule treats a net as fast unless its net class declares `edge_rate slow`; `PWR12`, `PWR12_IN`, `PWR33`, `SIG` and `Default` declare it. Ground-return vias at layer changes are tracked in [absmach/synth#280](https://github.com/absmach/synth/issues/280). Each finding is checked against the nearest ground via on the exported board. Both layers carry a GND pour, so the return current stays on a continuous plane; the finding is that its path to a stitching via is longer than the rule's window.

| Net | Via position (mm) | Nearest GND via (mm) |
|---|---|---|
| `NRST` | (38.0, 37.1) | 4.84 |
| `I2C_SCL` | (38.2, 28.4) | 4.78 |
| `+3V3` | (47.7, 34.0) | 3.64 |
| `VIN_FUSED` | (18.2, 20.0) | 6.07 |
| `VIN_FUSED` | (18.6, 26.2) | 3.45 |

The added loop is taken as 2 x the distance at about 1 nH per mm, and V = L x di/dt.

- `NRST` the reset input is a slow node behind a 10 k pull-up and 100 nF; it carries no fast edge.
- `I2C_SCL` carries 3.3 V / 4.7 kohm = 0.7 mA with edges of at most 300 ns: 9.6 nH gives 22 uV.
- `+3V3` is the rail of the MCU, the sensor, the EEPROM and two LEDs, held by `C_VDD1`, `C_U4` and `C_U5` at the load pins; for an assumed load step of 50 mA in 1 us, 7.3 nH gives 0.36 mV.
- `VIN_FUSED` carries the protected 12 V input; for an assumed step of 0.54 A (one valve switching) in 1 us across 12.1 nH, 6.6 mV on a 12 V rail, held by `C_IN1`, `C_IN2`, `C_VIN` and `C_BULK`.
- `VIN_FUSED` carries the protected 12 V input; for an assumed step of 0.54 A (one valve switching) in 1 us across 6.9 nH, 3.7 mV on a 12 V rail, held by `C_IN1`, `C_IN2`, `C_VIN` and `C_BULK`.

## Review points

| Review point | State |
|---|---|
| DFM findings | In the file: 165 of the 167 vias are 0.6 mm over a 0.2 mm drill (ring 0.20 mm) and 2 are 0.46 mm over 0.2 mm (ring 0.13 mm); the nearest through-hole to SMD pad pair is 2.17 mm; silkscreen lines are drawn at 0.18 mm and kept 0.25 mm from copper. The package in `fab/` is generated from this export (Gerber content hash `a26463bd09`). |
| Input protection rating | the input requirement, the protection and the margins of "Input protection" above |
| Decouplers at their pins | the distances of the "Layout" table; the input capacitors are 0805 parts; `C_VDD2`, the 4.7 uF capacitor of ST's reference scheme, is 7.78 mm from `U2.4` and the 100 nF `C_VDD1` is 3.6 mm from it |
| LED, bleeder and crystal values | Datasheets opened (2026-10-30). **LED:** Lite-On LTST-C190KGKT, Spec DS22-2000-074 Rev N, via LCSC's copy: AlInGaP green, VF 1.9 to 2.4 V at 20 mA (no typical; bins 4 to 8 in 0.1 V steps, each +/-0.1 V), 30 mA DC, 80 mA peak, 75 mW, 5 V reverse, peak 574 nm, 18 to 71 mcd at 20 mA. `D_PWR`, `D_N2`: (3.3 - 2.4) / 680 = 1.3 mA to (3.3 - 1.9) / 680 = 2.1 mA, from the 20 mA limits (the datasheet gives VF only at 20 mA); 14 times under the 30 mA maximum. The datasheet specifies brightness only at 20 mA; Fig. 4 puts 1 to 2 mA at roughly a tenth of it (graph read, approximate), about 1 to 11 mcd. **Crystal:** JYJE 3TJ480000FYLBC (LCSC C2924620), the manufacturer's specification sheet dated 2021-11-18, [`fab/sourcing/jyje_3tj480000fylbc_specification_2021-11-18.pdf`](fab/sourcing/jyje_3tj480000fylbc_specification_2021-11-18.pdf) (`8588ccc71462`): 8.000 MHz fundamental, CL 8 pF, +/-10 ppm at 25 C, +/-20 ppm over -40 to +85 C, ESR 150 ohm maximum, **shunt capacitance C0 5 pF maximum**, drive 100 uW, 3.2 x 2.5 mm, pads 1 and 3 the crystal and 2 and 4 ground (the sheet's connection drawing), suggested land 1.4 x 1.2 mm at a 2.2 x 1.7 mm pitch, the pad centres of the library footprint. The LCSC page lists +/-30 ppm stability where the sheet says +/-20 ppm, and 56 pieces in stock on 2026-11-05. The sheet gives no stray capacitance. **Stray capacitance:** the STM32G030 datasheet (DS12991 Rev 4, Table 36 note, p. 52) says PCB and MCU pin capacitance must be included and that 10 pF can be used as a rough estimate of the combined pin and board capacitance; its I/O pin capacitance is 5 pF typical. Read as 10 pF at each oscillator pin, 6.2 pF load capacitors give CL = (6.2 + 10) / 2 = 8.1 pF, the same 8.1 pF as the source notes' 6.2 / 2 + 5 (5 pF in the series formula is 10 pF per pin). The datasheet does not say which reading it means; ST's application note AN2867, which would, could not be downloaded (st.com refuses it), so the 5 pF is consistent with ST's figure but not confirmed. **Start-up margin:** ST's maximum critical crystal transconductance is 1.5 mA/V (Table 36). With the crystal's own maximum ESR of 150 ohm and maximum C0 of 5 pF, the effective load CL = 8.1 pF and 8 MHz, the standard formula gm_crit = 4 ESR (2 pi f)^2 (C0 + CL)^2 (from AN2867, not opened) is 0.26 mA/V, a margin to ST's 1.5 mA/V of 5.8 times, and gm_crit stays under 1.5 mA/V for any C0 up to 23 pF, 4.6 times the stated maximum. The ESR and C0 are those of this crystal's own specification, not of a sibling part. No oscillator start-up measurement with this board's STM32G030, crystal and load capacitors has been made. `D_VALVE` (KENTO KT-0603Y, Vf 2.4 V on its page) from an MCU pin through 680 ohm: 1.3 mA, the pin's own drop not counted, against the 15 mA the STM32G030 datasheet (DS12991, Table 19) allows any I/O pin to source. `D_N1` from 13.0 V through 2.2 k: (13.0 - 2.4) / 2.2 k = 4.8 mA, 51 mW in a 100 mW resistor. Bleeders are at 13.0 V in the source notes (`R_B1` 100 k: 0.13 mA). |
| 5 V clamps on 3.3 V signals | `FLOW_PULSE` is clamped by `D_FZ`, a 3.3 V zener, behind `R_FS` 2.2 k: (13.0 V - 3.3 V) / 2.2 k = 4.4 mA, 14 mW in a 500 mW part. |
| SHT30 footprint and escape | A breakout attaches to `J_I2C` at 0x44. There is no fine-pitch part on the board. |
| I2C bus speed | `R_SDA` and `R_SCL` are 4.7 k. With the I2C rise-time limits of NXP UM10204 (1000 ns in standard mode, 300 ns in fast mode) and t_r = 0.8473 x R x C, the bus capacitance may be up to 251 pF at 100 kHz and 75 pF at 400 kHz; the 4.7 k value sinks at most (3.3 V - 0.4 V) / 4.7 k = 0.6 mA against the 3 mA the specification allows. The board is specified for standard mode (100 kHz); 400 kHz holds only while the breakout and its cable add no more than 75 pF in all. |
| BOM and fabrication output | every line sourced; Gerbers, drill and pick-and-place generated and hashed (below). |
| DRC with the library table | 0 errors, 0 unconnected, 121 `lib_footprint_mismatch` warnings and no `silk_overlap` or `track_dangling`, above. |
| Return-path advisories | `NRST`, `I2C_SCL`, `+3V3`, `VIN_FUSED`, `VIN_FUSED`, checked in "Return paths" above. |
| Crystal load | the JYJE 3TJ480000FYLBC has an 8 pF load. With 5 pF assumed for stray capacitance, CL = C1 x C2 / (C1 + C2) + 5 pF; C1 = C2 = 2 x (8 - 5) = 6 pF; the nearest E24 value 6.2 pF gives 8.1 pF. `C_X1` and `C_X2` are 6.2 pF. The 8 pF is from the JYJE specification sheet; the 5 pF is the series-formula equivalent of the 10 pF per pin ST's STM32G030 datasheet gives as a rough estimate, not confirmed (see "LED, bleeder and crystal values"). |
| Valve current | a 12 V solenoid at most 0.5 A: the `PWR12` class is 0.8 mm; the AO3400A (5.7 A on its listing) and the 1N5819HW flyback diode (1 A) are above it. Calculated from listing figures, not measured. |
| Schematic references and groups | every part is in a named group; there is no `OTHER` region. |
| MP2307 divider | VFB 0.925 V (0.900 to 0.950 V, MPS datasheet); the divider and compensation are calculated from it. |
| MP2307 compensation | MPS MP2307 datasheet Rev. 1.7, "Compensation Components": `R3 < 2 pi C2 (0.1 fS) VOUT / (GEA GCS VFB)` with GEA 820 uA/V, GCS 5.2 A/V, fS 340 kHz (300 to 380 kHz) and VFB 0.925 V puts the crossover below one tenth of the switching frequency. The output capacitance is 2 x 22 uF, 44 uF nominal and about 31 uF with the DC-bias derating of an X5R 1206 at 3.3 V (an assumption, not a measurement): `R3` below 7.9 k and 5.5 k respectively. `R_COMP` is 5.6 k, giving a crossover of 24 kHz at 44 uF and 34 kHz at 31 uF, 0.07 and 0.10 of fS. `C3 > 4 / (2 pi R3 fC)` is 4.7 nF at the 34 kHz crossover (3.3 nF), so `C_COMP` is 4.7 nF. The second capacitor `C6` is not fitted: the ESR zero of a ceramic output capacitor is far above fS / 2. |

## Sourcing

[`fab/irrigation_controller_bom_orderable.csv`](fab/irrigation_controller_bom_orderable.csv) has 127 lines: 107 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number from [`fab/sourcing/sourcing.csv`](fab/sourcing/sourcing.csv), and 20 board features (4 mounting holes and 16 test points) that are not parts. Every part identity was looked up by manufacturer part number and the page is in the table. The design names generic parts for passives and LEDs, so the table picks one orderable part per value and footprint. Where the exact part the design names has no LCSC listing, the table gives its page on another distributor as the part itself: the MP2307DN, the fuse and the reset switch (DigiKey or Mouser), the EEPROM and the 100 uF capacitor (RS), and the APDS-9301 and the 1x5 header (Farnell). The amber LED is the only part chosen as a stand-in: a KENTO KT-0603Y, listed as yellow at 584 to 594 nm, which is the amber range. The green LED is the registry's `ltst_c190kgkt`, Lite-On LTST-C190KGKT (LCSC C125094). The two parts of the input stage are the Texas Instruments LM74700QDBVRQ1 (LCSC `C2941042`, page read; 100 in stock when looked up, which changes) and the Diodes DMT6007LFG-7 (LCSC `C2802568`, tied to the part by the LCSC datasheet file; its stock is not shown on the page read). The APDS-9301 and the through-hole parts are not stocked for JLCPCB assembly and are fitted by hand.

## Fabrication package

[`fab/`](fab/) holds the files a quote needs:

- `irrigation_controller_gerbers.zip`: the nine Gerber layers, the Excellon drill file, a drill map and the job file, from KiCad 10.0.6;
- `irrigation_controller_cpl.csv`: the placements in JLCPCB's column order;
- `irrigation_controller_bom_orderable.csv`: the exported BOM joined with `fab/sourcing/sourcing.csv`;
- [`models/irrigation_controller.step`](models/irrigation_controller.step): the board's STEP model;
- `GERBER_CONTENT_SHA256`: the Gerber headers carry the creation time, so the content is hashed with those lines masked; the three exports give the same hash, `a26463bd09`. The mask expansion is the exported 0 mm (opening equal to the pad).

The STEP model covers every part and mechanical feature of the board. KiCad 10.0.6's `kicad-cli pcb export step` exports it from a scratch copy of the exported board that names a model for each footprint: KiCad's stock 3D library has none for `TestPoint_Pad_D1.5mm` (16 test points), `Fuse_1812_4532Metric` (`F1`) or the APDS-9301 (`U4`), so models of them are in [`models/`](models/): the test point as the footprint's 1.5 mm pad in 1 oz copper (a disc 0.035 mm thick); the fuse from the Littelfuse 1812L datasheet's dimensions table, row 1812L150/16, at the middle of its ranges (4.55 mm long, 3.24 mm wide, 1.00 mm thick, with 0.75 mm end terminations, as three solids); and the APDS-9301 as a 2.60 x 2.20 x 0.55 mm body, its miniature ChipLED package. The MP2307 (`U1`) takes the library's `SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.81mm` model with its exposed pad cut to the footprint's 3.3 mm. The STEP is reopened in OpenCASCADE and compared with the PCB: the 123 components of the STEP are the 123 footprints with a body, each under its reference and none repeated; every component origin is the footprint's position (worst difference 0.0000 mm) and every rotation the footprint's (0.000 deg); the board solid is the Edge.Cuts extent (79.2 x 70.7 mm); its holes are the PCB's drills (13 of 0.75 mm, 2 of 0.8 mm, 11 of 1.0 mm and the 4 mounting holes of 2.5 mm); and no body lies more than 0.25 mm outside its courtyard, connectors included. The board solid is 1.51 mm thick; the PCB's general thickness is 1.6 mm. KiCad's STEP writer numbers its entities differently on every run, so the file hash is not reproducible and the geometry hash is stated instead: `e1a6d41ea9`, the SHA-256 of the sorted multiset of the coordinates of the file's `CARTESIAN_POINT` entities.

## JLCPCB

No JLCPCB DFM, BOM-matching or quote run is recorded for Gerber content hash `a26463bd09`.

## How to check

From this directory, with the KiCad 10 footprint and symbol library tables configured:

```bash
sha256sum -c SHA256SUMS
kicad-cli pcb drc --severity-error kicad/irrigation_controller.kicad_pcb
kicad-cli sch erc kicad/irrigation_controller.kicad_sch
SYNTH_USER_REGISTRY_DIR=registry synth validate irrigation_controller.synth
```

The first command verifies every file below `SHA256SUMS`; the Gerber content hash is in `fab/GERBER_CONTENT_SHA256` and the STEP geometry hash is stated above. `registry/` holds the parts this board needs that the shipped registry does not carry.

## Human review

Every part of the board is recorded as reviewed, with the review date and the datasheet or product page, in the part files of [`registry/`](registry/) (`reviewed_by`, `reviewed_at`, `datasheet_url`): JeffMboya for every part except `ltst_c190kgkt`, which keeps its registry record (Steve Munene, 2026-10-29).

### PCB review sign-off

PCB review sign-off required by #39: JeffMboya, on the package in `fab/` (Gerber content hash `a26463bd09`, STEP geometry hash `e1a6d41ea9`). The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review. The independent PCB expert sign-off required by #39 is open, and no JLCPCB DFM or BOM-matching run is recorded for this Gerber hash.

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Return paths | |
| ERC | |
| DRC | |
| Input protection | |
| Placement | |
| Parts and values | |

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The exports are recorded in [`PROVENANCE.md`](PROVENANCE.md). Each cell is the first ten characters of a SHA-256. All three exports are `routed`, with no blocking reason, 0 DRC errors and 0 unconnected pads.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `irrigation_controller.kicad_sch` | f55dfd0c1d | f55dfd0c1d | f55dfd0c1d | byte-identical |
| `irrigation_controller.kicad_pro` | 52cf078ec0 | 52cf078ec0 | 52cf078ec0 | byte-identical |
| `irrigation_controller.kicad_sym` | 37a5d9275e | 37a5d9275e | 37a5d9275e | byte-identical |
| `bom.csv` | 797f04fbe3 | 797f04fbe3 | 797f04fbe3 | byte-identical |
| `pnp.csv` | d95d269bfd | d95d269bfd | d95d269bfd | byte-identical |
| `irrigation_controller.synth.kicad_pcb` | de430875aa | de430875aa | de430875aa | byte-identical |
| `irrigation_controller.kicad_pcb` | 965d113d85 | bacc961188 | b478a105a0 | not byte-identical; identical after the router's and KiCad's random ids are masked (normalized) |
| Gerber and drill content | a26463bd09 | a26463bd09 | a26463bd09 | identical after the creation time in the headers is masked (normalized) |
