# Final exports

The three exports of the final source and sidecar; `reproduce/reproduce.sh` makes them again. The schematic, project, symbol, BOM, placement and un-routed board files are byte-identical across the three exports. The routed board differs in the random ids the router and KiCad write, and the Gerber, drill and job files in the creation time of their headers; both are equal after normalization, with those masked (see the README).

| export | router state | DRC errors | unconnected | blocking findings | vias | track segments |
|---|---|---|---|---|---|---|
| 1 | routed | 0 | 0 | 0 | 73 | 602 |
| 2 | routed | 0 | 0 | 0 | 73 | 602 |
| 3 | routed | 0 | 0 | 0 | 73 | 602 |
