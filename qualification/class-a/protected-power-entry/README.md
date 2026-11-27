# Protected 12 V power entry (Class A, case 3)

A 12 V power-entry board with no microcontroller. The input passes a polyfuse, a TVS diode and a reverse-polarity P-MOSFET. An MP2307 buck makes 5.0 V, a TPS22918 load switch makes a switched 5 V output, and an AMS1117-3.3 makes 3.3 V. It has two copper layers, measures 114.9 x 76.0 mm and has 84 BOM lines.

It answers whether `synth mcp`, driven by an agent from a prompt, makes a routed power stage with a protected input, net classes sized for the current and connectors on the board edges. The claim covers the circuit, the part choices and the routed board in `kicad/`. No board has been built or powered, so no electrical behaviour is measured.

The prompt to reproduce the board is in [`PROMPT.md`](PROMPT.md), written from the finished board. The prompts the agent ran are in the history at `972cc597`.

## The board

### Parts

| Group                     | Parts                                                                                                                                                                                                                                                                                                     |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source                    | [`protected_power_entry.synth`](protected_power_entry.synth) with its placement sidecar [`protected_power_entry.placement.layout.toml`](protected_power_entry.placement.layout.toml)                                                                                                                      |
| 12 V input and protection | `J1` JST S2B-XH-A-1 (pin 1 +12 V, pin 2 GND); `F1` Littelfuse 1812L150/16DR (1.5 A hold, 3 A trip, 16 V); `D1` Littelfuse SMAJ13A TVS on the fused node ahead of `Q1`; `Q1` AO3401A P-MOSFET for reverse polarity, gate held within 5.1 V of the source by the zener `D9` and `R1` 47 k; `C1` 100 uF 25 V |
| 5 V buck                  | `U1` MPS MP2307DN (3 A, 0.925 V reference); `L1` Bourns SRN6045TA-100M 10 uH; input `C2`, `C3` 10 uF 25 V and `C4` 100 nF; boot `C5`, soft-start `C6`, compensation `R2` and `C7`; feedback `R3` 44.2 k and `R4` 10 k; enable divider `R5` 100 k and `R6` 33 k; output `C8`, `C9` 22 uF and `C10` 100 uF  |
| Load switch               | `U3` TI TPS22918 with `C12`, `C13` (2.2 nF slew), `C14`; `R7` pull-down, `R8` and zener `D10` on `LOAD_EN`                                                                                                                                                                                                |
| 3.3 V                     | `U2` AMS1117-3.3 (SOT-223) from the 5 V rail, with `C15` to `C18`                                                                                                                                                                                                                                         |
| Outputs                   | `J3` +5 V through `F2`, `J4` +5V_SW through `F3`, `J5` +3V3; all JST XH, pin 2 GND                                                                                                                                                                                                                        |
| Control header            | `J2` 1x8: 1 +3V3_REF, 2 GND, 3 LOAD_EN, 4 GND, 5 SENSE_12V, 6 SENSE_5V, 7 SENSE_3V3, 8 GND                                                                                                                                                                                                                |
| Sense lines               | Each sense line has a divider (`R10` to `R18`), a filter capacitor (`C19` to `C21`) and a 3.3 V zener (`D11` to `D13`)                                                                                                                                                                                    |
| Indicators                | `D2` white raw input (through the Schottky `D8`), `D3` blue +12V_PROT, `D4` green +5 V, `D5` green +5V_SW, `D6` amber +3V3, `D7` red when `F1` has tripped; resistors `R19` to `R24`                                                                                                                      |
| Test points               | `TP1` to `TP10` on the main nodes                                                                                                                                                                                                                                                                         |
| Mounting holes            | Four M2.5 holes `H1` to `H4`, one in each corner                                                                                                                                                                                                                                                          |
| Hand-fitted parts         | `C1`, `C10`, `C17` and `J2` are through-hole parts fitted by hand                                                                                                                                                                                                                                         |

### Layout

| Item                    | Value                                                                                                                                                                                                                                                                               |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Outline                 | 114.9 x 76.0 mm                                                                                                                                                                                                                                                                     |
| Copper layers           | 2                                                                                                                                                                                                                                                                                   |
| Stackup                 | 1.6 mm board thickness; the board file has no stackup section                                                                                                                                                                                                                       |
| Parts in the BOM        | 84 lines in [`fab/bom.csv`](fab/bom.csv): 70 parts and 14 board features (ten test points, four holes)                                                                                                                                                                              |
| Parts placed in the CPL | 70 in [`fab/cpl.csv`](fab/cpl.csv): the 84 BOM lines less the ten test points and four holes                                                                                                                                                                                        |
| Nets                    | 34, counted in the board file                                                                                                                                                                                                                                                       |
| Track segments          | 288                                                                                                                                                                                                                                                                                 |
| Vias                    | 44                                                                                                                                                                                                                                                                                  |
| Track length            | 873.4 mm                                                                                                                                                                                                                                                                            |
| Zones and planes        | GND pour on F.Cu and on B.Cu; four screw-head keepouts, one at each of `H1` to `H4`                                                                                                                                                                                                 |
| Fab package             | [`fab/gerbers.zip`](fab/gerbers.zip), [`fab/bom.csv`](fab/bom.csv), [`fab/cpl.csv`](fab/cpl.csv); STEP model in [`models/protected_power_entry.step`](models/protected_power_entry.step); board in [`kicad/protected_power_entry.kicad_pcb`](kicad/protected_power_entry.kicad_pcb) |

### Checks

| Check                              | Result                               | Notes                                        |
| ---------------------------------- | ------------------------------------ | -------------------------------------------- |
| KiCad DRC (10.0.8, --refill-zones) | 0 errors, 80 warnings, 0 unconnected | All 80 warnings are lib_footprint_mismatch   |
| KiCad ERC (10.0.8)                 | 0 errors, 19 warnings, 0 unconnected | 18 lib_symbol_issues, 1 label_multiple_wires |

### Design figures

| Area             | Item                  | Value                                                                        | Basis                                                                           |
| ---------------- | --------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Power            | Supply range          | 12 V +/-5 % (10.8 to 12.6 V)                                                 | Design requirement in the source                                                |
| Power            | Rail ratings          | 5 V rail 1.5 A; 3.3 V rail 0.35 A                                            | Design requirement in the source                                                |
| Power            | Buck output           | 0.925 V x (1 + 44.2 k / 10 k) = 5.01 V                                       | Calculated; 0.925 V reference from the MP2307 BOM rating (datasheet not opened) |
| Power            | Buck start            | Enable divider 100 k / 33 k starts the buck above about 4.8 V in             | Calculated                                                                      |
| Power            | Buck input current    | 1.55 A at 10.8 V in                                                          | Calculated                                                                      |
| Power            | Load switch           | `LOAD_EN` is off by default (`R7` pull-down) and turns +5V_SW on above 1.0 V | Source design note                                                              |
| Thermal          | AMS1117 dissipation   | (5.0 - 3.3) V x 0.35 A = 0.60 W                                              | Calculated                                                                      |
| Thermal          | AMS1117 junction rise | 54 C at 90 C/W, so 0.35 A holds to 71 C ambient                              | Calculated; 90 C/W SOT-223 from the AMS1117 datasheet (ds1117)                  |
| Protection       | TVS `D1`              | 13.0 V standoff; 21.5 V clamp at 18.6 A (10/1000 us)                         | SMAJ13A datasheet (LCSC C148214)                                                |
| Protection       | Clamp margin          | 21.5 V clamp is below the MP2307 23 V recommended input and 26 V maximum     | MP2307 BOM rating (datasheet not opened)                                        |
| Protection       | Clamp margin          | 21.5 V clamp is below the 30 V drain-source rating of `Q1`                   | AO3401A datasheet (Alpha and Omega)                                             |
| Protection       | Reverse supply        | A reversed supply forward-biases `D1` and `F1` opens                         | Circuit topology in the source                                                  |
| Protection       | `Q1` gate             | `D9` 5.1 V leaves 6.9 V of margin to the 12 V gate-source rating of `Q1`     | Calculated; AO3401A datasheet (Alpha and Omega)                                 |
| Routing          | Net class widths      | PWR12 and SWNODE 0.6 mm, PWR5 0.6 mm, PWR3 0.5 mm, PGND 0.8 mm, SIG 0.25 mm  | Source net classes                                                              |
| Routing          | Vias                  | 0.6 mm over a 0.2 mm drill                                                   | Source net classes                                                              |
| Routing          | Width for 1.5 A       | 0.53 mm (1 oz, 10 C rise)                                                    | IPC-2221, calculated                                                            |
| Signal integrity | Edge rate             | PWR12, PWR5 and PWR3 declare `edge_rate slow`; SWNODE does not               | Source net classes                                                              |
| Assembly         | Edge connectors       | `J1` on the left edge; `J3`, `J4` and `J5` on the right edge                 | Source placement hints                                                          |

### Sourcing

| Item                       | Status                  | Notes                                                                                                    |
| -------------------------- | ----------------------- | -------------------------------------------------------------------------------------------------------- |
| BOM parts                  | 70 of 70 marked sourced | Distributors: LCSC, DigiKey, Farnell, RS and Wurth Elektronik, as listed in [`fab/bom.csv`](fab/bom.csv) |
| Datasheets                 | 4 rows not opened       | `C1`, `C10`, `C17` and `U1` say the datasheet was not opened                                             |
| `U1` MP2307                | Not For New Designs     | DigiKey lists it as Not For New Designs                                                                  |
| JLCPCB BOM match and quote | Not in the tree         | No longer recorded in this folder                                                                        |

## Not verified by Synth

| Area                       | What is not verified                                                                                                                           |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Electrical behaviour       | No board has been built or powered                                                                                                             |
| Router state, DFM and STEP | Router state, fabrication DFM and the STEP model check are not in this folder                                                                  |
| Reproducibility            | The export reproducibility and Gerber content hashes are no longer recorded here                                                               |
| Return paths               | One `+12V_PROT` via is farther from a ground via than the return-via window ([absmach/synth#280](https://github.com/absmach/synth/issues/280)) |
| Placement density          | Courtyard fill is low ([absmach/synth#427](https://github.com/absmach/synth/issues/427))                                                       |
| Surge                      | The input is qualified only up to 12.6 V and a surge of 18.6 A (10/1000 us)                                                                    |
| Independent sign-off       | The independent PCB review required by #39 is still open.                                                                                      |
