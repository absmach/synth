# Qualification: iCE40HX4K HDMI (DVI-style TMDS) node (Class C, case 3)

**Status:** the board is routed and released. The export's routing report shows 49 of 49 nets connected and 0 open.

- The exported KiCad files, the fabrication package and the STEP model are committed in [`kicad/`](kicad/), [`fab/`](fab/) and [`models/`](models/).
- The router's report, the pair measurements and the layout metrics in this document come from the author's run of the export this package was built from. No report or measurement file is committed.
- The delivered pairs stay within the declared 25 mm intra-pair skew budget. See the TMDS pair measurements table.
- The pairs are not a coupled differential interface. The router routes P and N as independent nets on different layers, and the median gap between the legs is 1.40 to 2.05 mm against the 0.15 mm class clearance.
- Spacing, coupling and differential impedance are not met. No HDMI compliance is claimed.

## What was measured

All figures come from the files of the export this package was built from (KiCad 10.0.8).

| | |
|---|---|
| Outline | 88.2 x 79.0 mm, 83 parts (74 excluding mounting holes and test points) |
| Stackup | six copper layers, `manufacturer "jlc-advanced"`, JLCPCB's `JLC06161H-3313` stackup as declared in the source (prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um; [JLCPCB impedance tool](https://jlcpcb.com/impedance)), 1.47 mm in the STEP model |
| Routing | 49 of 49 nets, 822 segments, 148 vias (0.46/0.2 mm: 53, 0.6/0.3 mm: 95), 1898.0 mm of track, measured on the delivered board; the router's report (from the author's run, not committed in this package) agrees |
| Warnings | 86 `kicad-cli` warnings in the export's DRC report (not committed): 83 `lib_footprint_issues` (silkscreen cut back from vias and pads by the exporter, #425, since closed; the footprint comparison shows no difference in pads, courtyard or fabrication layers) and 3 `track_dangling` |
| Zone fill | The committed board carries refilled zones (`kicad-cli pcb drc --refill-zones --save-board`), and the Gerbers are plotted from that fill. `kicad-cli pcb drc` without a refill reports 0 unconnected items on it |
| ERC with the library tables | 0 errors, 4 warnings (from the author's run; the ERC report is not committed): 2 `lib_symbol_mismatch` on `IRLML6244` (`Q1`, `Q2`; the exported symbol differs from KiCad's `Transistor_FET` copy, #428, since closed) and 2 `pin_to_pin` on the configuration-select pins `CBSEL0` and `CBSEL1`, tied to ground on purpose |
| Engine verdict | `engine_policy_verdict: rejected`: the routing engine runs its strict-size policy, which rejects any track or via narrower than the size its net class declares. The engine's counters record 123 vias claimed and 66 of them in pads (`claimed_via_in_pad`), and "rules below board floor: none reported"; the delivered board's own counts, 148 vias in all, are the ones this document uses |

### TMDS pair measurements (not a coupled differential interface)

Each TMDS lane is two nets per leg: the FPGA pin to the 270 ohm source resistor (`Dn_P_SRC`, `Dn_N_SRC`) and the resistor to the connector (`HDMI_Dn_P`, `HDMI_Dn_N`). Lengths are the sum of the track segments of both nets of a leg, from the pair measurements, which are the author's measurement of the delivered tracks; the measurement file is not committed in this package.

| Lane | P: source + connector side | N: source + connector side | Intra-pair skew | Declared budget | Layer changes (P / N vias) | Pair gap as routed (min / median, centre to centre) |
|---|---|---|---|---|---|---|
| `D0` | 11.2 + 31.1 = 42.3 mm | 10.5 + 36.6 = 47.2 mm | 4.86 mm | 25 mm | 7 / 6 | 0.09 / 1.44 mm |
| `D1` | 10.6 + 31.0 = 41.6 mm | 10.9 + 33.3 = 44.2 mm | 2.61 mm | 25 mm | 6 / 6 | 0.05 / 1.48 mm |
| `D2` | 11.9 + 22.6 = 34.5 mm | 7.5 + 27.8 = 35.3 mm | 0.83 mm | 25 mm | 4 / 4 | 0.07 / 1.40 mm |
| `CK` | 7.5 + 39.1 = 46.6 mm | 6.9 + 43.4 = 50.2 mm | 3.64 mm | 25 mm | 4 / 5 | 0.04 / 2.05 mm |

The gap is the plan-view distance between the P and N tracks, measured regardless of layer. The minimum gaps of 0.04 to 0.09 mm are below the 0.127 mm track width, so the legs cross or overlap in plan view on different layers. They are not a copper spacing.

- **Inter-pair mismatch:** the mean pair lengths are 44.7 (`D0`), 42.9 (`D1`), 34.9 (`D2`) and 48.4 mm (`CK`), a mismatch of 13.54 mm; no inter-pair budget is declared in the source.
- **Skew budget.**
  - Every `diff_pair` declares `max_skew 25mm`.
  - The TMDS transmitter rule is 0.15 Tbit between the two halves of a pair, with Tbit = 0.1 x the pixel period. The sources are secondary: [Texas Instruments SLVA747](https://ti.com/lit/pdf/SLVA747) and the layout notes of Analog Devices and Chrontel, not the HDMI 1.4 text.
  - The 25 MHz oscillator gives a 40 ns pixel period (VGA timing, 252 Mbit/s per lane). Tbit is 4.0 ns and 0.15 Tbit is 0.6 ns, which is 85 mm at 7.0 ps/mm (stripline, er 4.4). The 25 mm budget is 0.175 ns, 29 % of that limit.
  - The delivered skew is 0.83 mm at best (`D2`) and 4.86 mm at worst (`D0`), by the sums of segments above.
  - The export reports no blocking signal-integrity finding (`signal_integrity_blocking` is 0).
  - The sums of segments are the delivered pair measure this document uses. The DRC's length-matching figures are not committed and are not quoted.
  - The router wrapper never calls the KiCadRoutingTools pair router, so P and N are routed as independent nets ([#447](https://github.com/absmach/synth/issues/447)). The export gate did not carry the class skew budgets through in the revision used ([#147](https://github.com/absmach/synth/issues/147), since closed).
  - The pairs are therefore not coupled. The median gap between the two legs is 1.40 to 2.05 mm, not the 0.15 mm class clearance.
- **Width and impedance.**
  - The class declares 0.127 mm tracks (the board's minimum track width) and a 0.15 mm clearance.
  - The closed-form microstrip estimate gives 96.7 ohm differential on the declared stackup. It is an estimate, not a field solve.
    - Z0 = 87/sqrt(er+1.41) x ln(5.98 h / (0.8 w + t)) = 87/2.347 x ln(0.5944/0.1366) = 54.5 ohm, at h = 0.0994 mm, er = 4.1, t = 35 um.
    - Zdiff = 2 Z0 (1 - 0.48 exp(-0.96 s / h)) = 96.7 ohm, at s = 0.15 mm.
  - The delivered TMDS tracks are 0.127 to 0.13 mm. The single-ended impedance is about 54 ohm.
  - The differential impedance is not established, because the pairs are not coupled.
  - The router put the halves of the pairs on different layers. `HDMI_D1_P` runs 13.4 mm on `In1.Cu` and `HDMI_D1_N` 23.4 mm on `In4.Cu`. `HDMI_D2_P` runs 18.7 mm on `In2.Cu` and `HDMI_D2_N` 24.6 mm on `F.Cu`.
  - The estimate is for an outer-layer microstrip over the 0.0994 mm prepreg. It does not describe those runs, and no impedance is claimed for them.
- **Drive levels.** With the iCE40 output at 0 V behind 270 ohm into the sink's 50 ohm pull-up to 3.3 V, the line sits at 3.3 V x 270 / (270 + 50) = 2.78 V; at 3.3 V it sits at 3.3 V, a swing of 0.52 V single-ended. The output impedance of the iCE40 pin is not included.

### Return paths and the edge rate of the TMDS nets

The rule `W-SYNTH-DRC-016` asks for a ground via within a window of the thickest dielectric of a layer-change via on a fast net.

- The rule blocks the release for a fast net. A declared `diff_pair` and a differential pin type are inferred fast.
- The TMDS and source classes declare `edge_rate medium`. The nets are driven only by the iCE40 through a 270 ohm resistor.
- Lattice's IBIS model `ice40.ibs` rev 2.5 gives 20 to 80 % rise and fall times of at least 0.976 ns (banks 0 to 2, `lvc330io`) and 0.728 ns (bank 3, `lvc330_b3io`) at the fast corner. No edge on these nets is faster than 0.3 ns.
- The `edge_rate medium` tier ([#445](https://github.com/absmach/synth/issues/445), since closed) widens the window to 6 mm.
- The Synth commit named in [`PROVENANCE.md`](PROVENANCE.md) lets a declared `medium` replace the inference from a `diff_pair` or a differential pin. A clock, RF or analog pin, a crystal or a switching node still proves a fast edge.

Measured on the delivered board, centre to centre, the TMDS layer-change vias are within these distances of a ground via:

| Lane | Distance to a ground via |
|---|---|
| `D0` | 0.63 to 6.01 mm |
| `D1` | 0.70 to 4.66 mm |
| `D2` | 0.70 to 3.89 mm |
| `CK` | 0.63 to 3.11 mm |

The 6.01 mm via of `D0` is just outside the 6 mm window.

## 3.3 V regulator thermal budget

`U1` (AP7361C-33E, SOT-223) drops at most 5.15 V to 3.3 V. The input is 5.5 V less the drops of `F1`, `Q3` and `R_SURGE`.

- Dissipation is `P = (5.15 - 3.3) x I`.
- The junction sits at `Tj = TA + P x RthJA`, held to 125 degC here.
- Diodes AP7361C datasheet DS37274 Rev. 5-2 gives `RthJA = 110 degC/W` for SOT223, 360 mV typical dropout at 1 A for the 3.3 V version, 1 A output, input 2.2 to 6.0 V recommended and 6.5 V absolute maximum.
- `W-SYNTH-THERMAL-001` is not reported for `U1`.

| Load on `+3V3` | Current used | Source |
|---|---|---|
| `U2` TLV70012 1.2 V core regulator | 200 mA (its rated output current; a linear regulator draws its output current from the input) | TI TLV700 datasheet SBVS173, recommended operating conditions IOUT 0 to 200 mA |
| `U4` W25Q128JV | 25 mA (page program and erase, `ICC5` and `ICC7` maximum) | Winbond W25Q128JV datasheet, DC Electrical Characteristics |
| Four LEDs, 470 ohm | 28 mA (3.3 V / 470 ohm each, which ignores the LED forward drop) | the design's own arithmetic |
| Total | 253 mA | |

At 253 mA the regulator dissipates 1.85 V x 0.253 A = 0.47 W, a 51 degC rise.

- Tj is 77 degC at 25 degC ambient and 122 degC at 70 degC ambient, under 125 degC.
- Tj reaches 125 degC at (125 - 25) / 110 / 1.85 = 0.49 A in total at 25 degC ambient, and at (125 - 70) / 110 / 1.85 = 0.27 A at 70 degC ambient.
- At 70 degC that leaves about 0.02 A for the FPGA's 3.3 V I/O current. That current depends on the design and is not counted above.
- The load currents are datasheet maxima added together. No operating state reaches them at once.
- Dropout is 360 mV typical at 1 A. At the 4.40 V worst-case input (4.65 V rail less `R_SURGE`) the output stays in regulation with 1.10 V to spare.
- The HDMI 5 V output is fed from the 5 V rail through `F2`, not through `U1` or `R_SURGE`.

## Input surge

`D1` (Nexperia PTVS6V0S1UR) has a 6.0 V standoff above the 5.5 V maximum input, a 6.67 V minimum breakdown and a 10.3 V clamp at 38.8 A.

- `U1` is rated 6.5 V absolute maximum at its input. Every low-dropout regulator this board could use (AP7361C, AP2114, AP2112K, LP5907) is rated 6.5 V or less, below the least breakdown of any TVS with a standoff above 5.5 V.
- `R_SURGE`, 1 ohm, therefore sits between the +5 V rail and `U1`, with `C2` and `C3` behind it. A short clamp event is held off the regulator, and a sustained over-voltage is limited by `F1` opening. The arithmetic and the one remaining gap are in the requirement check below.
- `Q3` is protected by `D_GZ`, a 5.1 V zener from its gate to its source. It holds the gate-source voltage under the SI2301CDS's 8 V rating while the source follows a surge to 10.3 V behind the 10 k gate resistor `R40`.

### Requirement check: regulator and input protection

`R_SURGE`, 1 ohm (Yageo RC1206FR-071RL), sits between the +5 V rail (`Q3`, `D1`, `C1`, and the `F2` branch to HDMI pin 18) and the regulator input, with `C2` and `C3` (20 uF) behind it. It is not in the HDMI path, so the pin-18 arithmetic below is unchanged.

| Requirement | Our choice | Arithmetic | Result |
|---|---|---|---|
| The regulator input stays inside its recommended range in operation | `U1` AP7361C-33E, input 2.2 to 6.0 V | the input is at most 5.5 V less `F1`, `Q3` (0.10 V at 0.31 A) and `R_SURGE` (0.25 V), 5.15 V, 0.85 V under 6.0 V | Met |
| Enough headroom for the dropout | 360 mV typical at 1 A (Diodes DS37274) | the worst-case rail is 4.65 V (4.75 V less 0.065 V in `F1` and 0.034 V in `Q3` at 0.31 A); less `R_SURGE` 0.25 V the input is 4.40 V; 4.40 V - 3.3 V = 1.10 V against 0.36 V at 1 A | Met |
| The regulator carries the load and sheds its dissipation | 1 A, SOT-223, `RthJA` 110 C/W | (5.15 V - 3.3 V) x 0.253 A = 0.47 W gives 77 C at 25 C ambient and 122 C at 70 C ambient, under 125 C; the total at which Tj reaches 125 C is 0.49 A at 25 C and 0.27 A at 70 C | Met |
| `R_SURGE` carries the load | 1 ohm, 250 mW | 0.253 A squared x 1 ohm = 0.064 W | Met |
| HDMI pin 18 stays at 4.8 V or more | `Q3` P-MOSFET stage instead of the Schottky | worst-case drops 0.138 V (`F1`, `Q3`, `F2`), so 4.94 V is needed at `J1` | Met when `J1` carries 4.94 V or more, which the README states |
| `Q3` survives the surge on its gate | `D_GZ` 5.1 V zener from gate to source, `R40` 10 k | with the source at the 10.3 V clamp the gate follows at the source less 4.8 to 5.4 V, so the gate-source voltage stays under 5.4 V against the 8 V rating, with 0.52 mA (2.7 mW) in `D_GZ` | Met |
| The TVS does not conduct in operation | `D1` PTVS6V0S1UR, 6.0 V standoff, 6.67 V minimum breakdown | 5.5 V maximum input is 0.5 V under the standoff | Met |
| The regulator survives a short clamp event | `U1` absolute maximum input 6.5 V | `D1` clamps at up to 10.3 V at 38.8 A; the current through `R_SURGE` is at most (10.3 V - 4.5 V) / 1 ohm = 5.8 A, so a 60 ns event carries 0.35 uC, which raises 20 uF by 0.02 V and holds the regulator input near 5 V; the energy in the resistor is about 2 uJ (its pulse rating was not read) | Met for events of 60 ns |
| The regulator survives a sustained over-voltage | `U1` absolute maximum input 6.5 V | `D1` holds the +5 V rail at about 7.0 V (typical breakdown) until `F1` opens; the regulator input is then 7.0 V - 0.25 V = 6.75 V, 0.25 V over its rating | Not met until `F1` opens |

The sustained over-voltage row is the one gap, for the reasons given under "Input surge": every low-dropout regulator considered is rated 6.5 V or less, and the 15 V and 20 V parts (AMS1117-3.3, NCP1117ST33T3G) need an ESR-stabilised output capacitor and have a higher `RthJA` in the SOT-223.

## +5 V at the HDMI connector (pin 18)

An HDMI source must present 4.8 to 5.3 V on pin 18. The path is `J1` to `F1` to `Q3` to the +5 V rail to `F2` to pin 18. Worst-case drops at the resistances the datasheets give, with 55 mA on pin 18 (the HDMI source minimum) and 0.253 A drawn by `U1` ahead of `F2`, so 0.308 A through `F1` and `Q3`:

| Part | Resistance | Current | Drop |
|---|---|---|---|
| `F1` Bourns MF-MSMF110/16 | 0.21 ohm (R1Max) | 0.308 A | 0.065 V |
| `Q3` Vishay SI2301CDS | 0.112 ohm (maximum at VGS -4.5 V, from a mirror of the Vishay datasheet) | 0.308 A | 0.034 V |
| `F2` Bourns MF-NSMF050 | 0.70 ohm (R1Max) | 0.055 A | 0.039 V |
| Total | | | 0.138 V |

With a 5.00 V input pin 18 sits at 4.86 V, inside the 4.8 V minimum. With a 4.75 V input (5 V less 5 %) it sits at 4.61 V, below it: the board needs at least 4.94 V at `J1` to meet 4.8 V on pin 18. The previous Schottky stage dropped about 0.45 V; the P-MOSFET stage drops 0.034 V.

## Back-drive

With the board unpowered and a powered sink attached, the sink's 50 ohm pull-ups to 3.3 V reach the FPGA's I/O banks through the 270 ohm source resistors and can back-power them. The board has no isolation switch for this; do not connect a powered sink to an unpowered board.

## Input range and vias in pads

Input range:

- `J1` supports 5.0 to 5.25 V. HDMI pin 18 needs at least 4.94 V at `J1`.
- The protection is designed for a 5.5 V maximum input: the TVS standoff and the regulator check use it.
- 4.75 V (5 V less 5 %) is a low-end check that shows why 5.0 V is the minimum.

Vias in pads:

- The source declares `requires via_in_pad`. It lets the router place a filled, capped via inside an SMD pad.
- The board has a few such vias. None is under a dense package, and the TQFP-144 does not need it.
- The declaration was not tested against an export without it.
- Whether a via in the pads of `TP2` or `TP5` affects probing is not assessed.

## Layout and fill

The placement is pinned in [`ice40_hdmi_node.placement.layout.toml`](ice40_hdmi_node.placement.layout.toml): the HDMI receptacle on the left edge, the eight source resistors in one column beside the FPGA's left pins, the power entry on the right edge. The placement is the original one scaled by 1.11 about its centroid, to leave room for the routing.

## Sourcing

[`fab/bom.csv`](fab/bom.csv) has 83 lines: 74 parts, each with a manufacturer, MPN, package, rating, datasheet or page, distributor and order number, and 9 board features (2 mounting holes, 7 test points) that are not parts and carry no identity. `J1` is a hand-fit through-hole part. Two lines carry a sourcing note: the 2 k resistors (`R9`, `R10`, UNI-ROYAL 0603WAF2001T5E, LCSC `C22975`) showed 0 in stock on the LCSC page read, and the onsemi SRV05-4MR6T1G (`D3`) has no onsemi listing at LCSC, so it is sourced from DigiKey. The part pages were read in 2026-11 and the stock is not firm.

## Fabrication package

[`fab/`](fab/): `gerbers.zip` (18 files: 13 Gerber layers, including the four inner layers, two drill files, two drill map PDFs and one Gerber job file), `bom.csv` (the orderable BOM) and `cpl.csv` (the placement file); the STEP model is in [`models/`](models/); every file is listed in [`SHA256SUMS`](SHA256SUMS). Models the KiCad library lacks are written from datasheet dimensions: the test points (1 oz copper discs), the fuse `F1` (the Bourns datasheet's mid-range 4.55 x 3.24 x 0.70 mm) and the oscillator `X1` (the SiT8008's 5.0 x 3.2 x 0.75 mm package, SiT8008B datasheet, page 11 of 18).

## Reproducibility

Export the source and sidecar in this directory through the Synth MCP server to reproduce the board.

1. Start the server with `synth mcp --stdio`.
2. Call the `synth_export` tool on `ice40_hdmi_node.synth` and its placement sidecar, with an empty output directory and the router included.
3. Compare the result with the committed files.

The committed export used KiCadRoutingTools `9399bfb2` and KiCad 10.0.8. It gave 49 of 49 nets, 822 segments, 148 vias and 1898.0 mm of track. Everything committed comes from that export. [`PROVENANCE.md`](PROVENANCE.md) describes how the board was made.

## Images

[`images/`](images/) holds `top.png`, `bottom.png` and `angled.png` (3D renders of the board) and `schematic.pdf` (every schematic sheet on one page).

## How to check

Follow these steps from this directory.

1. Install KiCad 10 and configure its footprint and symbol library tables.
2. Verify every file listed in `SHA256SUMS`:

   ```bash
   sha256sum -c SHA256SUMS
   ```

3. Run the DRC on the exported board:

   ```bash
   kicad-cli pcb drc --severity-error kicad/ice40_hdmi_node.kicad_pcb
   ```

4. Validate the source:

   ```bash
   synth validate ice40_hdmi_node.synth
   ```

The parts this board adds to the registry are in `registry/parts/` of the repository.

## Human review

### PCB review sign-off

PCB review sign-off required by #40: JeffMboya, on the package in `fab/`. The sign-off is JeffMboya's own. Jeff Mboya is the author of this pull request, so the table below is the author's self-review. The independent PCB expert sign-off required by #40 is open, and no JLCPCB DFM or BOM-matching run is recorded for the package in `fab/`. No item below is signed off yet: the table is the checklist for the review, and its cells stay blank until each item is verified and approved.

| Item | Signed off |
|---|---|
| PCB and Gerber package | |
| Differential interface | |
| Return paths | |
| ERC | |
| DRC | |
| JLCPCB reports | |
| Placement | |
| Parts and values | |

The parts this board adds to `registry/parts/` (`ice40hx4k_tq144`, `w25q128jvsiq_208mil`, `tlv70012_ddc`) carry the author's review record (`reviewed_by`, `reviewed_at`, `datasheet_url`); the pull request also edits four existing registry parts (`hdmi_a_receptacle`, `polyfuse_1r1a`, `polyfuse_500ma`, `n_mosfet_irlml6244`), whose records are changed by this pull request, and every other registry part is untouched.
