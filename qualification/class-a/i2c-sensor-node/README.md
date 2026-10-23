# I²C sensor node

A field sensor node: JST-fed 5 V into a 3V3 LDO, an STM32G030 reading two I²C
devices — an environmental sensor and a config EEPROM — with a 4-pin JST
break-out for the host UART and a 20-pin expansion header carrying the bus,
SWD and ten spare GPIOs.

It is programmable, it is usable as a plain I²C module without its MCU, and
every rail has a probe point.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 71.10 × 64.00 mm |
| Layers | 2 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 37 |
| Nets | 23 |
| Source | `i2c_sensor_node.synth` |

## Rails

| Rail | Nominal | Source | Available | Feeds |
| --- | --- | --- | --- | --- |
| `V5_IN` | 5 V | J1, 2-pin JST PH, through F1 and D1 | 500 mA, set by F1 | U1, J3 pin 1 |
| `V3V3` | 3.3 V | U1 AMS1117-3.3 | 800 mA declared | MCU, both I²C devices, J2, J3 |

Protection order is the point of the front end: F1 polyfuse first, then D1
SS14 series Schottky, so a fault current meets a part that resets and a
reversed supply meets a blocking diode. D4 clamps the input behind the fuse —
a field node's supply arrives on a cable, which is where surges come from, and
the fuse limits what the clamp has to conduct.

The 3V3 budget is about 300 mA: the STM32G030's declared 150 mA, the BME680's
100 mA and the EEPROM's 50 mA. C2 is 10 µF, which is what the AMS1117 wants on
its output to be stable.

## Connectors

| Ref | Part | Carries |
| --- | --- | --- |
| J1 | 2-pin JST PH | 5 V in, GND |
| J2 | 4-pin JST PH | host UART, with D2 and D3 clamping both lines |
| J3 | 1×20 header, 2.54 mm | expansion: bus, debug, reset, GPIO |

**J2 pinout** — 3V3, TX (PA2), RX (PA3), GND, in that order.

**J3 pinout**

| Pin | Signal | Pin | Signal |
| --- | --- | --- | --- |
| 1 | `V5_IN` | 11 | PA0 |
| 2 | `V3V3` | 12 | PA1 |
| 3 | GND | 13 | PA4 |
| 4 | SDA (PA10) | 14 | PA5 |
| 5 | SCL (PA9) | 15 | PA6 |
| 6 | GND | 16 | PA7 |
| 7 | SWDIO (PA13) | 17 | PB0 |
| 8 | SWCLK (PA14) | 18 | PB1 |
| 9 | NRST | 19 | PB3 |
| 10 | GND | 20 | PB4 |

SDA and SCL are the same bus the on-board devices sit on, with the 4.7 kΩ
pull-ups already fitted, so a host can drive the sensor directly and hold the
MCU in reset through pin 9. That is what makes this a sensor module as well as
a sensor node.

Every signal on this header is 3V3 CMOS and none of it is protected. Driving
it from 5 V logic overstresses the pins: use a level shifter on the host side.
PA2 and PA3 are not here because they are the UART already brought out on J2,
clamped; PA9 and PA10 appear only as SDA and SCL. The remaining 13 pins of the
package are not exposed.

## I²C bus

One bus, 4.7 kΩ pull-ups on both lines, two devices: a BME680 environmental
sensor and a 24LC256 EEPROM. Each carries its own 100 nF, hinted to sit beside
the part rather than wherever the placer would otherwise put it.

| Device | Address | Set by |
| --- | --- | --- |
| U3 BME680 | 0x76 | CSB high selects I²C, SDO low selects the low address |
| U4 24LC256 | 0x50 | A0, A1 and A2 to ground |

U4's WP is grounded, so the EEPROM is write-enabled. All four of those pins
were floating before, which left the device's address and its write-protect
state undefined — four CMOS inputs with no DC path.

## Indicators and test points

D5, green, through R5 1k, says `V3V3` is up — the first question when a node
is dark.

| Ref | Probes |
| --- | --- |
| TP1 | `V5_IN` |
| TP2 | `V3V3` |
| TP3 | SDA |
| TP4 | SCL |
| TP5 | GND, the reference for the other four |

## Mechanical

Four M2.5 non-plated holes, hinted to the four corners so the node can be
mounted in an enclosure, and two fiducials.

## Measured

One local run of the export and the external router, with
`--place-route-iterate`, which routes each candidate placement and keeps the
one that routes best. That flag is load-bearing here and the reason is worth
reading: of the five candidates, **four route cleanly and one does not**, and
the default path takes the first. The one it takes leaves the ground pour cut
into two islands with no via tying them together — every net connected, 349
segments, and `kicad-cli pcb drc` reporting one unconnected item that names
`Zone [GND]` twice. Filed as #279; the figures below are the placement the
loop chose.

| | |
| --- | --- |
| Nets closed | **23 of 23** |
| Open | **0** |
| Routed copper | 1191.7 mm over 273 segments and 65 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **0** |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, 2 warnings |
| Routing verdict | `routed`, fabrication-ready |

Nothing is left for KiCad to report at error severity, and no pad is unconnected. Warnings are advisory: 2× `silk_overlap`.

## Fabrication

The package validates, so the artifacts are real: `bom.csv` and `pnp.csv` come
out of the run above, and `--gerbers --drill` add 21 Gerber layers and Excellon
PTH and NPTH files with drill maps. They need `--allow-unverified-parts`,
because no part in the registry carries provenance.

## System boundary

1. **Accepts** 5 V nominal on J1, 2-pin JST PH, reverse-protected and fused
   at 500 mA.
2. **Generates** `V3V3` from an AMS1117, and passes `V5_IN` out on J3 pin 1.
3. **Supports** a ~300 mA 3V3 budget against the regulator's declared 800 mA.
   The input fuse is the binding limit, not the regulator.
4. **Exposes** the I²C bus, the host UART, reset and ten GPIOs, all 3V3 CMOS.
5. **Provides** SWD on J3 pins 7 and 8 with reset on pin 9, and five probe
   points.
6. **Still needs**, externally: a 5 V supply, and either a UART host on J2 or
   an I²C host on J3. A debug probe to load firmware the first time.
7. **Limits**: 3V3 logic only, no 5 V tolerance on any exposed pin; 500 mA
   input; 2.54 mm header pitch; no conformal coating or sealing, so "field"
   means inside an enclosure.

## Limitations

- **Two parts are operated outside the envelope their registry entries
  declare.** `jst_ph_2pin` and `jst_ph_4pin` each declare 5 V and 100 mA,
  against a ~300 mA budget; `polyfuse` declares 100 mA and carries
  `value "500mA"`. The real devices are fine — a JST PH is 100 V, 2 A — but
  the declared numbers are a template default. Filed as #275.
- `W-SYNTH-THERMAL-001` reports U1's junction at about 107 °C. That is the
  AMS1117's declared 1360 mW through 60 °C/W, not this board's load: at
  300 mA the drop is 510 mW and the rise about 31 °C. Filed as #274.
- `W-SYNTH-PART-UNVERIFIED` fires on `ptvs6v0s1ur`, the input clamp, which
  carries a `[provenance]` table naming no reviewer. That is #51's governance
  question rather than this board's.
- No level shifting on J3. A 5 V host needs one of its own, and the header is
  documented accordingly rather than protected.
- Eighteen of its parts carry no distributor identity, so the BOM is not
  orderable as it stands.
- Two `E-SYNTH-SCHEM-001` and two `E-SYNTH-SCHEM-003` findings are
  schematic-readability, not electrical, and `kicad-cli pcb drc` adds two
  `silk_overlap` warnings — reference designators crossing a pad, which a
  silkscreen pass Synth does not perform would move.
- **The default export path does not produce this board.** It picks the first
  candidate placement, which cuts the ground pour in two (#279). The figures
  above come from `--place-route-iterate`, and a reviewer re-running without
  it will get a refused package.
- No thermal measurement, and no independent reviewer has signed off.
