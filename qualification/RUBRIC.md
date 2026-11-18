# Qualification review rubric

Use this when a human reviews a board package. Every row must be marked
**proved**, **limited** or **unsupported**, with the artifact that shows it.

1. **Source and netlist** — symbols resolve, pin maps match, required pins are
   connected, no duplicate refdes, BOM matches the netlist.
2. **Schematic** — readable at a sensible zoom, no overlapping text, labels on
   one wire, every net named or intentionally auto-named.
3. **Placement** — connectors flush on their hinted edges, decoupling within
   its declared distance, courtyard fill reported, no part under a keepout.
4. **Routing and SI** — declared pair gap/skew, length budgets, return vias for
   fast edges, plane continuity, no via in a pad unless the process allows it.
5. **Fabrication** — Gerber/drill/position/STEP open in KiCad, DRC clean, DFM
   danger rows reviewed, mask and silkscreen limits met.
6. **Thermal and power** — dissipation estimated from the board load, derating,
   thermal via/copper path, regulator stability and sequencing.
7. **Mechanical** — board outline, mounting holes, connector access, assembly
   clearance, 3D render.
8. **Evidence** — hashes, commands, router/KiCad versions, reviewer name and
   date, and an explicit list of what Synth did not prove.

A package with an unmarked row is not qualified.
