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
| Routed copper | 403.6 mm over 131 segments and 11 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **0** |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, 1 warning |
| Routing verdict | `routed`, fabrication-ready |

### DRC detail

| Severity | Finding | Count |
| --- | --- | --- |
| warning | track has unconnected end | 1 |

Nine `annular_width` errors stood here until the net classes Synth writes into
the exported project stopped asking for a 0.6 mm via over a 0.4 mm drill — a
0.10 mm ring against the 0.13 mm the same file states. Nothing about the board
changed; the project stopped contradicting itself, and the single remaining
finding is advisory.

## Limitations

- **One dangling track end.** A stub the router left behind; every net is
  closed and no pad is unconnected, so it is redundant copper rather than a
  missing connection.
- **Eleven layer changes have no ground return via within 1.53 mm**
  (`W-SYNTH-DRC-016`). On two layers with no plane there is nowhere for the
  return current to follow the signal, which is a property of the stackup
  rather than of this routing run.
- **Four floating digital inputs** (`E-SYNTH-CONNECT-008`): `U4.a0`, `U4.a1`,
  `U4.a2` and `U4.wp` on the 24LC256. The EEPROM answers at 0x50 with its
  address pins low and is write-enabled with WP low, and all four need
  strapping to a rail rather than leaving the inputs undefined.
- `E-SYNTH-POWER-001` and `E-SYNTH-POWER-006` want more decoupling on the 3V3
  rail than the two bulk capacitors at the regulator provide.
- The BME680's LGA-8 is 0.65 mm pitch and leaves 0.150 mm between pads, under
  the 0.254 mm a minimum track needs. The escape analysis reports it; the
  board still routes, and on two layers that package has no inner plane to
  change layers into.
- Five of its parts carry no distributor identity, so the BOM is not orderable
  as it stands.
- No thermal analysis, and no independent reviewer has signed off.
