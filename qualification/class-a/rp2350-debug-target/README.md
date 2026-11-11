# RP2350 debug target

A minimal RP2350 board: USB-C power into a 3V3 LDO, the MCU's own crystal, a
tactile reset on RUN, a 3-pin SWD header, QSPI boot flash and one status LED.
No USB data path — that belongs to a faster class.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 77.90 × 62.95 mm |
| Layers | 4 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 25 |
| Nets | 20 |
| Source | `rp2350_debug_target.synth` |

## Why four layers

The RP2350's QFN-60 is a 0.4 mm pitch package leaving 0.200 mm between pads,
and a via on this process needs 0.687 mm between centres. The drill that would
fit between two pins is 0.013 mm, which no fabricator offers. Synth's escape
analysis and KiCadRoutingTools reach that conclusion independently.

Two layers cannot route this part. Four can, and the cost is in *Measured*
below: the escape still needs vias inside pads.

## Connectors

| Ref | Part | Carries |
| --- | --- | --- |
| J1 | USB-C receptacle | 5 V in, CC resistors to 5.1k |
| J2 | 1×3 header, 2.54 mm | SWCLK, SWDIO, GND |

## Measured

One local run of the export and the external router. No figure here came from
anywhere else.

| | |
| --- | --- |
| Nets closed | 19 of 20 |
| Open | 1 — `net_4` at `U2.49`, reaching 1 of 19 pads |
| Routed copper | 628.7 mm over 289 segments and 35 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| `kicad-cli pcb drc` | 14 errors, 1 unconnected item, 8 warnings |

Three consecutive exports produce a byte-identical board and schematic.

### DRC detail

| Severity | Finding | Count |
| --- | --- | --- |
| error | via diameter | 14 |
| warning | library footprint mismatch | 6 |
| warning | dangling track | 1 |
| warning | dangling via | 1 |

All fourteen errors are the same disagreement, and it is Synth's with itself
rather than the engine's with the board: the escape vias are sized from the
process floor at 0.56 mm, while the net classes written into the exported
project state a 0.6 mm minimum. The copper is above the fabricator's floor
either way.

## Limitations

- **One net does not close.** `net_4` reaches 1 of its 19 pads. That is
  ground on a 4-layer board with no plane declared, and it is routing a human
  would finish by hand.
- **Two vias land inside pads** and via-in-pad is not declared, so the package
  is refused. On a 0.4 mm pitch QFN this is the escape arithmetic above, not a
  router defect; declaring `requires via_in_pad` would accept them at the cost
  of a filled-via process.
- The fourteen via-diameter errors above are a disagreement between two parts
  of Synth, not a fabrication risk.
- No fabrication artifacts are produced; the package is refused as
  `UNTRUSTED / NOT FOR FABRICATION`.
- Six `lib_footprint_mismatch` warnings compare the board's embedded
  footprints against the installed libraries; they depend on library version
  and should be read from CI rather than a local run.
