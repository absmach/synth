# Qualification: iCE40HX4K BGA node with a USB-C power input (Class C, case 1)

**Status:** the board is routed and released.

- All 31 of 31 nets are routed. The export's routing report shows 0 open nets.
- KiCad 10.0.8 with zone refill reports 0 unconnected items on the committed board.
- The exported KiCad files, the fabrication package and the STEP model are committed ([`kicad/`](kicad/), [`fab/`](fab/), [`models/`](models/)).

The numbers and files below all come from one export, made through the Synth MCP server (see [`PROVENANCE.md`](PROVENANCE.md)). The report files are not committed.

## What was measured

All figures come from the files of the export this package was built from (KiCad 10.0.8).

| | |
|---|---|
| Outline | 59.9 x 45.7 mm, 63 parts (56 excluding mounting holes and test points) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"` (the JLCPCB tier that offers the 0.2 mm drill and via-in-pad on six layers), `requires via_in_pad`, 1.37 mm in the STEP model |
| Routing | 31 of 31 nets, 372 segments, 86 vias (0.35/0.2 mm: 19, 0.46/0.2 mm: 10, 0.6/0.3 mm: 57), 1003.8 mm of track, measured on the delivered board; the router's report (not committed) agrees |
| Plane | `+3V3` is poured on the inner layers (`plane` on its net class); ground on the outer layers; the 1.2 V core rail and the PLL supply are routed |
| BGA escape | `escape { method underpad }` on `U1`; the board has 19 vias at 0.35/0.2 mm, the size of a filled and capped via in a ball pad; see the BGA escape notes below the table |
| USB-C | `J1` sits on the left edge with its mating opening facing the edge (rotation 270, courtyard 0.05 mm from Edge.Cuts); `escape { off }` on `J1`, which is not a ball grid. `J1` is a power-only input: its data pins are not connected |
| DRC rules | The 19 vias at 0.35/0.2 mm are below the 0.46 mm and 0.13 mm floors of the board setup. The rules file `kicad/ice40_bga_node.kicad_dru` lowers the floor to 0.35 mm and 0.075 mm for vias of that size inside the courtyard of `U1`, the filled and capped via-in-pad process of `jlc-advanced`. KiCad loads the file when it sits beside the board. Without it, DRC reports `via_diameter` and `annular_width` errors on those vias |
| Zone fill | The committed board carries refilled zones (`kicad-cli pcb drc --refill-zones --save-board`), and the Gerbers are plotted from that fill. `kicad-cli pcb drc` without a refill reports 0 unconnected items on it |
| Warnings | 66 in the export's `kicad-cli` DRC report with zone refill (not committed): 63 `lib_footprint_issues` (silkscreen cut back from vias and pads by the exporter, #425), 2 `silk_overlap` and 1 `silk_over_copper`; 0 errors and 0 unconnected items with the rules file `kicad/ice40_bga_node.kicad_dru` beside the board |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy; see the engine verdict notes below the table |

BGA escape notes:

- A filled-and-capped via in a pad is the process JLCPCB's via-in-pad guide describes: land of the drill plus 0.15 mm.
- A dog-bone escape was tried and does not fit. An export with `escape { method channel }` and without `requires via_in_pad` stopped at the power-plane stage with `net +3V3: no legal stitching-via position beside the pad at (38.00, 36.40) mm on Top: every candidate is outside the board or within clearance of another pad or via`.
- The board therefore keeps `requires via_in_pad`.

Engine verdict notes:

- The engine reports no rule below the board floor (`rules_below_board_floor: none reported`).
- Its `claimed_total_vias: 29` and `claimed_via_in_pad: 36` count the vias its own routing and fanout steps placed.
- Synth's later steps add the plane, return and stitch vias and move vias out of the pads of components that do not declare an under-pad escape.
- The board's own counts are the ones this document uses: 86 vias, 19 of them 0.35/0.2 mm.
- The delivered board is judged by Synth's own checks: connectivity (31 of 31), the process floor and KiCad DRC. JLCPCB's via-in-pad page gives the land for that via.

## 3.3 V regulator thermal budget

`U3` (AMS1117-3.3, SOT-223) drops 5 V to 3.3 V.

- It dissipates `P = (5.0 - 3.3) x I`, and its junction sits at `Tj = TA + P x RthJA`.
- The datasheet limits the junction to 125 degC ([Advanced Monolithic Systems AMS1117 datasheet](https://files.waveshare.com/upload/b/b8/AMS1117-datasheet.pdf), a third-party print of the manufacturer's document): "Maximum junction temperature must not exceed 125 degC".
- For SOT-223 the datasheet gives `RthJA = 90 degC/W`, which "can vary from 46 degC/W to >90 degC/W depending on the size of the copper area". The board uses 90 degC/W, the top of that range.
- The Synth warning `W-SYNTH-THERMAL-001` comes from the registry part's declared 1200 mW rating at 90 degC/W (25 + 1.2 x 90 = 133 degC). It is the package limit, not this board's load.

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U4` TLV70012 1.2 V core regulator | 200 mA (its rated output current; a linear regulator draws its output current from the input) | TI TLV700 datasheet SBVS173, recommended operating conditions IOUT 0 to 200 mA |
| `U2` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Four LEDs with 1 k resistors | 1.3 mA | the design's own arithmetic in the source |
| Total | 226 mA | |

At 226 mA the regulator dissipates 1.7 V x 0.226 A = 0.38 W, a 35 degC rise.

- Tj is 60 degC at 25 degC ambient and 105 degC at 70 degC ambient, under the 125 degC limit.
- The 3.3 V of `J3` can feed an external load. With that load, Tj reaches 125 degC at 25 degC ambient at `(125 - 25) / 90 / 1.7 = 0.65 A` in total, and at 70 degC ambient at `(125 - 70) / 90 / 1.7 = 0.36 A`.
- So `J3` can supply about 0.43 A and 0.13 A respectively on top of the 226 mA.
- The input fuse `F1` holds 0.5 A, which bounds the sustained 5 V current and with it the regulator's dissipation at 0.85 W (Tj 101 degC at 25 degC ambient).

Note: The load currents are datasheet maxima added together, which no operating state reaches at once.

## ESD on the connector signals

The data pins of the USB-C receptacle are not connected, so they carry no signal to protect.

- The board adds two TPD1E10B06 bidirectional TVS diodes beside the receptacle, `D8` on `CC1` and `D9` on `CC2`. They have a 5.5 V stand-off and no rail connection.
- The CC diodes have no connection to `VBUS`, so a source that drives CC before VBUS does not back-power the rail through them. A 3 A charger attach test was not run.
- `D1` (a 6.0 V standoff TVS) protects `VBUS`.
- The 12-pin header `J3` carries FPGA I/O through 100 ohm series resistors and carries no protection device either.

Note: The board is a bench programming node, and no IEC 61000-4-2 level is claimed for it.

## Layout and fill

The placement is pinned in [`ice40_bga_node.placement.layout.toml`](ice40_bga_node.placement.layout.toml): the USB-C receptacle on the left edge with its opening facing out.

## Sourcing

[`fab/bom.csv`](fab/bom.csv) has 63 lines: 56 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number, and 7 board features (2 mounting holes, 5 test points) that are not parts and carry no identity. Several LCSC pages show different stock figures between snapshots and are noted in the table; the part pages were read in 2026-11 and the stock is not firm. `J2` and `J3` are hand-fit through-hole parts. The BOM-matching findings are `U1` below the 5-board quantity.

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

The committed export used KiCadRoutingTools `9399bfb2` and KiCad 10.0.8. It gave 31 of 31 nets, 372 segments, 86 vias and 1003.8 mm of track. Everything committed comes from that export. [`PROVENANCE.md`](PROVENANCE.md) describes how the board was made.

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

Note: The parts this board adds to `registry/parts/` (`tlv70012_ddc`, `w25q128jvsiq_208mil`, `tpd1e10b06`, `header_1x6_prog`) carry the author's review record (`reviewed_by`, `reviewed_at`, `datasheet_url`); `ice40hx4k_bg121` types `VPP_FAST` (B10) as a passive pin that is not required, so it can stay unconnected; every other part keeps its registry record.
