# I²C sensor node

A field sensor node: JST-fed 5 V into a 3V3 LDO, an STM32G030 reading two I²C
devices — an environmental sensor and a config EEPROM — with a 4-pin JST
break-out for the host UART. One bus, one set of pull-ups.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 78.85 × 43.50 mm |
| Layers | 2 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 19 |
| Nets | 10 |
| Source | `i2c_sensor_node.synth` |

## Supply

| Rail | Source | Protection |
| --- | --- | --- |
| 5 V in | J1, 2-pin JST PH | F1 500 mA polyfuse, then D1 SS14 series Schottky |
| 3V3 | U1 AMS1117-3.3 | 10 µF in, 1 µF out |

Polyfuse before the Schottky: resettable over-current first, then
reverse-polarity, so a reversed supply meets a blocking diode and a fault
current meets a part that resets rather than one that burns.

## Connectors

| Ref | Part | Carries |
| --- | --- | --- |
| J1 | 2-pin JST PH | 5 V, GND |
| J2 | 4-pin JST PH | host UART, with D2 and D3 clamping both lines |

## I²C bus

One bus, 4.7 kΩ pull-ups on both lines, two devices: a BME680 environmental
sensor and a 24LC256 EEPROM. Each carries its own 100 nF, hinted to sit beside
the part rather than wherever the placer would otherwise put it.

## Measured

One local run of the export and the external router.

| | |
| --- | --- |
| Nets closed | **10 of 10** |
| Open | **0** |
| Routed copper | 408.9 mm over 155 segments and 15 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| `kicad-cli pcb drc` | 9 errors, 0 unconnected items, 10 warnings |

Three consecutive exports produce a byte-identical board and schematic.

### DRC detail

| Severity | Finding | Count |
| --- | --- | --- |
| error | clearance | 7 |
| error | via diameter | 2 |
| warning | silkscreen over copper | 6 |
| warning | library footprint mismatch | 3 |
| warning | dangling via | 1 |

Every net on this board closes and no copper is below the fabrication floor.
What remains is seven places where the router worked inside the clearance the
board asked for.

## Limitations

- **Seven clearance violations.** The board routes completely but not to its
  own declared clearance in seven places; each is copper a human would nudge.
- **One via lands inside a pad** and via-in-pad is not declared, so the
  package is refused. On a two-layer board at this pitch that via is avoidable
  by hand.
- Six silkscreen-over-copper warnings are reference designators crossing pads.
  They are cosmetic on a fabricated board and would be fixed in a silkscreen
  pass that Synth does not perform.
- No fabrication artifacts are produced; the package is refused as
  `UNTRUSTED / NOT FOR FABRICATION`.
- No part carries distributor identity, so the BOM is not orderable as it
  stands.
