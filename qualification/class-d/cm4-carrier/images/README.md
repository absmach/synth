# images — CM4 carrier (SYN-407)

- `schematic/` — `kicad-cli sch export svg` of the four-sheet schematic:
  `cm4_carrier.svg` (root), `cm4_carrier-Power.svg`, `cm4_carrier-CM4.svg`,
  `cm4_carrier-Interfaces.svg`, `cm4_carrier-Peripherals.svg`. These are
  vector images, not PNG; no PNG rasterizer is installed in this
  environment.
- Top copper, bottom copper and 3D renders are **missing**: they need a
  `.kicad_pcb`, and the PCB export did not finish. See
  `../agent-run/VERIFICATION_REPORT.md`.
