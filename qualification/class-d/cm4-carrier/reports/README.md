# reports — CM4 carrier (SYN-407)

What is here:

- `cm4_carrier.kicad-erc.json` — `kicad-cli sch erc --severity-all` on the
  exported schematic. 67 violations: 0 errors, 67 warnings. The old
  `RTC_BAT` power-pin error is cleared; the remaining warnings are 66
  `lib_symbol_issues` and one `multiple_net_names` (`FB_5V`/`VCC_5V`). See
  `../agent-run/VERIFICATION_REPORT.md`.

- `cm4_carrier.kicad-drc.json` — `kicad-cli pcb drc --severity-all
  --refill-zones` on the **routed** 6-layer HDI PCB: 0 violations, 0
  unconnected items.

What is missing and why:

- `cm4_carrier.routing.json`, `.connectivity.json`, `.drc.json` — no routing
  run.
- `release.json`, `release-fab.json` — no release or automated fab package;
  a manual package was generated locally and delivered to the user; it is not committed/release evidence.
- `registry-qualify.txt` — each authored part was qualified individually;
  the only blocking finding is `E-SYNTH-QUAL-008` (no reviewer), which stays
  open by design.

The `kicad/` directory contains the four-sheet schematic, the embedded symbol
library, the project file, the DRC rule file, the **routed** `.kicad_pcb`,
`bom.csv` and `pnp.csv`. `cm4_carrier.synth.kicad_pcb` is the unrouted baseline.
