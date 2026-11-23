# Reference board programs

Status: accepted, 2026-11-13. Covers #69, #260 and #263.

These are product-shaped acceptance programs, not unit tests. A board is done
when its source, placement sidecar, exported KiCad project and qualification
record all reproduce from one commit.

## Class A integration board: irrigation controller (#263)

- STM32G030, two valve outputs, soil and flow inputs, I2C sensor header and an
  MP2307 buck.
- Pinned floorplan: the valve, soil and flow connectors keep the edges their
  hard hints name; a corner mounting hole is not allowed to push them inward.
- Acceptance: `synth check --fab --json` is `release_ready`, DRC has no danger
  rows, and every connector courtyard touches Edge.Cuts on its hinted edge.

## MCU + SWD/JTAG + crystal + reset + status LED (#260)

- Met by the irrigation controller: SWD header, crystal, reset button and a
  status LED are all on that board.
- The RP2350 QFN-60 is explicitly **not** the Class A vehicle. Its escape is a
  Class C problem (#320); the part map itself is correct (#152).
- Acceptance: the SWD header, crystal load capacitors and reset pull-up are
  placed within their declared `near:` distances, and the status LED current
  is computed from its resistor.

## Precision analog front end (#69)

- PT100/ADS124S08/REF5025 class front end with a split AGND/DGND intent and a
  declared single join point (see `docs/ground-domains.md`).
- Blocked on a reference layout that demonstrates the join and the ADC input
  filter; until then its rows are recorded as **unverified**, not omitted.
- Acceptance: the analog and digital ground domains are declared, the join is
  a net tie, and every signal crossing the boundary has a stitching path.

## What is deliberately not claimed

A board program is not evidence that a *family* of boards works. Each row names
the exact commit, the router version and the Gerber hash it was judged on.
