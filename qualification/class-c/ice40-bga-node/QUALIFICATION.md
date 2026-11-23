# Qualification: iCE40HX4K BGA node with a USB-C input and an MCU (Class C, case 1)

**Status:** the board is routed and released.

- All 42 of 42 nets are routed. The export's routing report shows 0 open nets.
- KiCad 10.0.8 with zone refill reports 0 unconnected items on the committed board.
- The exported KiCad files, the fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)).

The numbers and files below all come from one export, made through the Synth MCP server (see [`PROVENANCE.md`](PROVENANCE.md)). The report files are not committed.

## What was measured

All figures come from the files of the export this package was built from (KiCad 10.0.8).

| | |
|---|---|
| Outline | 60.9 x 48.5 mm, 81 parts (74 excluding mounting holes and test points) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"` (the JLCPCB tier that offers the 0.2 mm drill and via-in-pad on six layers), `requires via_in_pad`, 1.37 mm in the STEP model |
| Routing | 42 of 42 nets, 639 segments, 142 vias (0.35/0.2 mm: 21, 0.46/0.2 mm: 16, 0.6/0.3 mm: 105), 1396.5 mm of track, measured on the delivered board; the router's report (not committed) agrees |
| Plane | `+3V3` is poured on the inner layers (`plane` on its net class); ground on the outer layers; the 1.2 V core rail, the PLL supply and the MCU supply `+3V3_MCU` are routed |
| BGA escape | `escape { method underpad }` on `U1`; the board has 21 vias at 0.35/0.2 mm, the size of a filled and capped via in a ball pad; see the BGA escape notes below the table |
| USB-C | `J1` sits on the left edge with its mating opening facing the edge (rotation 270, courtyard 0.05 mm from Edge.Cuts); `escape { off }` on `J1` and on the MCU `U5`, which are not ball grids |
| USB pair | D+ and D- run from `J1` through 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11`, declared as 90 ohm differential pairs (the connector side and the MCU side) in the `USB` class (0.12 mm tracks, locked to `F.Cu`, `edge_rate fast`); see the USB pair notes below the table |
| DRC rules | The 21 vias at 0.35/0.2 mm are below the 0.46 mm and 0.13 mm floors of the board setup. The rules file `kicad/ice40_bga_node.kicad_dru` lowers the floor to 0.35 mm and 0.075 mm for vias of that size inside the courtyard of `U1`, the filled and capped via-in-pad process of `jlc-advanced`. KiCad loads the file when it sits beside the board. Without it, DRC reports 21 `via_diameter` and 21 `annular_width` errors |
| Zone fill | The committed board carries refilled zones (`kicad-cli pcb drc --refill-zones --save-board`), and the Gerbers are plotted from that fill. `kicad-cli pcb drc` without a refill reports 0 unconnected items on it |
| Warnings | 83 in the export's `kicad-cli` DRC report with zone refill (not committed): 81 `lib_footprint_issues` (silkscreen cut back from vias and pads by the exporter, #425), 1 `silk_overlap` and 1 `silk_over_copper`; 0 errors and 0 unconnected items with the rules file `kicad/ice40_bga_node.kicad_dru` beside the board |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy; see the engine verdict notes below the table |

BGA escape notes:

- A filled-and-capped via in a pad is the process JLCPCB's via-in-pad guide describes: land of the drill plus 0.15 mm.
- A dog-bone escape was tried and does not fit. An export with `escape { method channel }` and without `requires via_in_pad` stopped at the power-plane stage with `net +3V3: no legal stitching-via position beside the pad at (38.00, 36.40) mm on Top: every candidate is outside the board or within clearance of another pad or via`.
- The board therefore keeps `requires via_in_pad`.

USB pair notes:

- Measured on the delivered board: D+ is 114.7 mm (84.4 mm from `J1` to `R25`, 30.3 mm beyond) and D- is 175.5 mm (95.0 mm from `J1` to `R26`, 80.5 mm beyond), a 60.8 mm difference, all on `F.Cu`.
- The pair is not length-matched, and its impedance is an analytic estimate, not a field solve.
- The supported operation is USB full-speed (12 Mbit/s) only. The STM32F103 `U5` is a USB 2.0 full-speed device (ST datasheet DS5319, "USB 2.0 full-speed interface"), and `PA11`/`PA12` carry no high-speed PHY.
- Full-speed drivers have 4 to 20 ns rise and fall times and a 90 ohm differential +/-15 % target.
- The USB section below cites the pages and shows the arithmetic: 94 ohm differential estimated from the stackup, and 60.8 mm of skew, about 334 ps, 8.4 % of a 4 ns minimum rise time.

Engine verdict notes:

- The engine reports no rule below the board floor (`rules_below_board_floor: none reported`).
- Its `claimed_total_vias: 67` and `claimed_via_in_pad: 57` count the vias its own routing and fanout steps placed.
- Synth's later steps add the plane, return and stitch vias and move vias out of the pads of components that do not declare an under-pad escape.
- The board's own counts are the ones this document uses: 142 vias, 21 of them 0.35/0.2 mm.
- The delivered board is judged by Synth's own checks: connectivity (42 of 42), the process floor and KiCad DRC. JLCPCB's via-in-pad page gives the land for that via.

## 3.3 V regulator thermal budget

`U3` (AMS1117-3.3, SOT-223) drops 5 V to 3.3 V.

- It dissipates `P = (5.0 - 3.3) x I`, and its junction sits at `Tj = TA + P x RthJA`.
- The datasheet limits the junction to 125 degC ([Advanced Monolithic Systems AMS1117 datasheet](https://files.waveshare.com/upload/b/b8/AMS1117-datasheet.pdf), a third-party print of the manufacturer's document): "Maximum junction temperature must not exceed 125 degC".
- For SOT-223 the datasheet gives `RthJA = 90 degC/W`, which "can vary from 46 degC/W to >90 degC/W depending on the size of the copper area". The board uses 90 degC/W, the top of that range.
- The Synth warning `W-SYNTH-THERMAL-001` comes from the registry part's declared 1200 mW rating at 90 degC/W (25 + 1.2 x 90 = 133 degC). It is the package limit, not this board's load.

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U4` TLV70012 1.2 V core regulator | 200 mA (its rated output current; a linear regulator draws its output current from the input) | TI TLV700 datasheet SBVS173, recommended operating conditions IOUT 0 to 200 mA |
| `U5` STM32F103C8 | 50 mA (72 MHz, all peripherals on, maximum run-mode supply current) | ST DS5319 as listed on a mirror ([radiolocman](https://www.radiolocman.com/datasheet/pdf.html?di=184185)); not read on st.com |
| `U2` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Four LEDs with 1 k resistors | 1.3 mA | the design's own arithmetic in the source |
| Total | 276 mA | |

At 276 mA the regulator dissipates 1.7 V x 0.276 A = 0.47 W, a 42 degC rise.

- Tj is 67 degC at 25 degC ambient and 112 degC at 70 degC ambient, under the 125 degC limit.
- The 3.3 V of `J3` can feed an external load. With that load, Tj reaches 125 degC at 25 degC ambient at `(125 - 25) / 90 / 1.7 = 0.65 A` in total, and at 70 degC ambient at `(125 - 70) / 90 / 1.7 = 0.36 A`.
- So `J3` can supply about 0.37 A and 0.08 A respectively on top of the 276 mA.
- The input fuse `F1` holds 0.5 A, which bounds the sustained 5 V current and with it the regulator's dissipation at 0.85 W (Tj 101 degC at 25 degC ambient).

Note: The load currents are datasheet maxima added together, which no operating state reaches at once.

## ESD on the connector signals

`E-SYNTH-ESD-001` flagged the four USB-C data pins (`D+` and `D-` on both receptacle rows).

- The board adds two PRTR5V0U2X arrays beside the receptacle: `D7` on `D+` and `D-`, and `D8` on `CC1` and `CC2`.
- The data lines also reach `U5` through the 22 ohm series resistors `R25` and `R26`.
- `D1` (a 6.0 V standoff TVS) protects `VBUS`.
- The MCU's own pins are rated 2 kV human-body-model (class 2) and 500 V charge-device-model according to ST DS5319, section 5.3.11, as listed on mirrors ([example](https://www.alldatasheet.net/html-pdf/516843/STMICROELECTRONICS/STM32F103C8T7/117354/60/STM32F103C8T7.html)). The datasheet itself was not read on st.com.
- The 12-pin header `J3` carries FPGA I/O through 100 ohm series resistors and carries no protection device either.

Note: The board is a bench programming node, and no IEC 61000-4-2 level is claimed for it.

## Layout and fill

The placement is pinned in [`ice40_bga_node.placement.layout.toml`](ice40_bga_node.placement.layout.toml): the USB-C receptacle on the left edge with its opening facing out, the MCU group between the receptacle and the FPGA, the 12-pin header `J3` on the top edge with nothing beyond it, the two mounting holes at opposite corners, and the power parts along the bottom.

## Sourcing

[`fab/bom.csv`](fab/bom.csv) has 81 lines: 74 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number, and 7 board features (2 mounting holes, 5 test points) that are not parts and carry no identity. Several LCSC pages show different stock figures between snapshots and are noted in the table; the part pages were read in 2026-11 and the stock is not firm. `J3` and `J4` are hand-fit through-hole parts. The BOM-matching findings are `U1` and `Y1` below the 5-board quantity and `J4` without a match.

## Fabrication package

[`fab/`](fab/) holds these files:

- `gerbers.zip` with 16 files:
  - 13 Gerber layers: `F_Cu`, `In1_Cu` to `In4_Cu` and `B_Cu` (the six copper layers), `F_Mask` and `B_Mask`, `F_Paste` and `B_Paste`, `F_Silkscreen` and `B_Silkscreen`, and `Edge_Cuts`
  - the drill file `ice40_bga_node.drl`
  - the drill map `ice40_bga_node-drl_map.pdf`
  - the job file `ice40_bga_node-job.gbrjob`
- `bom.csv` (the orderable BOM)
- `cpl.csv` (the placement file)

The STEP model is in [`models/`](models/). Every file is listed in [`SHA256SUMS`](SHA256SUMS).

- The STEP model is exported with KiCad. The USB-C body is the model KiCad's own `royalblue54L_feather` demo ships for this footprint, since the stock 3D library has none. The test points use the discs in [`models/`](models/).

No JLCDFM or BOM-matching run is recorded for this package, and no JLCDFM or BOM-matching report is committed.

## Reproducibility

Export the source and sidecar in this directory through the Synth MCP server to reproduce the board.

1. Start the server with `synth mcp --stdio`.
2. Call the `synth_export` tool, the call an agent makes, with an empty directory as the output and the router included.
3. Compare the result with the committed files.

The committed export used KiCadRoutingTools `9399bfb2` and KiCad 10.0.8. It gave 42 of 42 nets, 639 segments, 142 vias and 1396.5 mm of track. Everything committed comes from that export. [`PROVENANCE.md`](PROVENANCE.md) describes how the board was made.

## Images

[`images/`](images/) holds `top.png`, `bottom.png` and `angled.png` (3D renders of the board) and `schematic.pdf` (the schematic on one page).

## How to check

Follow these steps to check the board.

1. Change to this directory.
2. Configure the KiCad 10 footprint and symbol library tables.
3. Run the checks:


   ```bash
   sha256sum -c SHA256SUMS
   kicad-cli pcb drc --severity-error kicad/ice40_bga_node.kicad_pcb
   kicad-cli sch erc kicad/ice40_bga_node.kicad_sch
   synth validate ice40_bga_node.synth
   ```

4. Read the results. The first command verifies every file below `SHA256SUMS`. The DRC reads `kicad/ice40_bga_node.kicad_dru` because it sits beside the board.

Note: The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, 2026-11-10, on the package in `fab/`.

- The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review.
- The independent PCB expert sign-off required by #40 is open.
- The self-review covers, each verified and approved by JeffMboya, the items in the table below.
- The two JLCPCB runs (BOM matching of 2026-11-06 and the JLCDFM PCB DFM of 2026-11-10) are not in the table. They were made, their reports are not committed in this package, and no per-category disposition is recorded here.

| Item | Signed off |
|---|---|
| PCB and Gerber package | the Gerbers, drill, BOM and CPL of `fab/`, the exported KiCad board and the STEP model of `fab/` |
| Return paths | the return-path rule as applied by the export gate |
| ERC | 0 errors in `kicad-cli sch erc`; the warnings are library symbol and footprint-link warnings, and their number depends on the configured library tables |
| DRC | the 83 warnings of the KiCad DRC run with zone refill, 0 unconnected items |
| Placement | the placement on 60.9 x 48.5 mm, as pinned in `ice40_bga_node.placement.layout.toml` |
| Parts and values | the BOM identities of `fab/bom.csv`, including `Y1` (JYJE 3TJ480000FYLBC, `C2924620`) and `U1` (Lattice ICE40HX4K-BG121, `C1552775`) |

The parts this board adds to `registry/parts/` (`tlv70012_ddc`, `w25q128jvsiq_208mil`, `xtal_8mhz_3225_jyje_4p`) carry the author's review record (`reviewed_by`, `reviewed_at`, `datasheet_url`); `stm32f103c8` gains the `usb_dn`, `usb_dp` and `spi_cs` capability tags the board's checks read, with PA11 and PA12 typed as the differential USB pair; `ice40hx4k_bg121` types `VPP_FAST` (B10) as a passive pin that is not required, so it can stay unconnected; every other part keeps its registry record.

## USB pair: supported speed, impedance estimate and skew

The USB data pair runs from `J1` through the 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11` of `U5` (STM32F103C8). The delivered lengths are 114.7 mm (D+) and 175.5 mm (D-), a 60.8 mm difference, all on `F.Cu`. The pair is in the `USB` net class (0.12 mm tracks, 0.127 mm clearance, locked to `F.Cu`), declared 90 ohm differential.

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

94 ohm is inside 76.5 to 103.5 ohm. On the delivered board both legs stay on `F.Cu`, so the 94 ohm figure applies along their whole length. The formulas are the common IPC-2141 approximations; they carry several ohms of error, the solder-mask and the real etched width are not modelled, and no field solver or TDR measurement was run.

### Skew

Effective permittivity 0.475 x 4.4 + 0.67 = 2.76 gives a propagation delay of sqrt(2.76) / 299.8 mm/ns = 5.5 ps/mm. The 60.8 mm difference is about 334 ps, 8.4 % of the 4 ns minimum rise time and 0.4 % of the 83 ns bit time. The pair is left unmatched. It would matter at 480 Mbit/s, which the part does not run.
