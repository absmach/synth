# Provenance: how the irrigation controller was made

The board is the output of an agent driving `synth mcp` from a prompt, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Design | An agent session driving `synth mcp` designed the board from an empty workspace (search of the registry, import of parts, validation, export) and produced the source. The prompt and the agent's tool-call logs are not kept in the repository. | [`irrigation_controller.synth`](irrigation_controller.synth) |
| Review changes | Later sessions and scripted edits applied the review points to the source and the placement sidecar: the crystal and LED parts, the compensation network and inductor of the 3.3 V buck, the TVS and fuse figures, the ideal-diode reverse-polarity stage (`U_IDEAL`, `Q1`), the placement of every part (pinned in the sidecar: connectors flush on the edges, the controller's capacitors beside it, through-hole to SMD pads at least 2.1 mm apart), and the net classes. | [`irrigation_controller.synth`](irrigation_controller.synth), [`irrigation_controller.placement.layout.toml`](irrigation_controller.placement.layout.toml) |
| Part records | Parts that the shipped registry does not carry, or that carry a review record of this board, are in the registry overlay, each with `reviewed_by`, `reviewed_at` and `datasheet_url`. `ltst_c190kgkt` keeps the registry's own record. | [`registry/`](registry/) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`, all `routed`, with no blocking reason, 0 DRC errors and 0 unconnected pads; no run passed `allow_incomplete`. | [`kicad/`](kicad/), with `irrigation_controller.routing.json` |

| | |
|---|---|
| Synth, final exports | `upstream/main` `be8a5b5a` (the merge of pull request #455, whose edge-track fix the router needs) plus the commits `62c4d661`, `7148b6ef` and `8be130f1` of Synth pull request #456, retrievable as `refs/pull/456/head` of `absmach/synth`; the last two move a pin-1 silkscreen marker clear of copper and of other parts' silkscreen instead of dropping it. `synth --version` prints `synth 0.1.2`; `synth mcp --stdio` is the same binary |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 (library tables for DRC and ERC) |
| Registry inputs | [`registry/`](registry/): the parts the exports ran with, among them `lm74700_q1` and `dmt6007lfg` (the ideal-diode stage), `l_power_10uh_6045`, `xtal_8mhz_3225_4p`, `ltst_c190kgkt`, `p_mosfet_si2301`, `ao3400a_nmos`, `smaj13a_tvs`, `polyfuse_1r5a` and the generic passives |

Synth findings from this board: #96 (missing symbols), #280 (ground-return vias at layer changes), #425 (silkscreen cut back from vias and pads), #426 (a label on a wire junction), #428 (flattened symbol).

## How an export is run

The export harness, a script not kept in the repository, calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`. It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON` (KiCadRoutingTools `9399bfb2` and the Python of its virtual environment) and sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`, a copy of [`registry/`](registry/); the routing record carries no `source_revision` because none is set. The workspace holds a `registry/parts` link to the build's registry. Each export goes into an empty directory. The router runs with a wall-clock limit of 5400 s.

## Generated artifacts and hashes

The first ten characters of SHA-256 of the exported files for the three exports are in the reproducibility table of [`QUALIFICATION.md`](QUALIFICATION.md). The schematic, project, symbol, BOM, placement and un-routed board files are byte-identical; the routed board is compared after masking the router's and KiCad's random ids, and the Gerber, drill and job files by a content hash with the creation time in their headers masked; the Gerber content hash is in [`fab/GERBER_CONTENT_SHA256`](fab/GERBER_CONTENT_SHA256) and the file hashes in [`SHA256SUMS`](SHA256SUMS).

## Validation commands and results

| Command | Result |
|---|---|
| The `synth_export` tool of `synth mcp --stdio` on the source and sidecar, three times into empty directories | [`kicad/`](kicad/), with `irrigation_controller.routing.json` |
| `kicad-cli pcb drc --refill-zones --severity-all` and `kicad-cli sch erc --severity-all` with the footprint and symbol library tables configured | [`kicad/drc_with_library_tables.json`](kicad/drc_with_library_tables.json), [`kicad/erc_with_library_tables.json`](kicad/erc_with_library_tables.json); the tables are [`kicad/fp-lib-table.library-check`](kicad/fp-lib-table.library-check) and [`kicad/sym-lib-table.library-check`](kicad/sym-lib-table.library-check) |
| The Gerbers, drill file, placement file and orderable BOM built from export 1 with KiCad's own CLI | [`fab/`](fab/) |
| The STEP model exported with `kicad-cli pcb export step` and reopened in OpenCASCADE against the PCB | [`models/irrigation_controller.step`](models/irrigation_controller.step); the results are stated in [`QUALIFICATION.md`](QUALIFICATION.md) |
