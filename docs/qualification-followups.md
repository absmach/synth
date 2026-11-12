# Qualification follow-ups

Status: accepted, 2026-11-13. Covers #396 and #423.

## Class B buck board (#396)

The buck board is an approved **experimental** contribution, not a Class B
release. Remaining release blockers:

1. Eleven signal vias have no compliant ground return via. The routed artifact
   is regenerated only after the return-path fix lands.
2. The LMR38010 thermal-via array is 2x3; the manufacturer's example is 4x3.
   It needs the declared filled/capped process and a regenerated DRC record.
3. The final schematic needs the professional-readability pass (no
   wire-through-text, no ambiguous crossings).
4. Independent power/SI/PI review has not been recorded for surge, thermal,
   converter stability, derating, switch-node/EMI, grounding/feedback or
   power-good behaviour.

Until all four close, `synth check --fab` is expected to be
`review_required` and `QUALIFICATION.md` says so.

## Class A I2C sensor node (#423)

The package is approved against its final Gerber hash. The remaining findings
are documented rather than hidden:

- twelve return-via advisories are accepted for slow nets (`edge_rate slow`);
- two dangling `U2` branches are removed by the post-route stub prune when the
  binding enables it, and the run record names them when it cannot;
- the BME680 SDO-to-ground warning is an intentional I2C address strap;
- footprint-library mismatch warnings are removed on new exports because the
  silkscreen pass now preserves library footprint bytes;
- the 0.127 mm stubs and the density remain a known limitation of the fitted
  outline on that board.

Each of these is machine-readable in the routing/ERC records, so a reviewer
does not have to trust prose.

## Rule

A qualification record is not a claim that a board is good. It is a list of
what was checked, on which commit, and what remains unverified.
