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
| Size | 47.23 × 37.71 mm |
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
| Nets closed | **13 of 13** |
| Open | **0** |
| Routed copper | 274.8 mm over 101 segments and 17 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **0** |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, **0 warnings** |
| Routing verdict | `routed`, fabrication-ready |

Nothing is left for KiCad to report: no violation at any severity the run
includes, no unconnected pad, and no schematic-parity difference. This is the
cleanest routed board in the Class-A set.

## Limitations

- **U3 runs hot.** `W-SYNTH-THERMAL-001` estimates the AMS1117's junction at
  about 107 °C: 1360 mW through a declared 60 °C/W. That is an estimate from
  declared figures, not a simulation, and it is the figure that decides whether
  the 3V3 rail belongs on a linear regulator at all.
- **Seventeen layer changes have no ground return via within 1.53 mm**
  (`W-SYNTH-DRC-016`). Two layers, no plane: there is nowhere for the return
  current to follow the signal.
- **The schematic is not readable yet.** `C1` is drawn 208.7 mm from the `U1`
  it decouples, one net is a 128.3 mm drawn wire that should have been
  truncated to labels, and two GND flags point up instead of down.
- The divider ratio and the soft-start values are datasheet and convention,
  not measurement. Nothing checks what the copper around the buck does with
  the heat it dissipates.
- Nine of its parts carry no distributor identity, so the BOM is not orderable
  as it stands.
- No independent reviewer has signed off.
