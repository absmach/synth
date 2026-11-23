# Qualification: Multi-rail compute node with USB-C and a 5 to 12 V input (Class C, not a case-4 pass)

**Status:** the board is routed and released. It has 42 of 42 nets, and KiCad 10.0.8 reports 0 errors and 0 unconnected pads.

- The exported KiCad files, the fabrication package and the STEP model are committed in [`kicad/`](kicad/), [`fab/`](fab/) and [`models/`](models/).
- One part of the claim is not met. Synth has no statement for a thermal-via array, so the array is written as hand-placed vias in the placement sidecar, as described below.
- The thermal array is six vias (2 x 3) against the 4 x 3 TI recommends, because the exposed pad cannot hold 12 vias at 1.2 mm pitch.
- The junction temperature of `U1` is not estimated. Its efficiency curve was not read.
- The board is a multi-rail contribution, not a pass of case 4 on keepout or thermal.

## What was measured

All figures come from the committed export (KiCad 10.0.8).

| | |
|---|---|
| Outline | 85.0 x 63.0 mm, 5,354 mm2, 85 parts (71 excluding mounting holes, fiducials and test points) |
| Courtyard fill | **20.5 %** of the board area, measured on the exported board (`layout_metrics.py`); the Class C target is 30 %, which at this outline needs about 1,606 mm2 of courtyard (the board holds about 1,098 mm2) |
| Stackup | four copper layers, `manufacturer "jlc-filled-via"` with `requires via_in_pad` (resin-filled and capped vias, needed for the thermal vias in the buck's exposed pad), 1.52 mm in the STEP model. The declared stack is 0.035 mm copper, 0.21 mm prepreg (er 4.4), 0.0175 mm copper, 1.065 mm core (er 4.6), 0.0175 mm copper, 0.21 mm prepreg, 0.035 mm copper, which is JLCPCB's 7628 four-layer stack ([JLCPCB stackup page](https://jlcpcb.com/quote/pcbOrderFaq/PCB%20Stackup): 7628 prepreg, 1.065 mm core; the 0.21 mm figure is from a community copy of that stack) |
| Routing | 42 of 42 nets, 434 segments, 90 vias (0.46/0.2 mm: 23, 0.6/0.3 mm: 67), 1285.9 mm of track |
| Copper | `GND` is poured on all four layers (a zone on each); the rails are routed as tracks, not planes |
| DRC with the library tables | 0 errors, 0 unconnected, 89 warnings: 85 `lib_footprint_mismatch` (silkscreen cut back from vias and pads by the exporter, #425; the footprint comparison shows no difference in pads, courtyard or fabrication layers) and 4 `silk_over_copper` |
| Synth validation | exits 0 with 48 warnings in the author's `synth validate` run; 19 are schematic-layout findings (see the notes below the table) |

Notes on the `synth validate` row:

- The 19 schematic-layout findings are 9 `E-SYNTH-SCHEM-003` (decoupling capacitors in another schematic sheet group than their IC, up to 455 mm apart on the sheet), 7 `E-SYNTH-SCHEM-001` (a power flag drawn pointing down), 2 `E-SYNTH-SCHEM-004` (a wire longer than 100 mm) and 1 `E-SYNTH-SCHEM-005` (a node with 4 wire lines).
- They concern the drawing of the generated schematic, not the netlist or the board. They are accepted as a group, not itemised, and no per-finding record is committed. The counts are from that run.
- `E-SYNTH-CONNECT-009` is explained under "Power good".

## Thermal

The rails run from the 3.611 V buck output. The dissipation of each linear regulator is its input-output drop times its current; the junction temperature is Tj = TA + P x RthetaJA, with the datasheet's junction-to-ambient resistance (a JEDEC figure that depends on the board; TI states its LMR33640 table value is not a design value). The last two columns give the load current at which Tj reaches 125 C at 50 C and at 85 C ambient.

| Part | Drop | RthetaJA used | At its rated current | Current limit, TA 50 C / 85 C |
|---|---|---|---|---|
| `U2` LP5907 3.3 V | 0.311 V | 193.4 C/W (TI datasheet, DBV) | 78 mW at 250 mA, +15 K | thermally not limiting (1.25 A / 0.67 A against the 250 mA rating) |
| `U3` AP2112K-1.8 | 1.811 V | 184 C/W (Diodes AP2112 datasheet, SOT25, no heatsink) | 1.09 W at its 600 mA rating, +200 K, beyond 125 C | 0.23 A / 0.12 A |
| `U4` TLV70012 1.2 V | 2.411 V | 262.8 C/W (TI datasheet, DDC) | 482 mW at 200 mA, +127 K, beyond 125 C | 0.118 A / 0.063 A |
| `U5` TPS22918 | 53 mohm x I2 | 183.2 C/W (TI datasheet, DBV) | 53 mW at 1 A, +9.7 K; 212 mW at 2 A, +38.8 K | not limiting |

The board has no load of its own on the 1.8 V and 1.2 V rails; they are exposed on `J4`, so the limits above are the currents an external load may draw from them. `U1`'s junction temperature is not estimated: the efficiency curve of the datasheet was not read, so its dissipation is not known here. The STM32F103C8 junction-to-ambient figure for LQFP-48 is 55 C/W (read from datasheet mirrors).

**USB-only operation.** With the DC header unpowered, the buck input is `VBUS` less the drops of `F2` and `D4`.

- Worst case: VBUS at 4.75 V (5 V less 5 %) and 1.0 A drawn. The 1.1 A hold current of `F2` is the upper bound.
- `F2` is 0.21 ohm (Bourns R1Max) and drops 0.21 V. `D4` drops 0.45 V (1N5819HW at 1 A).
- `U1` sees 4.75 - 0.21 - 0.45 = 4.09 V.
- TI's LMR33640 datasheet gives a 3.8 V minimum input and a dropout of 150 mV at 1 A (measured at a 5 V output). A 3.611 V output therefore needs 3.76 V at the input.
- The margin is 0.33 V to the dropout and 0.29 V to the 3.8 V minimum. The buck stays in regulation.
- The LP5907 in SOT-23 needs at most 250 mV at 250 mA (TI datasheet, dropout voltage). It needs 3.55 V, and the 3.611 V rail leaves 61 mV.
- The output current the buck can deliver from USB at 4.09 V is not stated, because its efficiency curve was not read.

**Thermal vias.**

- The exposed pad of `U1` (pin 9, 2.41 x 3.1 mm, tied to `GND`) carries a 2 x 3 array of six 0.46/0.2 mm ground vias at 1.2 mm pitch. They sit inside the pad at x 35.4 and 36.6 mm, y 8.8, 10.0 and 11.2 mm, measured on the export.
- Synth has no statement for a thermal array. The vias are `[[vias]]` entries of [`multirail_compute_node.placement.layout.toml`](multirail_compute_node.placement.layout.toml), written before routing so the router sees them as existing `GND` copper.
- TI's LMR33640 datasheet (section 9.5.1.1, as the Class B buck README cites it) asks for at least 4 x 3 thermal vias. A pad of this size holds six at this pitch.
- The `GND` pour on all four layers is the copper area. No copper area per dissipating part is declared, because Synth has no statement for it.

**Keepouts.**

- The source declares `keepout hot_buck { radius 5mm anchor "U1" allow "VIN", "SW", "BOOT", "VCC_BUCK", "FB", "PG_3V6", "V3V6", "GND" }`.
- It is a 5 mm ring centred on `U1`. The anchor is supported by `main` since [#449](https://github.com/absmach/synth/issues/449) closed.
- The nets of the buck itself are allowed through, so its own routing is not refused.
- An export of this source carries a `hot_buck keepout` rule area around `U1`, and the two mounting-hole screw-head zones (`H1 screw head`, `H2 screw head`, 2.75 mm radius).
- `synth validate` no longer reports `E-SYNTH-KEEPOUT-003`.

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

The net classes set the widths (all declare `edge_rate slow` except `BUCK_SW`, `USB` and `SPI`): `PWR` 0.6 mm (IPC-2221 external 1 oz at 10 K rise: about 1.6 A), `PWR_LOW` 0.3 mm (about 1.0 A), `GNDC` 0.4 mm, `BUCK_SW` 0.6 mm on `F.Cu` (fast, with the switch node), `SIG_SLOW` 0.2 mm, `SPI` (no edge rate declared, so counted fast) and `XTAL` (the crystal nets) 0.2 mm locked to `F.Cu` so they make no layer change and need no return via, and `USB` 0.2 mm, locked to `F.Cu` like `XTAL` so the pair makes no layer change and needs no return via.

**USB.**

- The pair runs `J1` to `PA12` and `PA11` of `U6`, full speed (12 Mbit/s) only, with the 1.5 k pull-up `R13` on D+.
- Delivered lengths are 111.8 mm (D+) and 92.7 mm (D-), a 19.1 mm difference, both on `F.Cu`.
- At the 5.5 ps/mm used for the case 1 estimate that is about 105 ps, a small fraction of the 4 ns minimum full-speed edge.
- The `USB` class declares `edge_rate medium`. Transceiver datasheets give full-speed rise and fall times of 4 to 20 ns (10 to 90 %; onsemi USB1T20, TI TUSB1210), far above 0.3 ns.
- Synth's rule treats a differential pin as fast. The Synth commit named in [`PROVENANCE.md`](PROVENANCE.md) lets a declared `medium` replace that inference.
- No `diff_pair` is declared for the pair.

**Power good.** `U1`'s `PG` pin (open drain) is the net `PG_3V6`, which goes to `PA0` of `U6` and is pulled up by `R1`, 10 k, to the `V3V3` rail: 3.3 V / 10 k = 0.33 mA when `PG` pulls low. The validator warning `E-SYNTH-CONNECT-009` (open-drain net without a pull-up) names this net because the source declares `V3V3` with `net`, not `power`, and the rule counts only a resistor to a declared power net as a pull-up; the resistor is on the board.

## Sourcing

[`fab/bom.csv`](fab/bom.csv) has 85 lines: 71 parts and 14 board features. Each part has a manufacturer, MPN, package, rating, datasheet or page, distributor and order number. The features (2 mounting holes, 3 fiducials, 9 test points) are not parts and carry no identity.

Notes that carry a decision:

- The registry's LCSC number for the LP5907 pointed at a different part, so the table names `C80670`.
- `U3` is the Diodes AP2112K-1.8TRG1 (`C176944`, found by search as the Diodes part in SOT-25). The AP2112 datasheet gives 600 mA, 500 mV typical dropout at 600 mA and 184 degC/W junction-to-ambient for the SOT-25.
- LCSC has no UNI-ROYAL 3.83 k, so the table names a Yageo (`C482840`).
- One distributor lists the Bourns inductor as discontinued.
- The two registry "amber" LEDs are the yellow KENTO KT-0603Y (584 to 594 nm).
- The junction capacitance of the PESD5V0S1BA on the USB lines was not read.
- `J2`, `J3` and `J4` are through-hole hand-fit parts. `J4` (Harwin) has no LCSC listing, so it is sourced from DigiKey.

The part pages were read in 2026-11 and the stock is not firm.

## Fabrication package

[`fab/`](fab/): `gerbers.zip` (the Gerbers and drill file: 14 files, namely four copper layers, two mask, two paste, two silkscreen, one Edge.Cuts, one drill file, one drill map PDF and one job file), `bom.csv` (the orderable BOM) and `cpl.csv` (the placement file); the STEP model is in [`models/`](models/); every file is listed in [`SHA256SUMS`](SHA256SUMS). Models the KiCad library lacks are written from datasheet dimensions (the test points as 1 oz copper discs, the 1812 fuses as the Bourns datasheet's mid-range block).

## Reproducibility

The source and sidecar in this directory were exported with `synth export-kicad --gerbers --drill --step --pnp` (KiCadRoutingTools `9399bfb2`, KiCad 10.0.8) into an empty directory.

- A `synth_export` call of `synth mcp --stdio` on the same files gives the same result: 42 of 42 nets, 434 segments, 90 vias, 1285.9 mm of track and 0 blocking findings.
- The `.kicad_sch`, `.kicad_pro` and `bom.csv` are identical.
- The board differs from the committed one in the order of the router's output and in random ids.
- Everything committed comes from the first export.

How the board was made is described in [`PROVENANCE.md`](PROVENANCE.md).

## Images

[`images/`](images/) holds `top.png`, `bottom.png` and `angled.png` (3D renders of the board) and `schematic.pdf` (every schematic sheet on one page).

## How to check

Follow these steps from this directory.

1. Install KiCad 10 and configure its footprint and symbol library tables. The package does not include the tables, so the `kicad-cli` commands need them.
2. Verify every file listed in `SHA256SUMS`:

   ```bash
   sha256sum -c SHA256SUMS
   ```

3. Run the DRC and ERC on the exported board and schematic:

   ```bash
   kicad-cli pcb drc --severity-error kicad/multirail_compute_node.kicad_pcb
   kicad-cli sch erc kicad/multirail_compute_node.kicad_sch
   ```

4. Validate the source:

   ```bash
   synth validate multirail_compute_node.synth
   ```

The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/`. The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review. The independent PCB expert sign-off required by #40 is open, and no JLCPCB DFM or BOM-matching run is recorded for this package. No item below is signed off yet; the table is the checklist for the review, and the independent PCB expert sign-off required by #40 is open.

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
