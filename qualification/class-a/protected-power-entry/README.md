# Protected 12 V power entry (Class A, case 3)

A 12 V power-entry board with no microcontroller. The input passes a polyfuse, a TVS diode and a reverse-polarity P-MOSFET. An MP2307 buck makes 5.0 V, a TPS22918 load switch makes a switched 5 V output, and an AMS1117-3.3 makes 3.3 V. Three JST XH outputs and an 8-pin control header leave the board. It has two copper layers, measures 114.9 x 76.0 mm and has 84 parts.

It answers whether `synth mcp`, driven by an agent from a prompt, makes a routed power stage with a protected input, net classes sized for the current and connectors on the board edges. The design is [`protected_power_entry.synth`](protected_power_entry.synth) with its placement sidecar [`protected_power_entry.placement.layout.toml`](protected_power_entry.placement.layout.toml).

The claim covers the circuit, the part choices and the routed board in `kicad/`. The supply is 12 V +/-5 % (10.8 to 12.6 V), and the 5 V and 3.3 V rails are rated for 1.5 A and 0.35 A. No board has been built or powered, so no electrical behaviour is measured.

The prompt used to produce the board is in [`PROMPT.md`](PROMPT.md).

## The board

| Group | Parts |
|---|---|
| 12 V input and protection | `J1` JST S2B-XH-A-1 (pin 1 +12 V, pin 2 GND); `F1` Littelfuse 1812L150/16DR (1.5 A hold, 3 A trip, 16 V); `D1` Littelfuse SMAJ13A TVS on the fused node ahead of `Q1`; `Q1` AO3401A P-MOSFET for reverse polarity, gate held within 5.1 V of the source by the zener `D9` and `R1` 47 k; `C1` 100 uF 25 V |
| 5 V buck | `U1` MPS MP2307DN (3 A, 0.925 V reference); `L1` Bourns SRN6045TA-100M 10 uH; input `C2`, `C3` 10 uF 25 V and `C4` 100 nF; boot `C5`, soft-start `C6`, compensation `R2` and `C7`; feedback `R3` 44.2 k and `R4` 10 k; enable divider `R5` 100 k and `R6` 33 k; output `C8`, `C9` 22 uF and `C10` 100 uF |
| Load switch | `U3` TI TPS22918 with `C12`, `C13` (2.2 nF slew), `C14`; `R7` pull-down, `R8` and zener `D10` on `LOAD_EN` |
| 3.3 V | `U2` AMS1117-3.3 (SOT-223) from the 5 V rail, with `C15` to `C18` |
| Outputs | `J3` +5 V through `F2`, `J4` +5V_SW through `F3`, `J5` +3V3; all JST XH, pin 2 GND |
| Control header | `J2` 1x8: 1 +3V3_REF, 2 GND, 3 LOAD_EN, 4 GND, 5 SENSE_12V, 6 SENSE_5V, 7 SENSE_3V3, 8 GND; each sense line has a divider (`R10` to `R18`), a filter capacitor (`C19` to `C21`) and a 3.3 V zener (`D11` to `D13`) |
| Indicators | `D2` white raw input (through the Schottky `D8`), `D3` blue +12V_PROT, `D4` green +5 V, `D5` green +5V_SW, `D6` amber +3V3, `D7` red when `F1` has tripped; resistors `R19` to `R24` |
| Test points and holes | `TP1` to `TP10` on the main nodes; four M2.5 holes `H1` to `H4`, one in each corner |

Design notes, from the source and the part ratings in [`fab/bom.csv`](fab/bom.csv):

- The buck output is 0.925 V x (1 + 44.2 k / 10 k) = 5.01 V. The enable divider starts it above about 4.8 V in.
- `LOAD_EN` is off by default and turns `+5V_SW` on above 1.0 V.
- `D1` has a 13.0 V standoff and a 21.5 V clamp at 18.6 A (10/1000 us). The clamp is below the MP2307's 23 V recommended input and 26 V maximum and the 30 V rating of `Q1`. A reversed supply forward-biases `D1` and `F1` opens.
- `D9` leaves 6.9 V of margin to the 12 V gate-source rating of `Q1`.
- The AMS1117 dissipates (5.0 - 3.3) V x 0.35 A = 0.60 W. At 90 C/W the junction rises 54 C, so 0.35 A holds to 71 C ambient.
- Net classes: PWR12 and SWNODE 0.6 mm, PWR5 0.6 mm, PWR3 0.5 mm, PGND 0.8 mm, SIG 0.25 mm; vias 0.6 mm over a 0.2 mm drill. By IPC-2221 (1 oz, 10 C rise) 1.5 A needs 0.53 mm, and the buck draws 1.55 A at 10.8 V in.
- PWR12, PWR5 and PWR3 declare `edge_rate slow`; SWNODE does not.

The board in [`kicad/protected_power_entry.kicad_pcb`](kicad/protected_power_entry.kicad_pcb) has 288 track segments and 44 vias. [`fab/cpl.csv`](fab/cpl.csv) lists 70 placements: the 84 BOM lines less the ten test points and four holes. `C1`, `C10`, `C17` and `J2` are through-hole parts fitted by hand.

## Not verified by Synth

- **Checks:** DRC, ERC, router state, fabrication DFM and the STEP model check are not in this folder. Their evidence is no longer in the tree.
- **Reproducibility:** the export reproducibility and Gerber content hashes are no longer recorded here.
- **Sourcing:** the JLCPCB BOM match and quote are no longer in the tree. Some BOM rows say the datasheet was not opened.
- **Return paths:** one `+12V_PROT` via is farther from a ground via than the return-via window ([absmach/synth#280](https://github.com/absmach/synth/issues/280)).
- **Placement density:** courtyard fill is low ([absmach/synth#427](https://github.com/absmach/synth/issues/427)).
- **Surge:** the input is qualified only up to 12.6 V and a surge of 18.6 A (10/1000 us).
- **MP2307:** DigiKey lists it as Not For New Designs.
