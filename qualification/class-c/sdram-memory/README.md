# SDRAM memory card

Two ISSI IS42S16400J 64 Mbit SDR SDRAM dice on four layers, sharing one
series-damped address and command bus and bringing a 32-bit data bus to a pair
of dual-row carrier connectors, with the serial presence detect, thermal
provision and supply filtering a memory module carries, under declared skew
and impedance budgets. Whether the routed copper meets them is measured below;
most of it does not.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 69.09 x 62.00 mm |
| Layers | 4 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 66 |
| Nets | 87 |
| Courtyard area | 1476 mm2 of 4284 mm2, 34.4% |
| Data width | 32 bits, two x16 dice |
| Placement | pinned, `sdram_memory.placement.layout.toml` |
| Source | `sdram_memory.synth` |

## Stackup

| Layer | Thickness | er | Carries |
| --- | --- | --- | --- |
| F.Cu | 0.035 mm | - | signal, ground pour |
| prepreg | 0.2104 mm | 4.4 | - |
| In1.Cu | 0.0152 mm | - | 3V3 plane |
| core | 1.065 mm | 4.6 | - |
| In2.Cu | 0.0152 mm | - | signal |
| prepreg | 0.2104 mm | 4.4 | - |
| B.Cu | 0.035 mm | - | signal, ground pour |

Total 1.5862 mm. The 0.2104 mm prepreg under F.Cu is the dimension the bus
geometry is derived from: over it, Synth's estimator puts a 0.2 mm trace at
67.3 Ohm and a 0.35 mm trace at 50.0 Ohm.

## Topology

| | |
| --- | --- |
| `DQ0`-`DQ15` | U1 only, masked by `DQM0` and `DQM1`, point to point, undamped |
| `DQ16`-`DQ31` | U2 only, masked by `DQM2` and `DQM3`, point to point, undamped |
| `A0`-`A11`, `BA0`, `BA1` | both dice, through R17-R30 - 14 damped two-load branches |
| `CLK`, `CKE`, `/CS`, `/RAS`, `/CAS`, `/WE` | both dice, through R7-R12 - 6 damped two-load branches |
| `DQM0`-`DQM3` | one die each, through R13-R16 - damped single loads |
| `SPD_SDA`, `SPD_SCL` | presence detect, J2 to U3, clamped at the connector |
| `THERM` | one analog line off the NTC divider |

Widening to x32 is what changes the problem from the easy half to the real
one, and damping is what changes the matching problem from one net to two.
Every shared line is now a stub from the connector to its 22 Ohm resistor and
a branch from the resistor to the dice: the stubs are nominally identical and
carry a `*_HOST` class, the branches are where the two halves diverge and
carry the bus class. A total path budget is the sum of the two.

## Supply

| Rail | Source | Nominal | Carried as | Feeds |
| --- | --- | --- | --- | --- |
| `RAIL_IN` | J2 pins 34, 35, 37, 38, from the carrier | 3.3 V | 0.8 mm track | F1 only |
| `V3V3` | F1, a 500 mA polyfuse | 3.3 V | **In1.Cu plane** | six core supply pins, U3, the pull-ups, the divider |
| `VDDQ` | L1, a 600 R ferrite bead off V3V3 | 3.3 V | 0.8 mm track | eight driver supply pins |
| `GND` | J1 and J2, 17 pins between them | 0 V | outer-layer pour | all fourteen ground pins |

The card takes its rail from the connector, as a mezzanine does, behind a
resettable fuse: a card that can be inserted live can also be inserted
wrongly. The die brings its core and driver supplies out on separate pins so
the two can be filtered apart, and a bead between them gives the switching
noise of thirty-two drivers a high impedance back toward the logic that has to
sample a 143 MHz clock, at a few tens of milliohms of DC.

Decoupling is one ceramic per supply pin - 0603 on the plane, 0402 on the
routed rail - plus 22 uF of bulk at each die, 22 uF on VDDQ, and 10 uF each
side of the fuse. TP1 and TP2 bring both rails out to probes.

Ground is **not** declared as a plane class. A declared plane needs a
stitching via beside every pad on it; GND reaches more than sixty endpoints
including both TSOP-II packages' own pads, and on an 0.8 mm two-row package
the exporter finds no legal position beside those pads. The bus escapes these
packages with a plain track on the standard floor, and buying a filled-via
process to stitch a ground plane would give that up for a return path the
sixteen ceramics already provide.

## Presence detect

| | |
| --- | --- |
| Device | U3, 24LC256, 32 KB I2C EEPROM |
| Address | 0x50 - A0, A1, A2 strapped to ground |
| Write protect | strapped to ground, so a host can program it |
| Bus | `SPD_SDA` and `SPD_SCL` on J2 pins 39 and 40, 4.7 kOhm to V3V3 |
| Clamp | D1, PRTR5V0U2X, across both lines at the connector |
| Decoupling | C20, 100 nF |

A module that cannot say what it is has to be configured by hand. The bus is
slow and it crosses the fast one, which is why it carries its own class with
its own width and clearance rather than the default.

## Thermal provision

| | |
| --- | --- |
| Sensor | R5, 10 kOhm NTC, B25/85 = 3380, between the two dice |
| Divider | R6, 10 kOhm to V3V3 |
| Output | `THERM`, J2 pin 30 |

The dice are the only parts here that dissipate anything, and a host that
cannot read the card's temperature cannot throttle it. An NTC in a divider is
what an SDR-era module did; it is deliberately not a digital sensor on the
presence detect bus, because the registry's TMP117 cannot have its address
strapped and a sensor whose address is decided by a floating pin is not a
measurement.

## Series damping

| | |
| --- | --- |
| Value | 22 Ohm, 0402 |
| Count | 24 - R7 clock, R8-R12 command, R13-R16 byte masks, R17-R30 address and bank |
| Position | beside the connector pin each damps, in two columns |
| Data bus | undamped: one pin to one pad on one die, no branch to damp |

A 143 MHz edge arriving at a T sees two unterminated stubs and reflects off
both. Twenty-two ohms in series at the source slows the edge into the branch
and dissipates the reflection on the way back, without the static current a
parallel termination to a reference rail would draw or the regulator that rail
would need.


## Connector pinouts


### J1 - data bus

| Pin | Net | Pin | Net | Pin | Net | Pin | Net |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `GND` | 11 | `GND` | 21 | `GND` | 31 | `GND` |
| 2 | `DQ0` | 12 | `DQ8` | 22 | `DQ16` | 32 | `DQ24` |
| 3 | `DQ1` | 13 | `DQ9` | 23 | `DQ17` | 33 | `DQ25` |
| 4 | `DQ2` | 14 | `DQ10` | 24 | `DQ18` | 34 | `DQ26` |
| 5 | `DQ3` | 15 | `DQ11` | 25 | `DQ19` | 35 | `DQ27` |
| 6 | `GND` | 16 | `GND` | 26 | `GND` | 36 | `GND` |
| 7 | `DQ4` | 17 | `DQ12` | 27 | `DQ20` | 37 | `DQ28` |
| 8 | `DQ5` | 18 | `DQ13` | 28 | `DQ21` | 38 | `DQ29` |
| 9 | `DQ6` | 19 | `DQ14` | 29 | `DQ22` | 39 | `DQ30` |
| 10 | `DQ7` | 20 | `DQ15` | 30 | `DQ23` | 40 | `DQ31` |

### J2 - address, command, clock, supply and sideband

| Pin | Net | Pin | Net | Pin | Net | Pin | Net |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `GND` | 11 | `DQM0_H` | 21 | `A4_H` | 31 | `BA0_H` |
| 2 | `CLK_H` | 12 | `DQM1_H` | 22 | `A5_H` | 32 | `BA1_H` |
| 3 | `GND` | 13 | `DQM2_H` | 23 | `A6_H` | 33 | `GND` |
| 4 | `CKE_H` | 14 | `DQM3_H` | 24 | `A7_H` | 34 | `RAIL_IN` |
| 5 | `CS_N_H` | 15 | `GND` | 25 | `GND` | 35 | `RAIL_IN` |
| 6 | `GND` | 16 | `A0_H` | 26 | `A8_H` | 36 | `GND` |
| 7 | `RAS_N_H` | 17 | `A1_H` | 27 | `A9_H` | 37 | `RAIL_IN` |
| 8 | `CAS_N_H` | 18 | `A2_H` | 28 | `A10_H` | 38 | `RAIL_IN` |
| 9 | `WE_N_H` | 19 | `A3_H` | 29 | `A11_H` | 39 | `SPD_SDA` |
| 10 | `GND` | 20 | `GND` | 30 | `THERM` | 40 | `SPD_SCL` |

Every `*_H` net ends at its damping resistor; the die-side net of the same
name carries the branch beyond it.

## Netclasses

| Class | Nets | Width | Clearance | Impedance | Budget |
| --- | --- | --- | --- | --- | --- |
| `SDRAM_DQL` | 16 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 2.5mm` |
| `SDRAM_DQH` | 16 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 2.5mm` |
| `SDRAM_ADDR` | 14 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 5mm` |
| `SDRAM_ADDR_HOST` | 14 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 3mm` |
| `SDRAM_CTRL` | 9 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 5mm` |
| `SDRAM_CTRL_HOST` | 9 | 0.35 mm | 0.2 mm | 50 Ohm | `max_skew 3mm` |
| `SDRAM_CLK` | 1 | 0.35 mm | 0.25 mm | 50 Ohm | `max_length 45mm` |
| `SDRAM_CLK_HOST` | 1 | 0.35 mm | 0.25 mm | 50 Ohm | `max_length 25mm` |
| `SPD` | 2 | 0.25 mm | 0.3 mm | - | - |
| `THERMAL` | 1 | 0.25 mm | 0.3 mm | - | - |
| `V3V3` | 1 | 0.6 mm | 0.25 mm | - | `plane` |
| `VDDQ` | 1 | 0.8 mm | 0.25 mm | - | - |
| `RAIL_IN` | 1 | 0.8 mm | 0.25 mm | - | - |
| `GND` | 1 | 0.6 mm | 0.25 mm | - | - |

Every net names its class. A class carries the nets that say so, and a skew
budget over one net is no budget at all.

## Placement

The floorplan is pinned in `sdram_memory.placement.layout.toml` and the
automatic placement is not what this card ships. The command connector takes
one edge with its twenty-four damping resistors in two columns beside the pins
they damp; the data connector takes the other; the dice are stacked between
them so each faces its own half of the data bus; each rail has its own
decoupling column per die; and the sideband fills the space beside the dice
that the bus does not need. The four mounting holes and two fiducials sit in
the top and bottom bands.

Placed automatically the same circuit comes out 88.59 x 72.09 mm at 22.6%
fill, with the two long headers as far apart as the canvas the solver settled
on. A pinned floorplan is also what makes the measurements below reproducible
rather than a property of a heuristic.

## Measured

Release build, KiCad 10.0.6, KiCadRoutingTools v0.23.0, one run.

| | |
| --- | --- |
| Nets closed | **87 of 87** |
| Open | none |
| Unconnected pads | none |
| Routed copper | 3502.4 mm over 1296 segments and 231 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| `kicad-cli pcb drc` | 88 errors, 0 unconnected items, 5 warnings |
| Engine verdict | `validation_failed` |

The 88 errors are 87 clearance violations and one hole clearance. The
clearance figures are the tell: `netclass 'GND' clearance 0.2500 mm; actual
0.1810 mm` and `netclass 'SDRAM_ADDR' clearance 0.2000 mm; actual 0.1657 mm`.
The exported `.kicad_dru` carries the manufacturer's edge and hole rules, not
the per-netclass clearances, so the external engine routes to the global
0.127 mm floor and KiCad then rejects the board against rules the engine never
saw. The five warnings are four silkscreen overlaps and one dangling track.

One blocking reason is not a clearance: a plane stitching via at (37.200,
26.600) mm sits inside U1 pad 34. The fanout places its vias from a
synthesized pad map rather than the footprint the board carries, so where they
land is not controlled.

### Skew against the declared budgets

| Class | Nets with copper | Shortest | Longest | Spread | Budget | |
| --- | --- | --- | --- | --- | --- | --- |
| `SDRAM_DQL` | 16 | `DQ9` 32.5 mm | `DQ2` 46.6 mm | 14.1 mm | 2.5 mm | missed |
| `SDRAM_DQH` | 16 | `DQ25` 29.5 mm | `DQ21` 71.1 mm | 41.6 mm | 2.5 mm | missed |
| `SDRAM_ADDR` | 14 | `A2` 42.7 mm | `A6` 101.1 mm | 58.4 mm | 5 mm | missed |
| `SDRAM_ADDR_HOST` | 14 | `A3_H` 7.1 mm | `BA0_H` 13.5 mm | 6.4 mm | 3 mm | missed |
| `SDRAM_CTRL` | 9 | `DQM0` 16.7 mm | `CKE` 93.6 mm | 77.0 mm | 5 mm | missed |
| `SDRAM_CTRL_HOST` | 9 | `CKE_H` 6.5 mm | `DQM0_H` 13.4 mm | 6.8 mm | 3 mm | missed |
| `SDRAM_CLK` | 1 | `CLK` 62.3 mm | - | - | 45 mm max | missed |
| `SDRAM_CLK_HOST` | 1 | `CLK_H` 6.9 mm | - | - | 25 mm max | **met** |

Lengths are Euclidean sums of the routed segments, measured outside Synth.

Two things are worth reading off that table rather than the headline. The
damped stubs are nearly matched - 6.4 and 6.8 mm of spread against a 3 mm
budget, where the branches they feed spread 58 and 77 mm - so the part of the
path this card controls behaves, and the part the router lays out does not.
And the clock stub is the first budget on this card that the copper meets.

The reason the branches miss is unchanged and structural: the language
declares the budget, `E-SYNTH-DRC-019` verifies it against copper, and nothing
between them tunes a track toward it. Two of the three parts of a
length-matched interface exist.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U1, U2 | IS42S16400J, 64 Mbit SDR SDRAM | `is42s16400j_sdram` |
| U3 | 24LC256, 32 KB I2C EEPROM | `24lc256_eeprom` |
| D1 | PRTR5V0U2X, dual rail clamp | `prtr5v0u2x` |
| L1 | 600 R ferrite bead, 0603 | `ferrite_bead_0603` |
| F1 | 500 mA resettable fuse | `polyfuse` |
| R5 | 10 kOhm NTC thermistor | `r_thermistor_ntc_10k` |
| J1, J2 | 2x20 pin header, 2.54 mm | `header_2x20` |
| TP1, TP2 | 1.5 mm test pad | `testpoint_pad_1_5` |

`header_2x20` was written for this board and verified against the stock KiCad
symbol and footprint: 40 part pins, 40 symbol pins, 40 footprint pads, matched
both ways. The remaining components are generic passives and mechanical
features; the generated `bom.csv` is the bill of materials.

## Limitations

- **The bus is not length-matched.** Declared and verified, not met - see
  *Skew against the declared budgets*. A carrier reading this card at 143 MHz
  would need each data group inside +/-100 mil of the clock; the routed copper
  spreads 14.1 mm and 41.6 mm.
- **Synth's own signal-integrity rules did not run on this copper.** The
  export runs them on the board named as delivered, and when the router's
  copper is rejected that name still points at the un-routed baseline. So the
  skew table above was measured externally, and the rules that would have
  reported it saw a board carrying only plane fanout. The impedance,
  crosstalk, ampacity, IR-drop and return-path rules are affected the same
  way.
- **A plane stitching via is inside a pad.** Reported by the connectivity
  check and blocking. The fanout works from a synthesized pad map, so this is
  not a position the board asked for.
- **Impedance is verified on the outer layers only.** The
  `W-SYNTH-IMPEDANCE-001` warnings say a line routed on an inner layer is
  referenced to copper the estimator does not model. A net cannot be pinned to
  a layer, so this cannot be designed away.
- **The clock is damped at the source but not terminated at the far end.** A
  22 Ohm series resistor is what an unbuffered module carries; a parallel
  termination would need a reference rail and a regulator this card does not
  have.
- **Ground is poured, not planed.** See *Supply*. The return path under the
  bus is the 3V3 plane, tied to ground by the sixteen ceramics, rather than a
  dedicated ground plane.
- **Seventeen connector pins carry ground and nothing else.** On a 2.54 mm
  header that is the best available return, and it is still worse than the
  edge fingers of a real memory module.
- **The EEPROM is not programmed.** Nothing writes an SPD image, so a host
  reading the card gets whatever the device shipped with.
- No thermal analysis. Each die draws 90 mA at 3.3 V, the NTC reports a
  voltage, and nothing checks what two dice do to the board.
- Return paths, layer transitions, via stubs and plane continuity are not
  analysed; the capability descriptor lists them as unsupported.
- Every component carries no distributor identity, so the BOM is not orderable
  as it stands.

## Reproducibility

The floorplan is pinned, so placement is byte-identical across runs by
construction. Routing is deterministic for a given board name: the name seeds
the entity uuids, so renaming changes the order the router works in. Compare
connectivity and per-class spread, not via counts.
