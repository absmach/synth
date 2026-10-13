# Release notes

## Unreleased

### Stackup-driven signal integrity

- A declared `stackup { … }` is now flattened into per-copper-layer geometry (dielectric height, permittivity, copper thickness), classified microstrip (outer) or stripline (inner), with layer-aware impedance/width solvers and standard 2/4/6-layer defaults when no stackup is declared (`synth-geometry`).
- `netclass` gained three attributes:
  - `impedance <value>ohm` — a single-ended controlled-impedance target, checked against the routed width.
  - `current <value>A` — the net current, checked against the routed width by the IPC-2221 ampacity curve.
  - `plane` — flood a copper plane for the class's nets on the inner layers (see below).
- New DRC codes:

  | Code | Meaning |
  | --- | --- |
  | `E-SYNTH-DRC-012` | routed geometry off the impedance target (pair or single-ended) |
  | `W-SYNTH-DRC-013` | parallel traces closer than a dielectric height (edge coupling) |
  | `E-SYNTH-DRC-014` | differential pair over its declared `max_skew` |
  | `E-SYNTH-DRC-015` | trace too narrow for its declared `current` |
  | `W-SYNTH-DRC-016` | signal via with no ground return via |
  | `W-SYNTH-DRC-017` | rail IR drop over budget |

  `W-` codes are advisory and never block a release; the DRC report now carries a severity dimension (`is_blocking`, warning-aware `is_clean`).
- Independent validation now reads a zone's **poured** copper (`filled_polygon`), not only its outline, and records `plane_splits`: signal tracks whose adjacent plane layer pours copper but does not cover the track, so it has no return path under it. Advisory.
- `E-SYNTH-STACKUP-004/005` and `E-SYNTH-IMPEDANCE-001/002` now cover a netclass's single-ended impedance too.

### Planes

- Ground and split-ground planes are still flooded automatically. A `netclass "PWR" { plane }` now floods a plane for each member net as well.
- Ground keeps the outer layers and the declared power plane takes the inner ones, so a full-board ground flood cannot starve a power plane on a shared layer. Without a declared power plane, ground floods every layer as before.
- Each power pad gets a stitching via beside it (reached by a short fanout stub) so the inner plane reaches the outer pads without via-in-pad.

### Routing

- **KiCadRoutingTools is the default router.** `--router free-routing` selects the managed FreeRouting JAR. On a machine with no KiCadRoutingTools checkout, discovery falls back to the `~/.local/share/synth/kicad-routing-tools` location `scripts/setup-routing-engines.sh` installs; if neither engine is available the run reports `router_unavailable` (never a silent fallback), so select `--router free-routing` or run the setup script.

### Placement

- The placer takes a deterministic seed: `synth export-kicad --place-route-iterate` (and `synth_export`'s `place_route_iterate`) routes each candidate placement — density-margin and seed variants — and keeps the one that routes best, judged by the independent checks. Best-of-N per candidate is used when `--best-of` is set, and the winner's hardest-net order is reported for a retry.
