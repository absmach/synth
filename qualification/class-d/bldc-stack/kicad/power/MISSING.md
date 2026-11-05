# Power board export state

The power board now has `bldc_power.kicad_pcb`, `bom.csv` and `pnp.csv`. The
export places the board, writes the un-routed baseline, and then hands it to
FreeRouting. That routing run hit the 1200 s router budget (53 nets on a
71.9 x 52 mm board) and was terminated, so this directory has:

- no routed candidate;
- a `timeout` run record in `../../reports/power.routing.json` and a
  `release_ready: false` manifest in `../../reports/release-power.json`;
- no Gerbers, drill files or fab quote.

The delivered PCB is an **un-routed placement record**, not a fabrication
artifact. Its placement is DRC-clean: 0 errors, 18 advisory silk warnings and
179 unconnected pads (the pads that have no copper yet). The mounting holes are
at the control's coordinates, `(2,2)`, `(51,2)`, `(2,30)` and `(51,30)`, and
the outline is framed so H1/H2/H3 sit 4.95 mm from its top/left edges. The
findings are in `../../reports/power.drc.json`.

`BOM_sourcing.md` at the board root remains the sourcing narrative.
