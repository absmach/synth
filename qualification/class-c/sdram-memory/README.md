# SDRAM memory card

Two ISSI IS42S16400J 64 Mbit SDR SDRAM dice on four layers, sharing one
address and command bus and bringing a 32-bit data bus to a pair of dual-row
carrier connectors, under declared skew and impedance budgets. Whether the
routed copper meets them is measured below; it does not.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 68.59 × 67.90 mm, auto-sized |
| Layers | 4 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 31 |
| Nets | 58 |
| Data width | 32 bits, two x16 dice |
| Source | `sdram_memory.synth` |

## Stackup

| Layer | Thickness | εr | Carries |
| --- | --- | --- | --- |
| F.Cu | 0.035 mm | — | signal, ground pour |
| prepreg | 0.2104 mm | 4.4 | — |
| In1.Cu | 0.0152 mm | — | 3V3 plane |
| core | 1.065 mm | 4.6 | — |
| In2.Cu | 0.0152 mm | — | signal |
| prepreg | 0.2104 mm | 4.4 | — |
| B.Cu | 0.035 mm | — | signal, ground pour |

Total 1.5862 mm. The 0.2104 mm prepreg under F.Cu is the dimension the bus
geometry is derived from: over it, Synth's estimator puts a 0.2 mm trace at
67.3 Ω and a 0.35 mm trace at 50.0 Ω.

## Topology

| | |
| --- | --- |
| `DQ0`–`DQ15` | U1 only, masked by `DQM0` and `DQM1` |
| `DQ16`–`DQ31` | U2 only, masked by `DQM2` and `DQM3` |
| `A0`–`A11`, `BA0`, `BA1` | both dice — 14 two-load branches |
| `CLK`, `CKE`, `/CS`, `/RAS`, `/CAS`, `/WE` | both dice — 6 two-load branches |

Widening to x32 is what changes the problem from the easy half to the real
one. Sixteen data lines running point to point from a connector to one pad are
routed; thirty-two split into two groups that match within a group, over an
address and command bus whose every line is a branch whose two halves have to
match each other. A single-die card does not carry that constraint, and a
x32 module is how memory is actually built.

## Supply

| Rail | Source | Nominal | Carried as | Feeds |
| --- | --- | --- | --- | --- |
| V3V3 | J2 pins 34, 35, 37, 38, from the carrier | 3.3 V | **In1.Cu plane** | all fourteen supply pins |
| GND | J1 and J2, 20 pins between them | 0 V | outer-layer pour | all fourteen ground pins |

The card takes its rail from the connector, as a mezzanine does. Decoupling is
one 100 nF per supply pin — the seven pins of each die sit on four sides of
its package, so a shared capacitor would be local to only one of them — plus
22 µF of bulk at each die and 10 µF where the rail lands. TP1 and TP2 bring
3V3 and ground out to probes.

Ground is **not** declared as a plane class. A declared plane needs a stitching
via beside every pad on it; GND reaches more than sixty endpoints including
both TSOP-II packages' own pads, and on an 0.8 mm two-row package the exporter
finds no legal position beside those and refuses the export unless via-in-pad
is allowed. The fourteen decoupling capacitors tie the 3V3 plane to ground at
every supply pin, which is what makes it a valid AC reference for the bus
above it.

`R1` pulls `CKE` down and `R2` pulls `/CS` up, so both dice are out of
command and deselected until the carrier drives them rather than until it
happens to.

## Connector pinouts

Both are 2x20 headers on 2.54 mm, odd-even numbered, so consecutive pins sit
across the two rows at one position. A dual row carries forty pins in the
51.81 mm a single row spends on twenty, which is why a 71-signal bus reaches
the card edge without setting the card's size.

Grounds are interleaved with the signals rather than grouped, so every run of
bus lines has a return pin next to it.

### J1 — data bus

| Pin | Net | Pin | Net | Pin | Net | Pin | Net |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | GND | 11 | GND | 21 | GND | 31 | GND |
| 2 | DQ0 | 12 | DQ8 | 22 | DQ16 | 32 | DQ24 |
| 3 | DQ1 | 13 | DQ9 | 23 | DQ17 | 33 | DQ25 |
| 4 | DQ2 | 14 | DQ10 | 24 | DQ18 | 34 | DQ26 |
| 5 | DQ3 | 15 | DQ11 | 25 | DQ19 | 35 | DQ27 |
| 6 | GND | 16 | GND | 26 | GND | 36 | GND |
| 7 | DQ4 | 17 | DQ12 | 27 | DQ20 | 37 | DQ28 |
| 8 | DQ5 | 18 | DQ13 | 28 | DQ21 | 38 | DQ29 |
| 9 | DQ6 | 19 | DQ14 | 29 | DQ22 | 39 | DQ30 |
| 10 | DQ7 | 20 | DQ15 | 30 | DQ23 | 40 | DQ31 |

Pins 1–20 are U1's byte lanes, 21–40 are U2's.

### J2 — address, command, clock and supply

| Pin | Net | Pin | Net | Pin | Net | Pin | Net |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | GND | 11 | DQM0 | 21 | A4 | 31 | BA0 |
| 2 | CLK | 12 | DQM1 | 22 | A5 | 32 | BA1 |
| 3 | GND | 13 | DQM2 | 23 | A6 | 33 | GND |
| 4 | CKE | 14 | DQM3 | 24 | A7 | 34 | V3V3 |
| 5 | nCS | 15 | GND | 25 | GND | 35 | V3V3 |
| 6 | GND | 16 | A0 | 26 | A8 | 36 | GND |
| 7 | nRAS | 17 | A1 | 27 | A9 | 37 | V3V3 |
| 8 | nCAS | 18 | A2 | 28 | A10 | 38 | V3V3 |
| 9 | nWE | 19 | A3 | 29 | A11 | 39 | GND |
| 10 | GND | 20 | GND | 30 | GND | 40 | GND |

The clock is the one pin whose neighbours are chosen for the signal rather than
for the pin count: ground on both sides. The four mask lines are the only
signals on this connector that do not fan out to both dice.

## Netclasses

| Class | Nets | Width | Clearance | Impedance | Budget |
| --- | --- | --- | --- | --- | --- |
| `SDRAM_DQL` | 16 | 0.35 mm | 0.2 mm | 50 Ω | `max_skew 2.5mm` |
| `SDRAM_DQH` | 16 | 0.35 mm | 0.2 mm | 50 Ω | `max_skew 2.5mm` |
| `SDRAM_ADDR` | 14 | 0.35 mm | 0.2 mm | 50 Ω | `max_skew 5mm` |
| `SDRAM_CTRL` | 9 | 0.35 mm | 0.2 mm | 50 Ω | `max_skew 5mm` |
| `SDRAM_CLK` | 1 | 0.35 mm | 0.25 mm | 50 Ω | `max_length 60mm` |
| `V3V3` | 1 | 0.6 mm | 0.25 mm | — | `plane` |
| `GND` | 1 | 0.6 mm | 0.25 mm | — | — |

Five bus classes rather than one because a memory bus is not one group: the
data lines are sampled against the clock, the address lines are set up before
it, and one tolerance across all seventy-one would state a constraint nobody
has. 2.5 mm on a data group is the ±100 mil the SDRAM layout guides ask for.

The two data groups are separate because nothing requires `DQ0` and `DQ31` to
match — they are captured by different dice under different masks. What has to
match is each group against the clock.

The clock takes a length rather than a skew because a skew budget on a one-net
class is a no-op: the group check compares a class's members against each other
and skips a class with fewer than two routed members.

## Measured

Everything below came from one local run of the export and the external
router, measured on the copper it produced. Lengths are the sum of each net's
routed segments along the segment, and the total agrees with the run record's
own `wire_length_mm`.

| | |
| --- | --- |
| Nets closed | **58 of 58** |
| Open | none |
| Routed copper | 3105.1 mm over 791 segments and 199 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| `kicad-cli pcb drc` | 162 errors, 0 unconnected items, 9 warnings |
| Engine verdict | `rejected` |

### Skew against the declared budgets

| Class | Nets with copper | Shortest | Longest | Spread | Budget | |
| --- | --- | --- | --- | --- | --- | --- |
| `SDRAM_DQL` | 16 | `DQ12` 25.2 mm | `DQ8` 89.5 mm | 64.3 mm | 2.5 mm | over by 61.8 mm |
| `SDRAM_DQH` | 16 | `DQ24` 21.3 mm | `DQ23` 57.3 mm | 35.9 mm | 2.5 mm | over by 33.4 mm |
| `SDRAM_ADDR` | 14 | `A3` 51.0 mm | `A5` 106.3 mm | 55.3 mm | 5 mm | over by 50.3 mm |
| `SDRAM_CTRL` | 9 | `DQM0` 40.8 mm | `CKE` 110.0 mm | 69.3 mm | 5 mm | over by 64.3 mm |
| `SDRAM_CLK` | 1 | `CLK` 72.7 mm | — | — | 60 mm max | over by 12.7 mm |

Every budget is missed. That is the result, and it is a property of the
toolchain rather than of this board: the language declares a bus skew budget,
`E-SYNTH-DRC-019` verifies it against routed copper, and **no stage between
them tunes a track toward it**. There is no serpentine or length-matching pass,
so the spread is whatever the geometry and the router's search order produce.
Two of the three parts of a length-matched interface are present.

Two measurement defects sit on top of that, each reported separately:

- The figures above are physical lengths. The checks that judge them add
  `dx + dy` per segment instead, which overstates a 45° track by up to 41%;
  384 of the 791 segments here are diagonal, and the board total reads
  3740.6 mm rather than 3105.1 mm.
- `CLK` reaches three pads, and the check sums both branches rather than
  measuring the path to either die, so the 60 mm budget is being compared
  against the whole T.

### DRC detail

| Severity | Finding | Count |
| --- | --- | --- |
| error | annular width | 158 |
| error | clearance | 4 |
| warning | hole to hole | 6 |
| warning | dangling via | 3 |

The 158 annular-width errors are one defect, not 158: every via a net class
places is 0.6 mm on a 0.4 mm drill, a 0.10 mm ring against the 0.13 mm
minimum the same exported project states. A fix is in flight.

The four clearance errors are two stitching vias, each 0.055 mm from a pad of
U2 — `A8` and `DQM3` — against the 0.25 mm the board declares. Synth places
them, before the router runs.

## Placement quality

| | |
| --- | --- |
| Board area | 4658 mm² |
| Component courtyards | 1335 mm² |
| Fill | 28.7% |
| Closest decoupling capacitor to its die's courtyard | 3.8 mm |
| Farthest | 20.7 mm |

Every capacitor declares `placement_hint { near: "<die>" priority: hard }`,
which the placer documents as a bound of 8 mm around the anchor's courtyard.
Nine of the fourteen hard hints land outside that bound, the worst 11.8 mm
beyond it, and the hint is dropped without a diagnostic.

The card is larger than a module of this content would be for a related
reason: the outline is auto-sized from where the placer put things, and
SynthSpec has no way to declare one.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U1, U2 | IS42S16400J, 64 Mbit SDR SDRAM | `is42s16400j_sdram` |
| J1, J2 | 2×20 pin header, 2.54 mm | `header_2x20` |
| TP1, TP2 | 1.5 mm test pad | `testpoint_pad_1_5` |

`header_2x20` was written for this board and verified against the stock KiCad
symbol and footprint: 40 part pins, 40 symbol pins, 40 footprint pads, matched
both ways. The remaining 25 components are generic passives and mechanical
features; the generated `bom.csv` is the bill of materials.

## Limitations

- **The bus is not length-matched.** Declared and verified, not met — see
  *Skew against the declared budgets*. A carrier reading this card at 143 MHz
  would need each data group inside ±100 mil of the clock; the routed copper
  spreads 35.9 mm and 64.3 mm.
- **Synth's own signal-integrity rules did not run on this copper.** The
  export runs them on the board named as delivered, and when the router's
  copper is rejected that name still points at the un-routed baseline — 37
  segments of plane fanout rather than 791 segments of routing. So the skew
  table above was measured externally, and the rules that would have reported
  it saw an empty board. The impedance, crosstalk, ampacity, IR-drop and
  return-path rules are affected the same way.
- **Impedance is verified on the outer layers only.** The
  `W-SYNTH-IMPEDANCE-001` warnings, one per bus net, say a line routed on an
  inner layer is referenced to copper the estimator does not model. A net
  cannot be pinned to a layer, so this cannot be designed away.
- **No source termination on the clock, by design.** A series resistor
  belongs at the driver, which is on the carrier. A resistor in the middle of
  this card's transmission line would damp the wrong end.
- **Ground is poured, not planed.** See *Supply*. The return path under the
  bus is the 3V3 plane, tied to ground by the fourteen decoupling capacitors,
  rather than a dedicated ground plane.
- **Twenty connector pins carry ground and nothing else.** On a 2.54 mm header
  that is the best available return, and it is still worse than the edge
  fingers of a real memory module. The card is a geometry and constraint
  exercise, not a 143 MHz signal-integrity claim.
- No thermal analysis. Each die draws 90 mA at 3.3 V and nothing checks what
  two of them do to the board.
- Return paths, layer transitions, via stubs and plane continuity are not
  analysed; the capability descriptor lists them as unsupported.
- Every component carries no distributor identity, so the BOM is not orderable
  as it stands.

## Reproducibility

Routing is deterministic for a given board name: the name seeds the entity
uuids, so renaming changes the order the router works in. Placement is
byte-identical across runs. Compare connectivity and per-class spread, not via
counts.
