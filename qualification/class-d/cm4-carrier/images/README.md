# images — CM4 carrier (SYN-407)

- `schematic/` — `kicad-cli sch export svg` of the four-sheet schematic:
  `cm4_carrier.svg` (root), `cm4_carrier-Power.svg`, `cm4_carrier-CM4.svg`,
  `cm4_carrier-Interfaces.svg`, `cm4_carrier-Peripherals.svg`. The PNGs are
  `kicad-cli sch export pdf` + `pdftoppm -r 150` (root at 7243 px wide).
  The SYN-420 sheet-fill layout gives Power/CM4/Interfaces A1 pages and
  Peripherals an A2 page, at 91-93% content fill.
- `pcb-3d-top.png`, `pcb-3d-angled.png` — `kicad-cli pcb render` of the
  **routed 6-layer HDI** board.
- `pcb-top-copper.png`, `pcb-bottom-copper.png` — `kicad-cli pcb export pdf`
  plots of the same routed 6-layer HDI board. They show the routed copper and
  the GND planes.
- The PCB was produced with `synth export-kicad --pcb-only`, which writes the
  PCB side without the slow schematic layout. See
  `../agent-run/VERIFICATION_REPORT.md`.
