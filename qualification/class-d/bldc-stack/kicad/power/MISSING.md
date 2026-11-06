# Power board export state

The power board now has `bldc_power.kicad_pcb`, `bom.csv` and `pnp.csv`. The
export places the board, writes the un-routed baseline, and then hands it to
FreeRouting. That routing run hit the 1200 s router budget (53 nets on a
72 x 52 mm board) and was terminated, so this directory has:

- no routed candidate;
- a `timeout` run record in `../../reports/power.routing.json` and a
  `release_ready: false` manifest in `../../reports/release-power.json`;
- no Gerbers, drill files or fab quote.

The delivered PCB is an **un-routed placement record**, not a fabrication
artifact. Its placement is DRC-clean: 0 errors, 26 advisory warnings and 179
unconnected pads (the pads that have no copper yet). The board declares
`outline 72mm x 52mm`; J_B2B is flush at 3.12 mm from the right edge,
J_PWR/J_MOTOR are flush and close together on the left edge, and the mounting
holes are at `(2,2)`, `(62.4,2)`, `(2,49)` and `(62.4,49)`. The findings are in
`../../reports/power.drc.json`.

`BOM_sourcing.md` at the board root remains the sourcing narrative.
