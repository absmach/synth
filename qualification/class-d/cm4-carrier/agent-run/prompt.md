You are building one Class-D qualification board in the Synth repo (/home/steve/work/synth). Work in a fresh git worktree off the latest upstream/main (run `git fetch upstream` first; origin is a stale fork). Branch name: `syn-407-cm4-carrier`.

## Your task
Implement GitHub issue absmach/synth#407 exactly. Read it first:
`gh issue view 407 --repo absmach/synth`
It contains the board spec, deliverables, acceptance checklist and ground rules. The issue is the source of truth. If something in it conflicts with the repo, stop and report the conflict instead of guessing.

Issues: 407 = cm4-carrier, 408 = bldc-stack, 409 = din-io, 410 = estop-negative.

## How to work
1. Read `qualification/class-b/buck/` and `qualification/class-b/usb-uart/` (README.md, QUALIFICATION.md, qualification.json, SHA256SUMS, the .synth, the placement sidecar) and copy their structure. Read `crates/synth-cli/tests/usb_uart_board.rs` and model your test on it.
2. Read `docs/kicad-workflows.md` and `registry/parts/` before writing the .synth. Use existing registry parts where the issue says they exist.
3. For every part that must be authored: fetch the manufacturer datasheet, write `registry/parts/<category>/<id>.synth.toml` with `[provenance]` (URL, revision, `reviewed_by = ""`), `mpn`, `lcsc_pn`, real pin numbers from the datasheet, and a footprint that exists in the local KiCad library (use `synth_resolve_footprints`). Never invent pins or footprints.
4. Use the Synth MCP tools in this gate order: `synth_validate` (zero blocking) -> `synth_place_with_hints` with run_routing=false and read `placement_quality.visual_review` -> `synth_render_schematic` and look at the image -> `synth_export` -> `kicad-cli sch erc` (zero errors) -> `synth check cm4-carrier --json` and `--fab --json`.
5. Work test-first where code changes (issue 410 core change, any multiboard check in 408): write the failing test, then the code, then refactor. Tests are the gate.
6. Fix problems at the `.synth` source or the placement sidecar. Never hand-edit generated `.kicad_*` or `bom.csv`.

## Agent-run record (required)
Also create `qualification/class-d/cm4-carrier/agent-run/` exactly as the "`agent-run/` folder" section of the issue lists (prompt.md, tool-calls.md, report.md, DESIGN_NOTES.md, PIN_MAPPING.md, NETLIST.md, POWER_BUDGET.md, ASSEMBLY.md, BRINGUP.md, VERIFICATION_REPORT.md). Model it on PR #269 (`gh pr view 269 --repo absmach/synth --json files`, then read its agent-run files). Keep a running log of your tool calls as you work so tool-calls.md is true. Nothing invented: write "not verified" where you did not measure or read it.

## Hard rules
- Do not claim anything Synth cannot verify. Each README needs a "Not verified by Synth" section. `qualification.json` keeps `"qualified": false`; human-review gates stay `passed: false`; never invent reviewer names or sign-offs.
- Record failures honestly (e.g. `--fab` failures). Do not hide, weaken or delete a check to make it pass.
- Smallest change that satisfies the issue. Do not refactor unrelated code. Match existing style.
- Do not regenerate golden hashes locally. Use hashes from CI output. Run the build/test in the repo's normal way (`cargo nextest`), then `cargo clean` the target dir when done.
- Add the `[[board]]` entry to `qualification/matrix.toml` (class "D", status "experimental").
- Commit with `git commit -sm "<message>"`. No AI attribution lines. Do not push, do not open a PR, do not merge.

## Stop and report (do not push on) if
- A datasheet cannot be found for a part you must author.
- A required Synth capability does not exist and the issue does not say how to add it.
- `synth_validate` has blocking diagnostics you cannot fix at the source.

## Final report (keep it short)
1. Branch name and commit hashes.
2. Acceptance checklist from the issue, each item: done / not done / blocked, with the command or file that proves it.
3. Output of the gate commands (verdict lines only).
4. Every gap, assumption or datasheet you could not verify.
