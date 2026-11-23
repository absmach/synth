# Qualification: iCE40HX4K SDR SDRAM node with a USB-C power input (Class C, case 2)

**Status:** the board is routed and released. The export's routing report shows 69 of 69 nets connected and 0 open.

- The exported KiCad files, the fabrication package and the STEP model are committed in [`kicad/`](kicad/), [`fab/`](fab/) and [`models/`](models/).
- The numbers in this document come from the author's run of the export this package was built from. No report file is committed.
- Case 2 is claimed for the routing of the memory lines only. No controller or bitstream has run on the board, so no interface timing is claimed.

## What was measured

All figures come from the files of the export this package was built from (KiCad 10.0.8).

| | |
|---|---|
| Outline | 70.4 x 67.0 mm, 85 parts (76 excluding mounting holes and test points) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"`, JLCPCB's `JLC06161H-3313` stackup as declared in the source (prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um; [JLCPCB impedance tool](https://jlcpcb.com/impedance)), 1.47 mm in the STEP model |
| Routing | 69 of 69 nets, 867 segments, 187 vias (0.46/0.2 mm: 130, 0.6/0.3 mm: 57), 1707.7 mm of track, measured on the delivered board; the router's report (not committed) agrees |
| Impedance | `SDCLK` declares 50 ohm: 0.15 mm tracks on the outer layers over 0.0994 mm of prepreg give Z0 = 87/sqrt(er+1.41) x ln(5.98 h / (0.8 w + t)) = 87/2.347 x ln(0.5944/0.155) = 49.8 ohm (closed-form microstrip estimate, not a field solve). `SDDATA` and `SDADDR` declare no impedance target: the ISSI IS42S16400J-7 AC characteristics retrieved for this board give timing only, the nets are LVTTL with 22 ohm series resistors, and their 0.15 mm width is the class width on every layer |
| Data bus | 8-bit use of the 16-bit part: the SDRAM's upper byte `DQ8` to `DQ15` (the pin row that faces the FPGA) carries data; `SD_LDQM` is held high by the FPGA, which masks the lower byte, and `DQ0` to `DQ7` each go to ground through their own 10 k resistor, `RPD1` to `RPD8`, so no two outputs share a net. IS42S16400J behaviour with a masked byte: ignored on a write, high-impedance on a read |
| Data bus lengths | `DQ8` to `DQ15`: 14.0, 13.7, 11.5, 17.7, 11.6, 12.0, 14.1 and 12.3 mm; spread 6.3 mm against the class budget `max_skew 10 mm` and `max_length 60 mm` |
| Clock | `SD_CK` 10.1 mm from the FPGA to the series resistor plus 6.8 mm beyond it, 16.9 mm in all, against `max_length 40 mm` |
| Address, bank and control group | lengths from 10.9 mm (`SD_A11`) to 65.2 mm (`SD_CS`, both nets of its series resistor), a spread of 54.3 mm, inside the declared `max_skew 60 mm` (details below the table) |
| Edge rate | `SDDATA` and `SDCLK` declare `edge_rate fast` (the SDRAM drives the data bus on a read, and no output-edge figure for the IS42S16400J was found, so no slower claim is made). `SDADDR` declares `edge_rate medium`: the FPGA alone drives it, and Lattice's IBIS model `ice40.ibs` rev 2.5 gives 20 to 80 % rise and fall times of at least 0.976 ns (banks 0 to 2, `lvc330io`) and 0.728 ns (bank 3, `lvc330_b3io`) at the fast corner, so no edge is faster than 0.3 ns. The medium window is 6 mm ([#445](https://github.com/absmach/synth/issues/445)) |
| Warnings | 84 `lib_footprint_issues` in the export's `kicad-cli` DRC report (not committed): silkscreen cut back from vias and pads by the exporter, #425; no other warning type |
| Zone fill | The committed board carries refilled zones (`kicad-cli pcb drc --refill-zones --save-board`), and the Gerbers are plotted from that fill. `kicad-cli pcb drc` without a refill reports 0 unconnected items on it |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The engine's counters record 128 vias claimed and 0 in pads, and "rules below board floor: none reported". The delivered board's own counts, 187 vias in all, are the ones this document uses |

Note on `synth validate`: it reports `W-SYNTH-THERMAL-001` on `Q2`. The warning applies the generic package rating of the AO3400A to an LED switch. It does not reflect a real load.

Notes on the address, bank and control group:

- The `SDADDR` class declares `max_skew 60 mm` and `max_length 80 mm`. The `SDDATA` class declares `max_skew 10 mm` and `max_length 60 mm`.
- The delivered address, bank and control group spans 54.3 mm, inside the 60 mm budget. The longest net is `SD_CS` (65.2 mm with its series resistor) against `max_length 80 mm`.
- The limits are chosen against the IS42S16400J-7 timing in the ISSI datasheet, Rev. G, AC characteristics: a 7 ns minimum clock cycle at CAS latency 3 (143 MHz), address setup 1.5 ns and hold 0.8 ns, and command setup 1.5 ns and hold 0.8 ns. They leave a 4.7 ns window.
- A 60 mm group skew is 0.42 ns at 7.0 ps/mm (stripline, er 4.4), 8.9 % of that window. The delivered 54.3 mm is 0.38 ns, 8.1 %. An 80 mm run is 0.56 ns of delay.
- No controller runs on this board, so the window is not exercised and no interface timing is claimed.
- Synth's export gate does not run the class skew check (`E-SYNTH-DRC-019`): [#147](https://github.com/absmach/synth/issues/147) (skew budgets are not carried through export).

### Return paths

The rule `W-SYNTH-DRC-016` asks for a ground via within a window of the thickest dielectric (floor: pad plus clearance) of a layer-change via on a fast net. The `SDDATA` and `SDCLK` classes declare `edge_rate fast`. The `SDADDR` class declares `edge_rate medium`, which widens the window to 6 mm.

Measured on the delivered board, centre to centre, the distance from each via on the SDRAM signal nets to the nearest ground via:

| Nets | Vias | Nearest ground via (mm) |
|---|---|---|
| `SD_DQ` | 10 | 0.60 to 0.64, median 0.60 |
| `SD_CK` and `SD_CK_F` | 4 | 0.61 to 2.48, median 1.20 |
| Address and control | 49 | 0.60 to 5.45, median 2.31 |

- Some vias on the fast clock nets are farther from a ground via than the fast-net window. This document does not claim the return-path rule is met for them.
- Ground is poured on all six layers (a `GND` zone on each), and the supply nets are routed as tracks.

## 3.3 V regulator budget and input surge

`U1` (AP7361C-33E, SOT-223; Diodes DS37274 Rev. 5-2: 1 A output, 360 mV typical dropout at 1 A, input 2.2 to 6.0 V recommended and 6.5 V absolute maximum, SOT223 `RthJA = 110 degC/W`) carries every load on `+3V3`. Known loads, as datasheet or source maxima:

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U2` TLV70012 1.2 V core regulator, carrying the iCE40 core and PLL | 28.7 mA (22.3 mA core plus 6.4 mA PLL, startup peak maxima; a linear regulator draws its output current from the input) | Lattice DS1040 v3.4, Peak Startup Supply Current, HX devices |
| `U4` IS42S16400J-7 SDRAM | 90 mA (`Idd4`, all banks active, BL = 4, CL = 3, minimum clock cycle; maximum) | ISSI IS42S16400J datasheet Rev. G, DC Electrical Characteristics 1 |
| `U3` FPGA I/O banks and SPI supply | 34 mA (6.8 mA startup peak for each of five supplies: `VCCIO_0` to `VCCIO_3` and `VCC_SPI`; assumes the figure is per supply) | Lattice DS1040 v3.4, Peak Startup Supply Current, HX devices |
| `U3` NVCM supply `VPP_2V5` | 4.1 mA (startup peak) | Lattice DS1040 v3.4, Peak Startup Supply Current, HX devices |
| `U5` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Three LEDs, 1 k | 10 mA (3.3 V / 1 k each, which ignores the LED forward drop) | the design's own arithmetic |
| Total known | 192 mA | |

The oscillator's supply current and the FPGA's dynamic current are not counted. The core and I/O figures are startup peak maxima for a blank pattern. The dynamic current depends on the bitstream, and none has run on this board. `U2` dissipates (3.3 V - 1.2 V) x 0.0287 A = 0.06 W.

With `R_SURGE` (1 ohm) ahead of the regulator, the input is at most 5.02 V at the 5.25 V maximum VBUS. The board is rated for 70 degC ambient.

- The regulator dissipates (5.02 V - 3.3 V) x 0.192 A = 0.33 W, a 36 degC rise.
- Tj is 61 degC at 25 degC ambient and 106 degC at 70 degC ambient, under 125 degC.
- Tj reaches 125 degC at `(125 - 25) / 110 / 1.72 = 0.53 A` in total at 25 degC ambient, which leaves 0.34 A for the uncounted loads.
- Tj reaches 125 degC at `(125 - 70) / 110 / 1.72 = 0.29 A` at 70 degC ambient, which leaves 0.10 A for the oscillator and the FPGA's dynamic current.
- Dropout is 360 mV typical at 1 A. At the 4.52 V worst-case input (4.75 V less `F1` and `R_SURGE`), the output stays in regulation with 1.22 V to spare.

`D1` (Nexperia PTVS6V0S1UR) has a 6.0 V standoff above the 5.25 V maximum VBUS, a 6.67 V minimum breakdown and a 10.3 V clamp at 38.8 A. `U1` is rated 6.5 V absolute maximum at its input. The low-dropout regulators this board could use (AP7361C, AP2114, AP2112K, LP5907) are all rated 6.5 V or less, below the least breakdown of any TVS with a standoff above 5.25 V.

So `R_SURGE`, 1 ohm, sits between `D1` and `U1`, with `C2` and `C3` behind it. A short clamp event is held off the regulator, and a sustained over-voltage is limited by `F1` opening. The arithmetic and the one remaining gap are in the requirement check below.

### Requirement check: regulator and input protection

`R_SURGE`, 1 ohm (Yageo RC1206FR-071RL), sits between the TVS node (`D1`, `F1`, `C1`) and the regulator input, with `C2` and `C3` (20 uF) behind it.

| Requirement | Our choice | Arithmetic | Result |
|---|---|---|---|
| The regulator input stays inside its recommended range in operation | `U1` AP7361C-33E, input 2.2 to 6.0 V | VBUS is at most 5.25 V; less `F1` (0.21 ohm x 0.192 A = 0.04 V) and `R_SURGE` (0.192 V) the input is at most 5.02 V, 0.98 V under 6.0 V | Met |
| Enough headroom for the dropout | 360 mV typical at 1 A (Diodes DS37274) | at the 4.75 V minimum the input is 4.75 - 0.04 - 0.192 = 4.52 V; 4.52 V - 3.3 V = 1.22 V against 0.36 V at 1 A, less at 0.192 A | Met |
| The regulator carries every load | 1 A output | known loads 0.192 A, 19 % of the rating; the uncounted oscillator and dynamic currents fit electrically up to 0.81 A | Met electrically |
| The regulator sheds its dissipation | SOT-223, `RthJA` 110 C/W | (5.02 V - 3.3 V) x 0.192 A = 0.33 W gives 106 C at 70 C ambient and 61 C at 25 C ambient; the total at which Tj reaches 125 C is 0.53 A at 25 C and 0.29 A at 70 C | Met for the known loads; at 70 C the uncounted loads have 0.10 A |
| `R_SURGE` carries the load | 1 ohm, 250 mW | 0.192 A squared x 1 ohm = 0.037 W | Met |
| The TVS does not conduct in operation | `D1` PTVS6V0S1UR, 6.0 V standoff, 6.67 V minimum breakdown | 5.25 V maximum VBUS is 0.75 V under the standoff | Met |
| The regulator survives a short clamp event | `U1` absolute maximum input 6.5 V | `D1` clamps at up to 10.3 V at 38.8 A (Nexperia PTVS6V0S1UR: 400 W, VRWM 6 V, 6.67 to 7.37 V breakdown); the current through `R_SURGE` is at most (10.3 V - 4.5 V) / 1 ohm = 5.8 A, so a 60 ns event carries 0.35 uC, which raises 20 uF by 0.02 V and holds the regulator input near 5 V; the energy in the resistor is about 2 uJ (its pulse rating was not read) | Met for events of 60 ns |
| The regulator survives a sustained over-voltage | `U1` absolute maximum input 6.5 V | `D1` holds the TVS node at about 7.0 V (typical breakdown) until `F1` (1.1 A hold, 2.2 A trip) opens; the regulator input is then 7.0 V - 0.192 V = 6.8 V, 0.3 V over its rating | Not met until `F1` opens |

The sustained over-voltage row is the one gap. No low-dropout regulator considered (AP7361C, AP2114, AP2112K, LP5907, all rated 6.5 V or less at the input) closes it, and the parts that are rated above the clamp (AMS1117-3.3 and NCP1117ST33T3G, 15 V and 20 V) need an ESR-stabilised output capacitor, where this board has ceramics, and have `RthJA` 160 C/W on a minimum pad for the SOT-223 (onsemi NCP1117), against 110 C/W here. Closing the row would need a regulator rated above the clamp with ceramic stability.

## Layout and fill

The placement is pinned in [`ice40_sdram_node.placement.layout.toml`](ice40_sdram_node.placement.layout.toml).

- The SDRAM and the FPGA face each other, with the series resistors between them.
- The USB-C receptacle and the power chain run along the bottom.
- The flash and the programming header are on the right.
- The header `J2` has its courtyard 0.98 mm from the screw-head keepout of mounting hole `H2` (a 5.5 mm square around the hole) and 1.57 mm inside the board's right edge.

## Sourcing

[`fab/bom.csv`](fab/bom.csv) has 85 lines: 76 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number, and 9 board features (2 mounting holes, 7 test points) that are not parts and carry no identity. `J2` is a hand-fit through-hole part. The part pages were read in 2026-11 and the stock is not firm. The IS42S16400J figures are from the ISSI datasheet Rev. G (copy at static.chipdip.ru). `D6` and `D7` have no distributor listing yet: their BOM lines carry the manufacturer, MPN and datasheet, and the order number is to be added.

## Fabrication package

[`fab/`](fab/) holds three files:

- `gerbers.zip` (18 files: 13 Gerber layers including the four inner layers, two drill files, two drill maps and the job file)
- `bom.csv` (the orderable BOM)
- `cpl.csv` (the placement file)

The STEP model is in [`models/`](models/). Every file is listed in [`SHA256SUMS`](SHA256SUMS).

The STEP model is exported with KiCad. The test points are 1 oz copper discs and the fuse is a block of the Bourns datasheet's mid-range dimensions (A 4.55, B 3.24, C 0.70 mm). Both are written from datasheet dimensions.

## Reproducibility

Export the source and sidecar in this directory through the Synth MCP server to reproduce the board.

1. Start the server with `synth mcp --stdio`.
2. Call the `synth_export` tool, the call an agent makes, with an empty directory as the output and the router included.
3. Compare the result with the committed files.

The committed export used KiCadRoutingTools `9399bfb2` and KiCad 10.0.8. It gave 69 of 69 nets, 867 segments, 187 vias and 1707.7 mm of track. Everything committed comes from that export. [`PROVENANCE.md`](PROVENANCE.md) describes how the board was made.

## Images

[`images/`](images/) holds `top.png`, `bottom.png` and `angled.png` (3D renders of the board) and `schematic.pdf` (every schematic sheet on one page).

## How to check

Configure the KiCad 10 footprint and symbol library tables first. Then run these steps from this directory.

1. Verify every file listed in `SHA256SUMS`:

   ```bash
   sha256sum -c SHA256SUMS
   ```

2. Run the KiCad DRC on the committed board:

   ```bash
   kicad-cli pcb drc --severity-error kicad/ice40_sdram_node.kicad_pcb
   ```

3. Validate the source:

   ```bash
   synth validate ice40_sdram_node.synth
   ```

Note: the sign-off table names the package in `fab/`. The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/`.

- The sign-off is JeffMboya's own.
- Jeff Mboya is the author of this pull request, so the table below is the author's self-review.
- The independent PCB expert sign-off required by #40 is open.
- No JLCPCB DFM or BOM-matching run is recorded for this Gerber hash.

No item below is signed off yet. The table is the checklist for the review, and the independent PCB expert sign-off required by #40 is open.

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
