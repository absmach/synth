# Power board export state

The power board now has `bldc_power.kicad_pcb`, `bom.csv` and `pnp.csv`. The
export places the board, writes the un-routed baseline, and then hands it to
FreeRouting. That routing run exceeded the 30-minute cap (53 nets on a
72 x 48 mm board) and was stopped, so this directory has:

- no routed candidate;
- no `reports/power.routing.json` or `reports/release-power.json`;
- no Gerbers, drill files or fab quote.

The delivered PCB is an **un-routed placement record**, not a fabrication
artifact. Its placement is DRC-clean: 0 errors, 0 warnings and 179 unconnected
pads (the pads that have no copper yet). The findings are in
`../../reports/power.drc.json`.

`BOM_sourcing.md` at the board root remains the sourcing narrative.
