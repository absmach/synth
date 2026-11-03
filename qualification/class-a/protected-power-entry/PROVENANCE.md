# From prompt to PCB

The board is the output of an agent driving `synth mcp` from a prompt, followed by exports through the same MCP tools and Synth's exporter. This file is the record of that path. [`reproduce/`](reproduce/) is supplemental tooling that rebuilds the exports from the committed source; it is not the evidence that the board was made this way.

## The prompt and the agent trace

| | |
|---|---|
| Prompt | [`agent-run/prompt.md`](agent-run/prompt.md), exactly as given to the agent run `a-hub12` |
| Tool-call trace | [`agent-run/tool-calls.md`](agent-run/tool-calls.md): every call of that run, in order (33 calls: `synth_export`, `synth_describe_placement` and file reads and edits of the source and sidecar); `allow_incomplete` was never passed |

## Versions

| | |
|---|---|
| Synth, final exports | commit `6207bba6549dd0a235274a84084cd8cf3fb2b58e` (branch `case-binary-v4` of `JeffMboya/synth`): the merge of Synth pull requests #370, #388 and #394. `synth --version` prints `synth-cli 0.1.1`; `synth mcp --stdio` is the same binary |
| KiCadRoutingTools | `9399bfb29e072bfe60b10d9cdae20c274fc2a17a` |
| KiCad | 10.0.7 (library tables for DRC and ERC), on a 16-vCPU server |

## Inputs

- Source and sidecar: [`protected_power_entry.synth`](protected_power_entry.synth) and [`protected_power_entry.placement.layout.toml`](protected_power_entry.placement.layout.toml).
- User registry: [`reproduce/user-registry/`](reproduce/user-registry/), the 35 parts the exports ran with, each recorded as reviewed by JeffMboya.

## Generated artifacts and hashes

The README's reproducibility table has the first ten characters of SHA-256 of each exported file for the three exports. The routed board file carries random ids from the router and KiCad, so its three raw hashes differ and are compared after masking those ids; the schematic, project, symbol, BOM, placement and un-routed board files are byte-identical, and the Gerber, drill and job files differ in the creation time of their headers and are compared by content hash.

## Validation commands and results

| Command | Result file |
|---|---|
| The `synth_export` tool of `synth mcp --stdio` on the source and sidecar, three times into empty directories ([`agent-run/tools/mcp_export.py`](agent-run/tools/mcp_export.py) makes the call and records it) | [`export/`](export/), with `protected_power_entry.routing.json`; the three recorded sessions are in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/) |
| [`agent-run/tools/library_check.sh`](agent-run/tools/library_check.sh): `kicad-cli pcb drc --refill-zones --severity-all` and `kicad-cli sch erc --severity-all` with the footprint and symbol library tables configured ([`reports/fp-lib-table`](reports/fp-lib-table), [`reports/sym-lib-table`](reports/sym-lib-table)); [`agent-run/tools/footprint_diff.py`](agent-run/tools/footprint_diff.py) compares each footprint DRC reports as differing with its library file | [`reports/kicad_drc.json`](reports/kicad_drc.json), [`reports/kicad_erc.json`](reports/kicad_erc.json), [`reports/footprint_diff.txt`](reports/footprint_diff.txt) |
| [`reproduce/compare_exports.py`](reproduce/compare_exports.py) on the three exports | the hash table in the README |
| [`agent-run/tools/fab_package.sh`](agent-run/tools/fab_package.sh) | [`fab/`](fab/) |
