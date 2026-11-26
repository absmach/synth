# iCE40HX4K HDMI (DVI-style TMDS) node (Class C, case 3)

An iCE40HX4K FPGA in a 144-pin TQFP (0.5 mm pitch) drives an HDMI Type-A receptacle through four TMDS-style pairs with 270 ohm source resistors. The board also has level-shifted DDC lines, a hot-plug-detect divider, a fused +5 V HDMI pin, an SPI boot flash and four status LEDs. It is powered from a 5 V terminal and is 88.2 x 79.0 mm on six copper layers.

This is the Class C case 3 attempt for #330 under #40. The board declares impedance, pair spacing and skew for its TMDS pairs, and the delivered copper is measured against those declarations. Only the skew budget is met; spacing, coupling and impedance are not (see [#447](https://github.com/absmach/synth/issues/447), open). The interface is DVI-style and no HDMI compliance is claimed.

The prompt to reproduce the board is in [`PROMPT.md`](PROMPT.md), written from the finished board. The prompts the agent ran are in the history at `45b6f93d`.

## The board

### Parts

| Group         | Parts                                                                                                                                         |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Power entry   | `J1` JST B2B-XH-A 2-pin header, 5.0 to 5.25 V in                                                                                              |
| Power entry   | `F1` Bourns MF-MSMF110/16-2 PTC fuse: 1.1 A hold, 2.2 A trip, 16 V                                                                            |
| Power entry   | `D1` Nexperia PTVS6V0S1UR TVS: 6.0 V standoff, 6.67 V minimum breakdown                                                                       |
| Power entry   | `Q3` Vishay SI2301CDS P-MOSFET (20 V) as the reverse-polarity stage; gate to GND through `R40` (10 k); 5.1 V zener `D_GZ` from gate to source |
| Power entry   | `C1`, test point `TP1`                                                                                                                        |
| 3.3 V         | `U1` Diodes AP7361C-33E-13 LDO: 1 A, SOT-223, stable with ceramic capacitors                                                                  |
| 3.3 V         | `C2` to `C5` 10 uF                                                                                                                            |
| 3.3 V         | `R_SURGE` 1 ohm series resistor in front of `U1`; it keeps a `D1` clamp event off the regulator input                                         |
| 1.2 V core    | `U2` Texas Instruments TLV70012DDCR, TSOT-23-5, with `C6` and `C7`                                                                            |
| 1.2 V core    | PLL supplies `VCCPLL0` and `VCCPLL1` fed through ferrite beads `L1` and `L2`, with `C21` and `C22`                                            |
| FPGA          | `U3` Lattice iCE40HX4K-TQ144, 107 user I/O                                                                                                    |
| FPGA          | 100 nF decoupling `C10` to `C20`, `C24`, `C25` to `C27`                                                                                       |
| FPGA          | `X1` SiTime SiT8008 25 MHz oscillator with `C23`                                                                                              |
| Configuration | `U4` Winbond W25Q128JVSIQ, SOIC-8 208 mil, master SPI                                                                                         |
| Configuration | `R30` and `R31` pull up `CDONE` and `CRESET_B`; `R32` and `R33` pull up the flash write-protect and hold pins                                 |
| HDMI          | `J2` Amphenol 10029449-111RLF HDMI Type-A receptacle                                                                                          |
| HDMI          | `R1` to `R8` 270 ohm TMDS source resistors, in one column between the FPGA and the connector                                                  |
| HDMI          | `D3` onsemi SRV05-4 4-line ESD array on the DDC and HPD lines                                                                                 |
| HDMI          | `D8` and `D9` TI TPD4E05U06 4-channel ESD arrays (0.5 pF per channel) on the eight TMDS lines                                                 |
| HDMI          | `F2` Bourns MF-NSMF050-2 PTC fuse (0.5 A hold) on the +5 V pin                                                                                |
| HDMI          | DDC level shifters `Q1` and `Q2` (IRLML6244), `R9` and `R10` (2 k to +5 V), `R11` and `R12` (10 k to 3.3 V)                                   |
| HDMI          | HPD divider `R13` (10 k) and `R14` (20 k)                                                                                                     |
| Indicators    | `D4` and `D5` green, `D6` and `D7` red LEDs with 470 ohm resistors `R20` to `R23`                                                             |
| Indicators    | `D5` is driven through N-FET `Q4` (AO3400A, gate on `CDONE`), so the open-drain `CDONE` net sees only a gate                                  |
| Test points   | `TP1` to `TP7`: +5 V, +3.3 V, +1.2 V, GND, CDONE, CRESET_B, +5 V_HDMI                                                                         |
| Mechanical    | Two M2.5 mounting holes, `H1` and `H2`                                                                                                        |

### Layout

| Item                    | Value                                                                                                                                          |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Outline                 | 88.2 x 79.0 mm; 1.47 mm thick in the STEP model                                                                                                |
| Copper layers           | 6                                                                                                                                              |
| Stackup                 | JLCPCB `JLC06161H-3313`: prepreg 3313 0.0994 mm er 4.1, core 0.55 mm er 4.6, prepreg 2116 0.1088 mm er 4.16; outer copper 35 um, inner 15.2 um |
| Parts in the BOM        | 83 (74 parts plus 2 mounting holes and 7 test points, which are board features)                                                                |
| Parts placed in the CPL | 74                                                                                                                                             |
| Nets                    | 49                                                                                                                                             |
| Track segments          | 822                                                                                                                                            |
| Vias                    | 148: 53 at 0.46/0.2 mm, 95 at 0.6/0.3 mm                                                                                                       |
| Track length            | 1898.0 mm                                                                                                                                      |
| Zones and planes        | GND zone on each of the six copper layers; two keepout areas on all layers for the mounting screw heads (no tracks, vias or pour)              |
| Fab package             | `fab/gerbers.zip`, `fab/bom.csv`, `fab/cpl.csv`                                                                                                |
| 3D models               | `models/` holds the STEP body of every part the board references and the board model `ice40_hdmi_node.step`. The board file reaches them by the relative path `${KIPRJMOD}/../models/`. Mounting holes and fiducials have no body. A part that KiCad's library has no model for is a simplified box of its envelope. |
| Schematic               | `images/schematic.pdf`: vector pages, one per sheet, plotted by `kicad-cli sch export pdf`. |

### Checks

| Check                                  | Result                               | Notes                                                                                                                                       |
| -------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| KiCad DRC (10.0.8, --refill-zones)     | 0 errors, 0 unconnected, 3 warnings  | 3 `track_dangling`                                                                                                                          |
| KiCad ERC (10.0.8)                     | 0 errors, 0 unconnected, 25 warnings | 18 `lib_symbol_issues`, 5 `lib_symbol_mismatch`, 2 `pin_to_pin`; the 2 `pin_to_pin` are on `CBSEL0` and `CBSEL1`, tied to ground on purpose |
| Routing (KiCadRoutingTools `9399bfb2`) | 49 of 49 nets routed                 | Measured on the board file                                                                                                                  |
| Gerbers                                | Plotted from refilled zones          | Zones were refilled before the package was written                                                                                          |
| Via in pad                             | Required                             | The source declares `requires via_in_pad`. An export without it leaves `+3V3` open at `U3` pin 131 and does not release (#512).             |

### Design figures

| Area             | Item                 | Value                                                                                                                                                    | Basis                                               |
| ---------------- | -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| FPGA             | User I/O             | 107 on the iCE40HX4K-TQ144                                                                                                                               | Lattice DS1040                                      |
| FPGA             | Part model           | `registry/parts/fpga/ice40hx4k_tq144.synth.toml` is built from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of DS1040            | Lattice DS1040                                      |
| Signal integrity | Lane pin assignment  | Four adjacent pin pairs on the FPGA's left side (1-2, 3-4, 7-8, 9-10), in the connector's lane order, so no pair crosses another                         | Measured on the board file                          |
| Signal integrity | Leg definition       | Each leg is the FPGA-to-resistor net plus the resistor-to-connector net; gaps are plan-view, centre to centre, regardless of layer                       | Measured on the board file                          |
| Signal integrity | `D0`                 | P 42.3 mm, N 47.2 mm, skew 4.86 mm, vias 7 / 6, gap min / median 0.09 / 1.44 mm                                                                          | Measured on the board file                          |
| Signal integrity | `D1`                 | P 41.6 mm, N 44.2 mm, skew 2.61 mm, vias 6 / 6, gap min / median 0.05 / 1.48 mm                                                                          | Measured on the board file                          |
| Signal integrity | `D2`                 | P 34.5 mm, N 35.3 mm, skew 0.83 mm, vias 4 / 4, gap min / median 0.07 / 1.40 mm                                                                          | Measured on the board file                          |
| Signal integrity | `CK`                 | P 46.6 mm, N 50.2 mm, skew 3.64 mm, vias 4 / 5, gap min / median 0.04 / 2.05 mm                                                                          | Measured on the board file                          |
| Signal integrity | Minimum gaps         | Gaps below the 0.127 mm track width mean the legs cross in plan view on different layers; they are not a copper spacing                                  | Measured on the board file                          |
| Signal integrity | Skew measure         | Sums of track segments; the DRC's own length-matching figures are not used                                                                               | Measured on the board file                          |
| Signal integrity | Skew budget          | 25 mm declared, met on all four lanes                                                                                                                    | Measured on the board file                          |
| Signal integrity | Skew limit           | 0.15 Tbit within a pair; at 25 MHz Tbit is 4.0 ns, so 0.6 ns, about 85 mm at 7.0 ps/mm; the 25 mm budget is 29 % of it                                   | TI SLVA747 (secondary source); calculated           |
| Signal integrity | Inter-pair mismatch  | Mean pair lengths differ by up to 13.54 mm; no inter-pair budget is declared                                                                             | Measured on the board file                          |
| Signal integrity | Impedance estimate   | 54.5 ohm single-ended, 96.7 ohm differential at 0.127 mm width and 0.15 mm gap, outer-layer microstrip only; it does not describe inner-layer runs       | Calculated (closed-form microstrip)                 |
| Signal integrity | Drive levels         | Each line is LVCMOS33 through 270 ohm into the sink's 50 ohm to 3.3 V: 2.78 V to 3.3 V, 0.52 V single-ended; FPGA pin output impedance not included      | Calculated                                          |
| Signal integrity | Edge rate            | 20 to 80 % edges of at least 0.728 ns at the fast corner; TMDS and source classes declare `edge_rate medium`, a 6 mm window for `W-SYNTH-DRC-016` (#445) | Lattice IBIS model `ice40.ibs` rev 2.5              |
| Signal integrity | Return-via distances | `D0` 0.63 to 6.01 mm, `D1` 0.70 to 4.66 mm, `D2` 0.70 to 3.89 mm, `CK` 0.63 to 3.11 mm; the 6.01 mm via of `D0` is just outside the 6 mm window          | Measured on the board file                          |
| Signal integrity | HPD divider          | 5 V x 20 / (10 + 20) = 3.33 V at the FPGA                                                                                                                | Calculated                                          |
| Power            | Input range          | `J1` takes 5.0 to 5.25 V; protection is sized for a 5.5 V maximum; a 4.75 V low-end check shows why 5.0 V is the minimum                                 | Calculated                                          |
| Power            | `D1` standoff        | 6.0 V standoff above the 5.5 V maximum input                                                                                                             | Nexperia PTVS6V0S1UR datasheet                      |
| Power            | HDMI pin 18 drop     | Worst case 0.065 V in `F1`, 0.034 V in `Q3`, 0.039 V in `F2`, 0.138 V in all                                                                             | Calculated                                          |
| Power            | HDMI pin 18 voltage  | A 5.00 V input gives 4.86 V against the 4.8 V minimum; pin 18 needs at least 4.94 V at `J1`                                                              | Calculated                                          |
| Power            | Regulator ratings    | `RthJA` 110 degC/W (SOT-223); 6.0 V recommended, 6.5 V absolute maximum input; 360 mV typical dropout at 1 A                                             | Diodes DS37274 Rev. 5-2                             |
| Power            | Dropout margin       | At the 4.40 V worst-case regulator input, `U1` has 1.10 V to spare against 0.36 V typical dropout at 1 A                                                 | Diodes DS37274 Rev. 5-2; calculated                 |
| Thermal          | 3.3 V load           | `U2` 200 mA, `U4` 25 mA, LEDs 28 mA: 253 mA                                                                                                              | TI SBVS173; Winbond W25Q128JV datasheet; calculated |
| Thermal          | `U1` junction        | 0.47 W in `U1`; Tj 77 degC at 25 degC and 122 degC at 70 degC ambient                                                                                    | Diodes DS37274 Rev. 5-2; calculated                 |
| Protection       | Surge clamp          | `D1` clamps at up to 10.3 V                                                                                                                              | Nexperia PTVS6V0S1UR datasheet                      |
| Protection       | Surge current        | `R_SURGE` limits the current into `C2` and `C3` (20 uF) to 5.8 A; a 60 ns event raises the regulator input by about 0.02 V                               | Calculated                                          |
| Protection       | `Q3` gate            | `D_GZ` holds the `Q3` gate-source voltage under 5.4 V against its 8 V rating                                                                             | Vishay SI2301CDS datasheet; calculated              |
| Assembly         | Via in pad           | The TQFP-144 escape does not need it; the plane stage does until #512 is fixed. A few vias sit in small-part and test-point pads                         | Measured on the board file                          |

### Sourcing

| Item                        | Status                 | Notes                                         |
| --------------------------- | ---------------------- | --------------------------------------------- |
| 74 placed parts             | sourced                | Distributor and order number in `fab/bom.csv` |
| `H1`, `H2`, `TP1` to `TP7`  | board feature, no part | Not placed                                    |
| JLCPCB DFM and BOM matching | not run                | No run is recorded for the package in `fab/`  |

## Not verified by Synth

| Area                   | What is not verified                                                                                                                                                               |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HDMI compliance        | None is claimed. The interface is DVI-style, with LVCMOS33 outputs through series resistors, because the iCE40 cannot source TMDS levels.                                          |
| Coupled pairs          | The pairs are routed as independent nets on different layers, so they are not a coupled differential interface.                                                                    |
| Impedance              | The differential impedance is not established. Only an analytic estimate for an outer-layer microstrip is given.                                                                   |
| Pair spacing           | The median gap between the legs, 1.40 to 2.05 mm, is wider than the declared 0.15 mm coupling gap, so the legs are not coupled.                                                    |
| Sustained over-voltage | While `D1` holds the rail near 7.0 V until `F1` opens, the regulator input is about 6.75 V, 0.25 V over its 6.5 V rating.                                                          |
| `R_SURGE` pulse rating | The 5.8 A, 60 ns event is not checked against the resistor's pulse-load curve.                                                                                                     |
| Back-drive             | A powered sink back-powers the FPGA banks through the 270 ohm resistors when the board is off. No isolation switch is fitted. Do not connect a powered sink to an unpowered board. |
| FPGA I/O current       | The thermal budget leaves about 0.02 A for the FPGA's 3.3 V I/O at 70 degC ambient. That current depends on the FPGA design and is not counted.                                    |
| Test-point probing     | The effect of filled vias in the `TP2` and `TP5` pads on probing is not assessed.                                                                                                  |
| Fabrication checks     | No JLCPCB DFM or BOM-matching run is recorded for the package in `fab/`.                                                                                                           |
| Independent sign-off   | The independent PCB review required by #40 is still open.                                                                                                                          |
