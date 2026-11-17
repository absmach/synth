# Class-A I2C sensor node: limitation disposition

Baseline: the approved package in this directory and its Gerber hash. This page
records how each follow-up from #423 is represented in the machine-readable
records today.

| Limitation | Disposition | Evidence |
|---|---|---|
| 12 return-via advisories | Accepted as slow-net advisories. Slow nets declare `edge_rate slow`; the router records the missing return via in `advisories`, not `blocking_reasons`. | `export/i2c_sensor_node.routing.json` |
| Two dangling `U2` branches (`GPIO_PB9_MCU`, `GPIO_PA4_MCU`) | Removed by the post-route dangling-stub prune; a remaining branch is a blocking finding naming the net and segment. | router finalize verdict |
| BME680 SDO-to-ground warning | Intentional I2C address strap. The part is declared with the strap in the source, and the warning is dispositioned in the qualification notes. | `i2c_sensor_node.synth`, README |
| 55 footprint-library mismatches | Removed on new exports: the silkscreen pass preserves library footprint bytes, so `lib_footprint_mismatch` is not manufactured by the exporter. | `docs/diagnostics/E-SYNTH-DRC-*`, router silkscreen pass |
| 0.127 mm U2 stubs / density | Remains a known fitted-outline limitation. It is not hidden: the routing/ERC records expose it, and the placement metric reports courtyard fill. | routing report, `PlacementDescription.courtyard_fill_pct` |

The package is not re-qualified by this note. A new qualification run must
regenerate the hashes and attach the refreshed DRC/ERC evidence.
