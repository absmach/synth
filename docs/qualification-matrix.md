# Reference-board qualification matrix

Status: accepted, 2026-11-13. Covers #45 and #98; #95 is deferred because this tree has no Synth-native router.

## The rule

A board is only release evidence when the same command sequence runs from one
commit and produces the same verdict:

```sh
synth check <board>.synth --json
synth drc <board>.synth --pretty
synth export-kicad <board>.synth --out <dir> --gerbers --drill --step --pnp
```

`synth drc` routes with the selected external engine, so its DRC column is a
verdict when that engine is installed and an availability report when it is
not:

- when an external engine is configured and available, its copper is routed,
  validated independently and DRC-checked as before;
- when no external engine is installed, the stage reports
  `router_unavailable` and the DRC column stays `unknown` — never a pass;
- a board with nothing to route (no nets, or every net on one pad) is a
  validated empty route: zero copper, real DRC counts, `pass` when KiCad
  agrees (#98).

The pin: `SYNTH_QUALIFY_ROUTER=kicad-routing-tools` names the engine a
qualification run records. A pin outranks the `--router` flag, so a pinned run
gets the pinned engine or an honest `router_unavailable` — never copper from
an engine its record does not name.

Manufacturing is fail-closed on routing: `synth check --fab` and
`synth export-kicad` need a validated external route. Without an engine
installed the manufacturing stage reports `unknown`/`router_unavailable`, which
is an **unavailable** check, not a clean one.

## Matrix status

| Board | Source | DRC | Fabrication | Evidence |
|---|---|---|---|---|
| `hello` | pass | pass (empty route, no engine needed) | unavailable without a router | `fixtures/designs/hello.synth` |
| Class A i2c sensor node | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-a/i2c-sensor-node` |
| Class A protected power entry | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-a/protected-power-entry` |
| Class B buck | pass | routed, return-via advisories recorded | review-required | `qualification/class-b/buck` |
| Class B USB-UART | pass | routed by KiCadRoutingTools | pass with router | `qualification/class-b/usb-uart` |

## What CI does

The `Qualification Matrix (release)` job in `.github/workflows/ci.yml` runs
`scripts/run_qualification_matrix.py` — one deterministic `synth check`
per row of `qualification/matrix.toml`, with each row's recorded engine set
in `SYNTH_QUALIFY_ROUTER` and the check gate's per-stage budget raised via
`SYNTH_CHECK_STAGE_BUDGET_SECS` (a corpus route takes minutes, so a
parse-and-print budget would report `unknown` for a board that is fine):

- `kicad-cli` and the routing engines are installed first, KiCadRoutingTools
  pinned to the revision the corpus was routed with, so each verdict names
  the engine that produced it; a row can record a different engine for its
  fab command (`fab_router`) when the corpus evidence does;
- rows that declare `expect_fab` also run the fab command, and their
  manufacturing stage must come out exactly as declared; rows that declare
  `user_registry` run with it loaded, the way the board's own reproduce
  script does;
- every row records the KiCad version, the design's SHA-256, the artifact
  hashes the package reports and which native reports were present;
- a row whose directory holds a `SHA256SUMS` covering the design source has
  that source verified, so an accidental fixture edit fails the matrix
  instead of quietly changing what a pass means;
- a row the corpus cannot run on this code declares `skip_reason`: it is
  recorded in the results and named in the job log, never silently dropped;
- results land in `qualification-matrix-results/` as one JSON document per
  row plus `results.json`, uploaded as reviewable artifacts on failure;
- the job runs on pull requests that touch the compiler, registry, layout,
  routing, KiCad-export or release paths (a job-level path check, so the
  other CI jobs keep running on every pull request).

## What only a human can check (review handoff)

The matrix proves the pipeline: source → schematic → PCB → native ERC/DRC →
manufacturing package, with hashes and machine-readable records. It cannot
prove the things a board review exists for. Every qualification record
(`qualification/*/QUALIFICATION.md`) carries these as open gates, and a board
is not "qualified" until a named person has signed each one:

- **Visual review.** Schematic readability (no wire-through-text, no
  ambiguous crossings, readable silkscreen labels), footprint land patterns
  against the datasheet drawings, and the routed copper's sanity — the
  renderer and the schematic visual loop help, but the judgement is human.
- **Assembly review.** BOM orderability and alternates, pick-and-place
  orientation and fiducials, panelization and depaneling, solder-joint risk
  (thermal reliefs, paste coverage on large pads), and parts with no source
  at the chosen fab.
- **SI/PI review.** Signal integrity the geometry gates cannot see: edge
  rates against trace topology, return-path continuity for fast edges
  (the advisories the router records are advice, not proof), decoupling
  adequacy, and supply sequencing. The buck board's return-via advisories and
  the sensor node's accepted `edge_rate slow` advisories are examples that
  need this sign-off.
- **Thermal review.** Regulator and converter dissipation against the
  copper's real ability to spread it, the thermal via arrays (the buck
  board's 2×3 array against the manufacturer's 4×3 example), and ambient
  assumptions for the intended enclosure.
- **Fabrication review.** The process claims: layer stack, controlled
  impedance, filled/capped vias, surface finish, soldermask and silkscreen
  choices — and, when the board declares a process requirement, that the
  ordered panel actually carries it.

The gate verdict in `release.json` covers only Synth's own routing and export
gates; its `gate_scope` field says so. Nobody may widen that scope without a
human record.

## Corpus coverage

`qualification/` holds the versioned corpus the matrix runs. Verified on
2026-11-13 (KiCad 10.0.8, KiCadRoutingTools 9399bfb, FreeRouting 2.4.1):

| Row | Class | Runs as |
|---|---|---|
| `class-a/negative-broken-mcu-board` | A | negative test: the source gate must fail with the planted diagnostics |
| `class-a/i2c-sensor-node` | A | cannot run: the route is rejected (99 blocking findings; see the row's `skip_reason`) |
| `class-a/protected-power-entry` | A | cannot run: no route verdict re-established within budget (see the row's `skip_reason`) |
| `class-b/buck` | B | cannot run: the DRC stage is unknown and the manufacturing gate fails on this branch (see the row's `skip_reason`) |
| `class-b/usb-uart` | B | cannot run: the source gate rejects it with E-SYNTH-POWER-010 (see the row's `skip_reason`) |
| `class-b/rtd-afe` | B | cannot run: the route does not validate (13 blocking findings; see the row's `skip_reason`) |

A row that cannot run is recorded by the runner and named in the job log;
its `skip_reason` says exactly what blocked it and what would lift it. The
Class C and Class D rows the capability classes define have no boards in the
corpus yet; when a board lands, it gets a row here and the matrix runs it —
no new harness is needed. `hello` is a fixtures board, not a corpus row; its
trivial empty route is covered by the CLI integration tests.
