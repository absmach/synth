# From prompt to PCB

The board is the output of an agent driving `synth mcp` from a prompt, followed by scripted and agent steps that all go through Synth's own tools. This file is the record of that path. [`reproduce/`](reproduce/) is supplemental tooling that rebuilds the exports from the committed source; it is not the evidence that the board was made this way.

## The prompt and the agent trace

| Stage | Prompt | Agent output and trace |
|---|---|---|
| Design, from an empty workspace | [`agent-run/prompt.md`](agent-run/prompt.md) | [`agent-run/report.md`](agent-run/report.md), [`agent-run/tool-calls.md`](agent-run/tool-calls.md) (every call in order: `synth_search_registry`, `synth_import_part`, `synth_validate`, `synth_export` and the rest), [`agent-run/resume-messages.md`](agent-run/resume-messages.md) |
| Fill (two runs) | [`agent-run/fill-pass-prompt.md`](agent-run/fill-pass-prompt.md) | [`report 1`](agent-run/fill-pass-report-1.md), [`report 2`](agent-run/fill-pass-report-2.md) |
| Review fixes, input MOSFET, second fill | [`prompt-1`](agent-run/review-fixes/prompt-1-review-fixes.md), [`prompt-2`](agent-run/review-fixes/prompt-2-input-mosfet.md), [`prompt-3`](agent-run/review-fixes/prompt-3-fill.md) | [`report-1`](agent-run/review-fixes/report-1-review-fixes.md), [`report-2`](agent-run/review-fixes/report-2-input-mosfet.md), [`report-3`](agent-run/review-fixes/report-3-fill.md) |
| Input protection and final blockers | [`agent-run/final-blockers/prompt.md`](agent-run/final-blockers/prompt.md) | [`agent-run/final-blockers/`](agent-run/final-blockers/) |

[`agent-run/HOW_THIS_BOARD_WAS_MADE.md`](agent-run/HOW_THIS_BOARD_WAS_MADE.md) lists every step in order, including the small scripted edits (each by a script in [`agent-run/tools/`](agent-run/tools/)) and the searches that drive the same MCP tools. No run passed `allow_incomplete`.

## Versions

| | |
|---|---|
| Synth, final exports | commit `a12aa289b7b001e8d5a197b15d20302e4718da5c` of `absmach/synth` (`upstream/main`: the merge of Synth pull request #269, which carries the router fix that counts the routing report's statistics after the repair passes, on top of `0ceb7b80`, the merge of #388 with #370 and #394 before it). `synth --version` prints `synth-cli 0.1.1`; `synth mcp --stdio` is the same binary |
| Synth, schematic and symbols | the 13 group-sheet schematics and `irrigation_controller.kicad_sym` come from an export of the same source and user registry on the tree of `upstream/main` `fbfd8d60` (pull request #440 merged); the pinned binary writes group sheets and a symbol library without the power symbols. The root sheet is byte-identical between the two binaries. The routed board, project, BOM and placement files are from export 1 on the pinned binary |
| Synth, earlier agent stages | the binary of the day, built from the layout and silkscreen branches listed in `HOW_THIS_BOARD_WAS_MADE.md` |
| KiCadRoutingTools | `9399bfb29e072bfe60b10d9cdae20c274fc2a17a` |
| KiCad | 10.0.6 (library tables for DRC and ERC) |

## Inputs

- Source and sidecar: [`irrigation_controller.synth`](irrigation_controller.synth) and [`irrigation_controller.placement.layout.toml`](irrigation_controller.placement.layout.toml).
- User registry: [`reproduce/user-registry/`](reproduce/user-registry/), the parts the exports ran with.

## Generated artifacts and hashes

The first ten characters of SHA-256 of the exported files for the three exports are in the README's reproducibility table. The schematic, project, symbol, BOM, placement and un-routed board files are byte-identical; the routed board is compared after masking the router's and KiCad's random ids, and the Gerber, drill and job files by a content hash with the creation time in their headers masked; the Gerber content hash is in [`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256) and the file hashes in [`fab/SHA256SUMS`](fab/SHA256SUMS).

## Validation commands and results

| Command | Result file |
|---|---|
| The `synth_export` tool of `synth mcp --stdio` on the source and sidecar, three times into empty directories ([`agent-run/tools/mcp_export.py`](agent-run/tools/mcp_export.py) makes the call and records it) | [`export/`](export/), with `irrigation_controller.routing.json`; the three recorded sessions are in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/) |
| [`agent-run/tools/library_check.sh`](agent-run/tools/library_check.sh): `kicad-cli pcb drc --refill-zones --severity-all` and `kicad-cli sch erc --severity-all` with the footprint and symbol library tables configured ([`reports/fp-lib-table`](reports/fp-lib-table), [`reports/sym-lib-table`](reports/sym-lib-table)); [`agent-run/tools/footprint_diff.py`](agent-run/tools/footprint_diff.py) compares each footprint DRC reports as differing with its library file | [`reports/kicad_drc.json`](reports/kicad_drc.json), [`reports/kicad_erc.json`](reports/kicad_erc.json), [`reports/footprint_diff.txt`](reports/footprint_diff.txt) |
| [`reproduce/compare_exports.py`](reproduce/compare_exports.py) on the three exports | the hash table in the README |
| [`agent-run/tools/fab_package.sh`](agent-run/tools/fab_package.sh) | [`fab/`](fab/) |

The agents' own reports in [`agent-run/`](agent-run/) describe each run as the agent saw it at the time. The review record of the final board is the one above and in `README.md`: every part and the board are reviewed by JeffMboya.
