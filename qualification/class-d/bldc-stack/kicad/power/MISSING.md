# Missing artifacts in this directory

The power-board export did not finish. `synth export-kicad` wrote the schematic,
project, symbol library, `.kicad_dru` and `sym-lib-table`, then spent more than
40 minutes in the routing stage and was killed before it wrote:

- `bldc_power.kicad_pcb`
- `bom.csv`
- `pnp.csv`
- `reports/release-power.json`
- `reports/power.routing.json`

The cause is the same FreeRouting adapter failure recorded for the control board
in `../../agent-run/VERIFICATION_REPORT.md`: `tools/freeroute_autoroute.py`
exits with a KiCad `PROPERTY_ENUM` assert. The same failure reproduces on the
committed Class-B buck board.

Do not treat the schematic as a routed or fabrication-ready artifact. The
`BOM_sourcing.md` file at the board root lists the power-board parts; it is not
a substitute for the missing `bom.csv`.
