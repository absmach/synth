# reports — CM4 carrier (SYN-407)

What is here:

- `cm4_carrier.kicad-erc.json` — `kicad-cli sch erc --severity-all` on the
  exported schematic. 93 violations: 13 errors, 80 warnings. The errors are
  listed in `../agent-run/VERIFICATION_REPORT.md`.

What is missing and why:

- `cm4_carrier.kicad-drc.json` — no PCB was exported. The export wrote the
  schematic at about 29 minutes and was killed at the 30-minute test budget
  before the PCB/routing stage.
- `cm4_carrier.routing.json`, `.connectivity.json`, `.drc.json` — no routing
  run.
- `release.json`, `release-fab.json` — no release or fab package.
- `registry-qualify.txt` — each authored part was qualified individually;
  the only blocking finding is `E-SYNTH-QUAL-008` (no reviewer), which stays
  open by design.

The `kicad/` directory contains the four-sheet schematic, the embedded symbol
library, the project file and the DRC rule file. It has no `.kicad_pcb`,
`bom.csv` or `pnp.csv`.
