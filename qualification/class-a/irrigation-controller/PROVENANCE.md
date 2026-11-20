# Provenance: how the irrigation controller was made

The board is the output of an agent driving `synth mcp` from a prompt, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Design | An agent session driving `synth mcp` designed the board from an empty workspace (search of the registry, import of parts, validation, export) and produced the source. The prompt and the agent's tool-call logs are not kept in the repository. | [`irrigation_controller.synth`](irrigation_controller.synth) |
| Review changes | Later sessions and scripted edits applied the review points to the source and the placement sidecar: the crystal and LED parts, the compensation network and inductor of the 3.3 V buck, the TVS and fuse figures, the ideal-diode reverse-polarity stage (`U_IDEAL`, `Q1`), the placement of every part (pinned in the sidecar: connectors flush on the edges, the controller's capacitors beside it, through-hole to SMD pads at least 2.1 mm apart), and the net classes. | [`irrigation_controller.synth`](irrigation_controller.synth), [`irrigation_controller.placement.layout.toml`](irrigation_controller.placement.layout.toml) |
| Part records | Parts that the shipped registry does not carry are in `registry/parts/` of the repository, each with `reviewed_by`, `reviewed_at` and `datasheet_url`. | `registry/parts/` |
| Final export | One fresh-directory export through `synth mcp`, `synth_export`, `routed`, 0 DRC errors and 0 unconnected pads; no run passed `allow_incomplete`. | [`kicad/`](kicad/) |

| | |
|---|---|
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 (library tables for DRC and ERC) |
| Registry inputs | `registry/parts/` of the repository: the shipped parts the board uses, and the added `lm74700_q1`, `dmt6007lfg` (the ideal-diode stage) and `xtal_32k768_fc135_7pf` |

Synth findings from this board: #96 (missing symbols), #425 (silkscreen cut back from vias and pads), #426 (a label on a wire junction), #428 (flattened symbol).

## How an export is run

The export harness, a script not kept in the repository, calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`. It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON` (KiCadRoutingTools `9399bfb2` and the Python of its virtual environment) and sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`, a copy of [`registry/parts/`](../../../registry/parts/); the routing record carries no `source_revision` because none is set. The workspace holds a `registry/parts` link to the build's registry. Each export goes into an empty directory. The router runs with a wall-clock limit of 5400 s.

## Generated artifacts and hashes

The file hashes are in [`SHA256SUMS`](SHA256SUMS).

## Validation commands and results

| Command | Result |
|---|---|
| The `synth_export` tool of `synth mcp --stdio` on the source and sidecar into an empty directory | [`kicad/`](kicad/) |
| `kicad-cli pcb drc --refill-zones --severity-all` and `kicad-cli sch erc --severity-all` with the footprint and symbol library tables configured | results stated in [`QUALIFICATION.md`](QUALIFICATION.md) |
| The Gerbers, drill file, placement file and orderable BOM built from export 1 with KiCad's own CLI | [`fab/`](fab/) |
| The STEP model exported with `kicad-cli pcb export step` and reopened in OpenCASCADE against the PCB | [`models/irrigation_controller.step`](models/irrigation_controller.step); the results are stated in [`QUALIFICATION.md`](QUALIFICATION.md) |
