# RP2350 routed example

`rp2350_devboard_freerouting_clean.kicad_pcb` is a pre-generated compact KiCad
review artifact associated with the workflow in
[`docs/rp2350-agent-workflow.md`](../../docs/rp2350-agent-workflow.md). 

Reference dimensions: approximately **84.5 × 73.1 mm**.

![Rendered RP2350 routed example](rp2350_devboard_freerouting_clean.png)

Open the [KiCad board](rp2350_devboard_freerouting_clean.kicad_pcb) for an
interactive review.

The companion DRC report records the current review state. The authoritative
KiCad check currently reports **80 DRC violations** and **18 unconnected
items**. The 18 unconnected entries are zone-to-zone ground-island reports,
not ordinary signal-pad pairs, but they are still violations. This is an
example and review fixture, not production sign-off; silkscreen and footprint
library warnings also require engineering review.
