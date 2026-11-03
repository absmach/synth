# images — CM4 carrier (SYN-407)

- `schematic/` — `kicad-cli sch export svg` of the four-sheet schematic:
  `cm4_carrier.svg` (root), `cm4_carrier-Power.svg`, `cm4_carrier-CM4.svg`,
  `cm4_carrier-Interfaces.svg`, `cm4_carrier-Peripherals.svg`. These are
  vector images, not PNG; no PNG rasterizer is installed in this
  environment.
- `pcb-3d-top.png`, `pcb-3d-angled.png` — `kicad-cli pcb render` of the
  **unrouted** placement (the board is placed, not routed).
- `pcb-top-copper.png`, `pcb-bottom-copper.png` — `kicad-cli pcb export pdf`
  plots of the same unrouted board. They show pads and the outline, not
  traces; there is no routed copper yet.
- The PCB was produced with `synth export-kicad --pcb-only`, which writes the
  PCB side without the slow schematic layout. See
  `../agent-run/VERIFICATION_REPORT.md`.
