# Protected power entry

A power front end on its own board: 12 V in, reverse-polarity P-FET,
resettable over-current, TVS clamp, a buck down to 5 V, a soft-start load
switch, and a 3V3 rail for logic. No MCU — this board exists to be reviewed
as a power stage.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 59.73 × 47.92 mm |
| Layers | 2 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 23 |
| Nets | 13 |
| Source | `protected_power_entry.synth` |

## The protection chain, in fault order

| Stage | Part | Against |
| --- | --- | --- |
| 1 | Q1 Si2301 P-FET, R1 100k, D1 12 V zener | reverse polarity — the FET blocks before anything downstream sees it |
| 2 | F1 polyfuse, 1.5 A | over-current, resettable |
| 3 | D2 TVS array, SMAJ15A | transients, clamped after the fuse so the fuse limits what the TVS conducts |

A TVS in front of the fuse has nothing to limit the fault current it conducts,
which is why the order is the order.

## Supply

| Rail | Source | Feeds |
| --- | --- | --- |
| 12 V | J1, 2-pin JST XH, through the chain above | the buck |
| 5 V | U1 MP2307, L1 6.8 µH, 10k/3.16k divider | the load switch |
| 5 V switched | U2 TPS22918, R4 100k, C5 10 nF soft-start | J2 |
| 3V3 | U3 AMS1117-3.3 | the status LED, and logic a carrier would add |

U1's exposed pad is tied to GND. It is the buck's heat path and the part
declares it `required`, so leaving it floating is an error rather than a
silent thermal fault.

## Connectors

| Ref | Part | Carries |
| --- | --- | --- |
| J1 | 2-pin JST XH | 12 V, GND |
| J2 | 1×3 header, 2.54 mm | switched 5 V, GND, enable |

## Measured

One local run of the export and the external router.

| | |
| --- | --- |
| Nets closed | 12 of 13 |
| Open | 1 — `net_5` at `U1.3`, reaching 1 of 3 pads |
| Routed copper | 416.1 mm over 117 segments and 21 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **0** |
| `kicad-cli pcb drc` | **1 error**, 1 unconnected item, 1 warning |

Three consecutive exports produce a byte-identical board and schematic.

The single error is one clearance violation. This is the cleanest routed
board of the four.

## Limitations

- **One net does not close.** `net_5` is the buck's switch node, reaching 1 of
  its 3 pads. A switch node is the one net on this board where the routing
  shape matters to the circuit, so finishing it by hand is the right call
  rather than accepting whatever an automatic retry produces.
- One clearance violation remains.
- **No thermal analysis.** The buck dissipates and declares a thermal
  resistance, but nothing checks what the copper around it does with that.
- The divider ratio and the soft-start values are datasheet and convention,
  not measurement.
- No fabrication artifacts are produced; the package is refused as
  `UNTRUSTED / NOT FOR FABRICATION`.
