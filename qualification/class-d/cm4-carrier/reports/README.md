# reports — CM4 carrier (SYN-407)

What is here:

- `cm4_carrier.kicad-erc.json` — `kicad-cli sch erc --severity-all` on the
  exported schematic. 67 violations: 1 error, 66 warnings. The error is the
  `RTC_BAT` power flag; see `../agent-run/VERIFICATION_REPORT.md`.

What is missing and why:

- `cm4_carrier.kicad-drc.json` — `kicad-cli pcb drc --severity-all
  --refill-zones` on the **unrouted** PCB: 153 violations, 359 unconnected
  items. The unconnected items are the unrouted nets; this is not a routed
  board.
- `cm4_carrier.routing.json`, `.connectivity.json`, `.drc.json` — no routing
  run.
- `release.json`, `release-fab.json` — no release or fab package.
- `registry-qualify.txt` — each authored part was qualified individually;
  the only blocking finding is `E-SYNTH-QUAL-008` (no reviewer), which stays
  open by design.

The `kicad/` directory contains the four-sheet schematic, the embedded symbol
library, the project file and the DRC rule file. It has no `.kicad_pcb`,
`bom.csv` or `pnp.csv`.
