# RP2350 debug target

A small RP2350 debug target: USB-C power and data into a 3V3 LDO, the MCU's
own crystal, a tactile reset on RUN, a BOOTSEL button, a 3-pin SWD header,
QSPI boot flash, a status LED, and probe points on every rail.

Two ways in and two ways back: SWD for a debugger, and USB mass storage for
someone holding BOOTSEL with no debugger in the room.

It is not a development board, and the reason is a tool limitation rather than
a design choice — see *No GPIO header* below.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 89.60 × 63.98 mm |
| Layers | 4 |
| Process | JLCPCB filled-via (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring, `requires via_in_pad`) |
| Components | 50 |
| Nets | 25 |
| Source | `rp2350_debug_target.synth` |

## Why four layers, and why filled vias

The RP2350's QFN-60 is a 0.4 mm pitch package leaving 0.200 mm between pads,
and a via on this process needs 0.687 mm between centres. The drill that would
fit between two pins is 0.013 mm, which no fabricator offers. Synth's escape
analysis and KiCadRoutingTools reach that conclusion independently.

Two layers cannot route this part at all. Four can, and the price is the
filled-via tier: vias still land in pads, because on a 0.4 mm pitch there is
nowhere else for them to go. Four layers buy the escape; declaring
`requires via_in_pad` buys the right to plate it. The filled-via tier carries
the same track, clearance, drill and annular floors as the standard one — it
adds resin fill and a capped plating step and relaxes no minimum — so the
copper is judged against exactly the same numbers. 26 of this board's 59 vias
land in pads.

What four layers do not buy is spare escape budget. With #221 scattering this
board's bypass capacitors 8 to 17 mm from the die, the escape spends its room
reaching them — which is why there is no GPIO header and why RUN stays open.

## Rails

| Rail | Nominal | Source | Available | Feeds |
| --- | --- | --- | --- | --- |
| `V5_VBUS` | 5 V | J1 VBUS, through F1 500 mA | 500 mA, set by F1 | U1, D2, TP1 |
| `V3V3` | 3.3 V | U1 AMS1117-3.3 | 800 mA declared | all six IOVDD banks, QSPI, ADC, the switcher input, U3 |
| `VCORE` | ~1.1 V | the RP2350's own switcher: VREG_VIN from 3V3, VREG_LX through L1 into DVDD, VREG_FB sensing DVDD | — | DVDD, DVDD2, DVDD3 |

`VCORE` is named but not declared. It comes out of the MCU's own regulator,
and the part's operating conditions are device-level 1.8–3.6 V, so declaring a
voltage for it would have the range rules judge the core against the I/O
limits.

IOVDD4, IOVDD5 and IOVDD6 are supplied, each with its own 100 nF. They are not
optional pins with a default: they are the supply for three of the six I/O
banks, and a bank with no supply does not drive.

All four of J1's VBUS contacts and all four of its grounds are tied, not just
the first of each — a USB-C receptacle splits its current across them, and
wiring one pin of each leaves the connector carrying its rating on a quarter
of its contacts. SHLD goes to ground.

## Connectors

| Ref | Part | Edge | Carries |
| --- | --- | --- | --- |
| J1 | USB-C receptacle | left | 5 V in, D+/D−, CC resistors to 5.1k |
| J2 | 1×3 header, 2.54 mm | — | SWCLK, GND, SWDIO, in probe pin order |

Cable in one side, probe header across the top, both clear of the two corner
mounting holes.

## No GPIO header

A development board brings its I/O out. This one does not, and that is worth
stating as a measured limit rather than leaving as an absence.

A 20-pin header carrying 8 GPIOs, reset, 3V3 and a ground beside every signal
was written, validated with zero errors, and would not route. The cause is
#221: every `placement_hint { near: … priority: hard }` on this board is
dropped, so the eleven capacitors hinted onto `U2` sit **7.9 to 17.2 mm** from
a 7 × 7 mm QFN-60, mean 12.1 mm. With its bypass that far away, the escape
from a 0.4 mm pitch package has to cross the board to reach it, and there is
no budget left for sixteen — or eight — more signals.

What was tried, and what each changed:

| Change | Open nets |
| --- | --- |
| 16 GPIOs on the header | 10 |
| 8 GPIOs, ground beside each | 4 |
| four mounting holes cut to two | 4, byte-identical |
| `--place-route-iterate`, five candidate placements | 10, 6, 6, 7, 6 |
| no header | **1** |

Nothing about board area or candidate placement moves it; the hints are not
weighed and lost, they are not applied. Until #221 is fixed, exposing GPIOs
from this package needs every component pinned in a placement sidecar, which
is what the Class-C case 4 board does and why it carries a 152-entry
floorplan.

GP0 to GP29 therefore reach nothing. GP25 drives the status LED.

## Programming, debug and recovery

| Route | Needs | Use |
| --- | --- | --- |
| SWD on J2 | a debugger | normal development, and the only route that works with no firmware at all |
| USB mass storage | a USB-C cable | drag-and-drop a UF2 after holding SW2 and tapping SW1 |

SW2 holds QSPI_SS low through R5 1k at reset, so the boot ROM finds no valid
flash and comes up as a mass-storage device. The 1 kΩ is what lets the flash
still be selected normally while the button is up — it is a weak pull, not a
short.

The USB data path is why this is recoverable. It was left out of the first
version of this board as "a faster class's problem", but USB full speed is
12 Mbit/s and the point of it here is not bandwidth: it is that a board with
bad firmware can be rescued without a debugger. Both halves of each pair are
tied, so the cable works either way up, and D4 and D5 clamp both lines.

## Indicators and test points

| Ref | Reports |
| --- | --- |
| D3, green, R6 1k | `V3V3` present |
| D1, green, R4 1k | whatever firmware puts on GP25 |

Those are different questions, which is why there are two: the first version
had only the GP25 LED, so a board with no firmware looked identical to a board
with no power.

| Ref | Probes |
| --- | --- |
| TP1 | `V5_VBUS` |
| TP2 | `V3V3` |
| TP3 | `VCORE`, the MCU's own regulator output |
| TP4 | GND, the reference for the other three |

## Mechanical

Two M2.5 non-plated holes on the diagonal, and two fiducials.

Two rather than four, which is a decision about this board. Each hole brings
an all-layer `screw head` keepout, so four of them punch four holes through
every routing layer — and this is a bench target that sits on a desk or on two
standoffs, not a board bolted into an enclosure at four corners.

## Measured

One local run of the export and the external router.

| | |
| --- | --- |
| Nets closed | **24 of 25** |
| Open | 1 — `net_13` at `R3.2`, reaching 1 of 5 pads |
| Routed copper | 931.3 mm over 425 segments and 59 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | 26, all on the declared filled-via process |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, 1 warning |
| Routing verdict | `validation_failed` — not fabrication-ready |

Nothing is left for KiCad to report at error severity, and no pad is unconnected. Warnings are advisory: 1× `via_dangling`.

## Fabrication

`--gerbers --drill --pnp` produce 21 Gerber layers, Excellon PTH and NPTH
files with drill maps, `pnp.csv` and `bom.csv`. They are generated with
`--allow-unverified-parts`, because no part in the registry carries
provenance.

## System boundary

1. **Accepts** 5 V on J1 VBUS, fused at 500 mA and clamped; it advertises
   itself as a 5 V sink with 5.1 kΩ on each CC.
2. **Generates** `V3V3` from an AMS1117, and `VCORE` from the MCU's own
   switcher through L1.
3. **Supports** 500 mA at the input, which is the binding limit; the
   regulator's declared maximum is 800 mA.
4. **Exposes** nothing but SWD on J2 and four probe points. No GPIO, no
   reset line to a host — see *No GPIO header*.
5. **Provides** SWD on J2 and USB mass-storage recovery through SW2, plus four
   probe points.
6. **Still needs**, externally: a USB-C cable. A debugger only if you want
   one.
7. **Limits**: 3V3 logic, no 5 V tolerance on any exposed pin; 500 mA input;
   USB full speed only, no high-speed signalling; the filled-via process is
   mandatory, so this board cannot be built on a standard tier.

## Limitations

- **This board is laid out to the wrong device.** `rp2350` carries RP2040 pin
  numbers (#152), so every net on the QFN-60 reaches the pad an RP2040 would
  have. Nothing else here matters until that part is corrected, and no routing
  result from this board should be read as a Class-A pass.
- **RUN does not close, and no GPIO is exposed.** Both are #221: every hard
  `near` hint on this board is dropped, so parts that should sit against the
  die sit 8 to 19 mm away. See *Measured* and *No GPIO header*. Until that is
  fixed this board needs a pinned placement sidecar, the way the Class-C case
  4 board does.
- **The filled-via process is a cost, not a free pass.** Vias land in pads, so
  this board cannot be built on the standard tier at all. That is the QFN-60's
  geometry and it is why the tier is declared rather than assumed, but a
  reviewer should agree the part is worth it before the board is ordered.
- `W-SYNTH-THERMAL-001` reports U1's junction at about 107 °C, which is the
  AMS1117's declared maximum rather than this board's load. Filed as #274.
- `E-SYNTH-POWER-004` reports six load endpoints on the VBUS net against one
  capacitor. Four of those six are one connector's paralleled VBUS contacts.
  Filed as #276.
- `W-SYNTH-PART-UNVERIFIED` fires on `ptvs6v0s1ur`; that is #51.
- `polyfuse` declares a 5 V 100 mA envelope and carries `value "500mA"`, and
  `header_1x3` declares none at all. Filed as #275.
- Eighteen of its parts carry no distributor identity, so the BOM is not
  orderable as it stands.
- Seven `E-SYNTH-SCHEM-003` and two `E-SYNTH-SCHEM-001` findings are
  schematic-readability, not electrical.
- No independent reviewer has signed off.
