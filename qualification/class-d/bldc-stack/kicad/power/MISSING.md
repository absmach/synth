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
artifact. Its placement is DRC-clean: 0 errors, 21 advisory warnings and 179
unconnected pads (the pads that have no copper yet). The board declares
`outline 72mm x 52mm`; the four mounting holes are at its corners, `(2,2)`,
`(69,2)`, `(2,49)` and `(69,49)`, and J_B2B stays at `(56.88,16)` so the
connector still lines up with the control. The findings are in
`../../reports/power.drc.json`.

`BOM_sourcing.md` at the board root remains the sourcing narrative.
