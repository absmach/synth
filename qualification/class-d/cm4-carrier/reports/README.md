# reports — CM4 carrier (SYN-407)

What is here:

- `cm4_carrier.kicad-erc.json` — `kicad-cli sch erc --severity-all` on the
  exported schematic. 68 violations: 1 error, 67 warnings. The error is the
  `RTC_BAT` power flag; see `../agent-run/VERIFICATION_REPORT.md`.

- `cm4_carrier.kicad-drc.json` — `kicad-cli pcb drc --severity-all
  --refill-zones` on the **routed** 6-layer HDI PCB: 0 violations, 0
  unconnected items.

What is missing and why:

- `cm4_carrier.routing.json`, `.connectivity.json`, `.drc.json` — no routing
  run.
- `release.json`, `release-fab.json` — no release or automated fab package;
  a manual `fab/` package (not release evidence) is in `../fab/`.
- `registry-qualify.txt` — each authored part was qualified individually;
  the only blocking finding is `E-SYNTH-QUAL-008` (no reviewer), which stays
  open by design.

The `kicad/` directory contains the four-sheet schematic, the embedded symbol
library, the project file, the DRC rule file, the **routed** `.kicad_pcb`,
`bom.csv` and `pnp.csv`. `cm4_carrier.synth.kicad_pcb` is the unrouted baseline.
