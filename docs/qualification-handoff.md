# Reference board qualification

## What the matrix is

`qualification/matrix.toml` is a checked-in corpus of boards, each declared
with a capability class (A–D), a status, and the result the deterministic
release command must produce. CI runs the same command a human would —
`synth check <board> --json` — and fails when any board stops matching its
declaration.

| Status | Meaning |
| --- | --- |
| `qualified` | The declared envelope is proven for this board by automation and a recorded human review. |
| `experimental` | A positive case: the board is in the corpus to demonstrate an envelope, and no human has signed off on its artifacts. Not a launch claim. Whether the release command passes today is recorded in `expect`, which is a measurement rather than a claim. |
| `unsupported` | A negative test. The board is expected to fail, and the diagnostics it must report are named. |

The gate fails in **both** directions. A board that was passing and starts
failing is a regression. A board declared `unsupported` that starts passing is
a launch claim that grew without anyone declaring it, which is the failure this
matrix exists to catch. Either way the fix is to understand the change first
and edit `matrix.toml` second.

### Where the corpus stands

No board in the corpus clears the gate today, and every `expect` says so. The
three real boards route completely — no open nets — and then fail
`kicad-cli pcb drc` on the board's own declared rules: the exported
`.kicad_dru` carries only the manufacturer edge and hole clearances, so the
per-netclass clearance and the minimum via diameter never reach the external
engine, which routes to the global floor instead. `hello`, which has nothing to
route, fails because the engine exits 1 on an empty board. The manufacturing
stage fails on every board whose source passes, because no shipped part
carries a recorded reviewer.

None of that is a reason to soften the declarations. A matrix whose rows all
read `fail` is doing its job: it names the four distinct causes, and the day
one of them is fixed the gate will say so by failing in the other direction.

## Running it

```bash
# The release-blocking gate. Use a release build: a debug DRC on a real board
# takes about ten times as long and overruns the stage budget.
cargo test --release -p synth-cli --test qualification_matrix -- --ignored --nocapture

# Also exercise the manufacturing stage. Needs KiCad installed.
SYNTH_QUALIFY_FAB=1 cargo test --release -p synth-cli \
  --test qualification_matrix -- --ignored --nocapture
```

### A router is not optional

Synth does not route copper itself; it drives an external engine and then
validates what the engine wrote. With no engine available `synth drc` refuses
to route, emits no DRC counts, and the DRC stage reports `unknown` — which is
not a pass. The gate then fails for every board, including the empty one, for
a reason that has nothing to do with any design.

`SYNTH_QUALIFY_ROUTER` selects the engine for the matrix, and `synth check`
passes it down to the `drc` and `export-kicad` stages it runs. CI sets it to
`kicad-routing-tools` and installs a pinned KiCadRoutingTools checkout;
freerouting is the reference baseline to compare against rather than the
engine the gate depends on. Left unset, `synth check` uses its own default,
which is freerouting and wants a JAR and a JVM.

```bash
SYNTH_QUALIFY_ROUTER=kicad-routing-tools \
KICAD_ROUTING_TOOLS_REPO=/path/to/KiCadRoutingTools \
SYNTH_QUALIFY_FAB=1 cargo test --release -p synth-cli \
  --test qualification_matrix -- --ignored --nocapture
```

`synth routers` reports which engines a machine has and why each one is or is
not usable. The results file records which engine ran, so a result is
attributable to the thing that laid the copper.

Results land in `target/qualification/results.json`. The file records the KiCad
version, a SHA-256 of the registry manifest, every stage status per board, the
error-level diagnostics each board reported, and hashes for the schematic,
board and BOM — so a result that changes is attributable to a compiler change, a
registry edit, or a different KiCad, rather than being a mystery.

The output behind those results lands in `target/qualification/boards/<id>/`:
`check.json` for every board, and under `--fab` the whole export in `release/`
(schematic, PCBs, DRC report, Gerbers, drill files, STEP, pick-and-place) with
the native evidence in `verification.json`. CI uploads the results and these
directories on every run, passing or failing, and keeps them for 14 days, so a
failure can be inspected without re-running it. `synth check --artifacts-dir
<DIR>` is what keeps them; a board that fails at source never reaches the export
and has only its `check.json`.

Stage budgets are configurable when a board needs more time:
`SYNTH_CHECK_SOURCE_TIMEOUT_SECS`, `SYNTH_CHECK_DRC_TIMEOUT_SECS`,
`SYNTH_CHECK_FAB_TIMEOUT_SECS`.

### Which output hashes are pinned

The schematic and the BOM are. Each row carries `expect_artifacts`, the
SHA-256 of the `.kicad_sch` and `bom.csv` the manufacturing stage wrote for
that board, and the fab job compares them exactly in both directions: a pinned
file that changed or was not written fails, and so does a schematic or BOM the
board now produces that no row declares. Both files are written by this
compiler from the board and the registry alone, and CI runs on different
commits produce the same bytes for them, so a moved hash means the board now
compiles to something else. A board whose source stage fails never reaches
manufacturing and pins nothing.

The physical result is pinned too, but not by its bytes. A routed `.kicad_pcb`
carries UUIDs the engine minted and tracks in the order it finished them, and
`kicad-cli` stamps the time into every Gerber and drill file (see the
determinism note in `crates/synth-kicad/src/fab.rs`), so raw hashes move on runs
where nothing changed. Each row's `expect_geometry` holds hashes taken after
normalizing that away, and the fab job compares them exactly in both directions:

- One entry per `.kicad_pcb` in the export: UUIDs and zone fills removed, top-level
  items sorted, each track's ends in a fixed order. Every board file is pinned,
  not only `<board>.kicad_pcb`, because a board whose routing fails validation
  exports the unrouted board under that name and the engine's copper survives
  only in the intermediates beside it.
- `gerbers` and `drill`, one entry each over the directory: the set of lines in
  each file, with the time and tool version removed, a draw's `D01`/`D02` removed,
  and copper zone-fill regions removed.

Zone fills are out of the hash because the same pour can list an extra vertex
along a straight edge depending on the order tracks were knocked out of it. A
fill is derived from the zone outline, tracks, pads and clearances that remain in
the hash, so what this cannot see is a change in how KiCad computes fills. The
uploaded Gerbers are there for that.

A moved hash means the copper, footprints, outline or fabrication output changed.
The same run prints the `expect_geometry` line that would accept it; write that
only once the change is understood. The pins were measured in CI, on the KiCad and
routing engine it installs. A new engine tag legitimately moves them, and a KiCad
release moves them if its footprint library changed; `results.json` records both
versions. Raw hashes are still recorded in `results.json` for attribution.

The matrix also gates on the declared verdict: `expect`, `expect_fab`, and
`expect_source_codes`. The last is compared as an exact set in both
directions, so a board that acquires a new error-level diagnostic fails even
while every code already written down keeps firing, and a board that stops
reporting one fails too. A board's error-level output is a declaration, not a
floor.

### The inputs are pinned

Every row carries a `fixture_sha256`: a SHA-256 over the board's `.synth`
source and the layout sidecars beside it (`<stem>.placement.layout.toml`,
`<stem>.schematic.layout.toml`), each hashed under its file name and length.
The verdicts on a row were measured on exactly those bytes, so an edit to
any of them — deliberate or not — makes the row a claim about a different
board.

The registry is pinned once for the whole corpus, as
`[baseline] registry_manifest_sha256`: the SHA-256 of `synth registry
manifest`. Every board compiles against the registry, so a part edit can move
any verdict or artifact in the matrix.

`the_matrix_is_well_formed` and
`the_registry_is_the_one_the_matrix_was_measured_against` check them in the
normal test shards, so an accidental fixture or registry change fails on the
pull request that made it, without waiting for the release gate; the release
gate checks the fixtures as well. When the change is deliberate, re-run the
matrix, correct any verdict or `expect_artifacts` entry that moved, and only
then write the new hash the failure prints.

## What automation cannot prove

A board that passes this matrix has passed the compiler, the rule checks and
the exported-artifact checks. That is **not** the same as being fabricable.
The following require a human with PCB experience looking at the artifacts,
and no amount of green CI substitutes for them:

| Area | What a reviewer has to judge |
| --- | --- |
| Visual / schematic | Readability, sheet organisation, net naming, reference/value consistency, whether the drawing communicates the design to the next engineer. |
| Assembly | Courtyard clearances, pick-and-place access, polarity and orientation markings, silkscreen legibility after fabrication, reflow shadowing, panelisation. |
| Signal integrity | Impedance against the real stackup, pair spacing and length matching, skew, return-path continuity, via stubs, neck-downs, plane splits under signals. |
| Power integrity | Rail droop, decoupling placement and effectiveness, plane resonance, inrush and sequencing, star-point and ground-return strategy. |
| Thermal | Copper area and via count for dissipating parts, exposed-pad attachment, derating at the intended ambient, airflow assumptions. |
| Fabrication | Fab-house capability against the chosen profile, drill-to-copper, annular ring, solder-mask slivers, impedance-control feasibility, first-pass yield. |

## The handoff

Promoting a board from `experimental` to `qualified` takes a review, not a
code change:

1. Run the matrix with `SYNTH_QUALIFY_FAB=1`, or take the
   `qualification-results-fab` artifact from CI, and attach `results.json` and
   the board's directory under `boards/` to the review.
2. Open the exported project in KiCad and run its own ERC and DRC.
3. Have a reviewer with PCB experience work the table above, recording a
   verdict per area rather than one overall opinion.
4. File every defect found as its own issue, linked to the board id and the
   exact artifact, and classified as launch blocker, must-fix, should-fix or
   follow-up.
5. Only once no launch blocker remains, change that board's `status` to
   `qualified` in `matrix.toml`, in a commit whose message names the reviewer
   and links the review.

A board whose review found blockers stays `experimental`. A board whose review
found the envelope is not supported at all becomes `unsupported`, with the
diagnostics that prove it named in `expect_source_codes`.

## Capability classes

The classes match the launch-qualification issues, so a matrix row and a
qualification exercise refer to the same envelope:

- **Class A** — low-speed embedded: MCU, power, sensors, connectors.
- **Class B** — mixed-signal and RF, and USB/Ethernet-class interfaces.
- **Class C** — dense or high-speed: differential pairs, length matching, constrained placement.
- **Class D** — complex systems where the obligation is an auditable result and a stated limitation, not a claim of full support.

## A note on branch protection

The workflow is path-filtered, so it does not run on pull requests that touch
only documentation. GitHub reports a filtered-out workflow as missing rather
than as passing, so if `Qualification matrix` is made a required status check,
configure it in a way that tolerates the filter — otherwise documentation-only
pull requests will wait forever for a check that was never going to run.
