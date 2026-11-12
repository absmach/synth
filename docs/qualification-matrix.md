# Reference-board qualification matrix

Status: accepted, 2026-11-13. Covers #45, #95 and #98.

## The rule

A board is only release evidence when the same command sequence runs from one
commit and produces the same verdict:

```sh
synth check <board>.synth --fab --json
synth drc <board>.synth --pretty
synth export-kicad <board>.synth --out <dir> --gerbers --drill --step --pnp
```

`synth check --fab` is fail-closed on routing. A machine without an external
router reports `router_unavailable`, which is an **unavailable** check, not a
clean DRC. This is deliberate and is why the old matrix reported `drc=unknown`:
the router was never installed, not because the boards were dirty.

## Matrix status

| Board | Source | DRC | Fabrication | Evidence |
|---|---|---|---|---|
| `hello` | pass | needs router for any net, none for an empty board | unavailable without a router | `fixtures/designs/hello.synth` |
| Class A i2c sensor node | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-a/i2c-sensor-node` |
| Class A protected power entry | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-a/protected-power-entry` |
| Class B buck | pass | routed, return-via advisories recorded | review-required | `qualification/class-b/buck` |
| Class B USB-UART | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-b/usb-uart` |

## What CI must do

- Install `kicad-cli` **and** a router before running the matrix. A run without
  one is labelled unavailable, not a pass.
- Run the matrix for pull requests touching `synth-ir`, `synth-place`,
  `synth-router`, `synth-kicad`, `synth-drc` or `synth-cli`.
- Keep unsupported boards as negative cases so a new feature cannot silently
  widen a claim.
- Upload the `release.json`, routing report and Gerber hash on failure.

## Known gaps

- The empty `hello` board is not routed by the engine because it has no pads.
  It should report `routed` with zero copper rather than `router_unavailable`;
  that needs a first-class "nothing to route" path in `synth-router`.
- Two positive boards were failing the routed matrix at the time of #98
  (`hello` as above, and `b3_usb_peripheral`'s QFN escape). The QFN escape needs
  the coarser-pitch or via-in-pad route it names in its own diagnostic.
