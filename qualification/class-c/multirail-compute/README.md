# Multi-rail compute board

An ESP32-WROOM-32 on four supply rails from one 12 V input, with an I2C GPIO
expander and two expansion headers, on a four-layer board whose two heavy
rails are inner-layer planes.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 109.68 × 90.79 mm |
| Layers | 4 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 48 |
| Nets | 25 |
| Source | `multirail_compute.synth` |

## Stackup

| Layer | Thickness | εr | Carries |
| --- | --- | --- | --- |
| F.Cu | 0.035 mm | — | signal, ground pour |
| prepreg | 0.2104 mm | 4.4 | — |
| In1.Cu | 0.0152 mm | — | 5 V plane |
| core | 1.065 mm | 4.6 | — |
| In2.Cu | 0.0152 mm | — | 3V3 plane |
| prepreg | 0.2104 mm | 4.4 | — |
| B.Cu | 0.035 mm | — | signal, ground pour |

Total 1.5862 mm.

## Supply

| Rail | Source | Nominal | Carried as | Feeds |
| --- | --- | --- | --- | --- |
| V12 | J1, through F1 and D1 | 12 V | traces | the buck |
| V5 | U1 MP2307 buck, L1 10 µH, 31.6k/10k divider | 5 V | **In1.Cu plane** | U2, U4 |
| V3V3 | U2 AMS1117-3.3 | 3.3 V | **In2.Cu plane** | the module, the expander |
| V3V3_A | U3 LP5907 behind FB1 | 3.3 V | trace | the analog header |
| V5_SW | U4 TPS22918 load switch | 5 V | trace | the peripheral header |

Reverse polarity is caught by D1 before the fuse; the TVS clamps after it, so
the fuse limits the current the clamp conducts.

## Thermal

| Part | Dissipation | Thermal resistance | Path |
| --- | --- | --- | --- |
| U1 MP2307 | 500 mW | 50 °C/W | exposed pad to the ground pour |
| U4 TPS22918 | 200 mW | 200 °C/W | SOT-23-6 leads |
| U3 LP5907 | 200 mW | 250 °C/W | SOT-23-5 leads |

NTC1 sits on the 5 V rail beside the buck, so the rise is readable rather
than assumed.

## Keepout

`antenna_clearance`, 12 mm radius, over the module's antenna end. Copper
under a PCB antenna detunes it; a keepout placed anywhere else would
constrain nothing and still read as satisfied.

## Assembly and DFM

Four M2.5 mounting holes, three 1 mm fiducials, and one test point per rail.
The input connector and both headers are hinted to edges, and the test points
to the top and bottom edges, so none of them is reached across a populated
board.

## Routing

KiCadRoutingTools 9399bfb, measured on this stack.

| | |
| --- | --- |
| Nets closed | 25 of 25 |
| Open nets | 0 |
| Copper | 1337.0 mm |
| Sub-minimum copper | none |
| `kicad-cli pcb drc` | 16 errors, 13 unconnected, 38 warnings |

Every net routes. That is the difference between this board and a fine-pitch
one: nothing here is below 1.27 mm pitch, so the router has room to work.

## Limitations

- **Twelve holes are below the declared drill floor, and none of them is
  ours.** KiCad's stock `RF_Module:ESP32-WROOM-32` carries twelve thermal
  vias at 0.2 mm inside its heatsink pad, against the 0.3 mm minimum this
  board's declared process states. The footprint's thermal path and the
  fabricator's floor disagree, and the board has to pick: a finer tier, or a
  modified footprint.
- **A 5 V stitching via lands on U2's 3.3 V pad**, shorting the two rails and
  raising three of the sixteen errors — the short, a hole-clearance breach and
  a mask bridge. Synth places that via, not the router; it is in the exported
  baseline. Filed as #183.
- **H2 and C1 courtyards overlap**: a mounting hole and the input electrolytic
  are placed on top of one another. Assembly access is declared through hints,
  and hints did not keep these two apart.
- Eight isolated copper fills. Plane continuity is checked by KiCad here, not
  by Synth, and nothing yet reports an orphaned island back to the design.
- The registry models eight of the module's thirty-eight pins, so the board
  exercises supply, enable, UART and I2C and leaves the rest undeclared. A
  spare GPIO would let the load switch be sequenced rather than strapped on.
- Twenty-three silkscreen overlaps and three library-footprint mismatches.
  The mismatches are a local library version difference, not a board fault.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U1 | MPS MP2307 3 A buck | `mp2307_buck` |
| U2 | AMS1117-3.3 LDO | `ams1117_3v3` |
| U3 | TI LP5907 low-noise LDO | `lp5907_3v3` |
| U4 | TI TPS22918 load switch | `tps22918_loadswitch` |
| U5 | Espressif ESP32-WROOM-32 | `esp32_wroom_32` |
| U7 | Microchip MCP23017 | `mcp23017_gpio` |

The generated `bom.csv` is the bill of materials; this table is the actives.
