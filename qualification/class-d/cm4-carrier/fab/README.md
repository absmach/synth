# fab — CM4 carrier (SYN-407)

**NOT FOR FABRICATION.** These artifacts were generated from the routed
board for review only. The authored parts are unreviewed (`reviewed_by = ""`),
JLCPCB has not confirmed the HDI via-in-pad fill/cap for this stackup, and no
DFM review was done.

- `gerbers/` — RS-274X, `kicad-cli pcb export gerbers --no-protel-ext
  --subtract-soldermask` (all six copper layers, masks, silkscreen, paste and
  Edge.Cuts).
- `drill/` — Excellon PTH/NPTH split plus PDF drill maps,
  `kicad-cli pcb export drill --excellon-separate-th --generate-map
  --map-format pdf`.
- `bom.csv`, `pnp.csv` — from the routed export.
- The routed board is `../kicad/cm4_carrier.kicad_pcb`; its DRC is
  `../reports/cm4_carrier.kicad-drc.json` (0 violations, 0 unconnected).
- The automated `synth check --fab` path is blocked by `synth validate`
  timing out; these artifacts were generated manually and are not release
  evidence.
- HDI process: 0.1 mm drill, 0.25 mm via, 0.075 mm annular ring, 0.1 mm
  track/clearance. The CM4 DF40 underpad vias require IPC-4761 Type VII
  (filled + capped + plated); state this on the fab drawing.
