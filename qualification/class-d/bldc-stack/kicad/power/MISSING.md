# Power board export state

The power board now has `bldc_power.kicad_pcb`, `bom.csv` and `pnp.csv`. The
export places the board, writes the un-routed baseline, and then hands it to
FreeRouting. That routing run exceeded the 30-minute cap (53 nets on a
72 x 48 mm board) and was stopped, so this directory has:

- no routed candidate;
- no `reports/power.routing.json` or `reports/release-power.json`;
- no Gerbers, drill files or fab quote.

The delivered PCB is an **un-routed placement record**, not a fabrication
artifact. `kicad-cli pcb drc` reports 178 unconnected items and 32 placement
and clearance errors. The hand-pinned sidecar has overlapping courtyards in
three clusters (`J_MOTOR`/`U_CS1`/`R_SH_U`, `J_PWR`/`Q_REV`, `H3`/`C_BULK2`)
and the TO-92 `Q_FAULT` pads are closer than the 0.3 mm `PWR` clearance. The
findings are in `../../reports/power.drc.json`; the board is rendered for
review only.

`BOM_sourcing.md` at the board root remains the sourcing narrative.
