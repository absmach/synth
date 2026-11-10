# Provenance: how the iCE40 HDMI (DVI-style TMDS) node was made

The board is the output of an agent driving `synth mcp` from a prompt, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompt | The prompt asked for an iCE40HX4K driving an HDMI Type-A output as DVI-style TMDS: four differential pairs with a declared impedance, pair spacing and skew budget, DDC level shifting, a hot-plug divider, a fused +5 V pin, an SPI boot flash, rails and a status LED row, and an evidence report measured from the exported files. | [`agent-run/prompts/`](agent-run/prompts/) |
| Agent session | One `claude -p` run with only the Synth MCP and file tools. It built the design on the 121-ball BGA package of the iCE40; none of its five exports released (return-via and connectivity findings), and the session was stopped. | [`agent-run/traces/agent-sessions/`](agent-run/traces/agent-sessions/) |
| Package change | The BGA left no room for return vias beside the TMDS and bus vias at 0.8 mm pitch, so the orchestrator moved the design to the iCE40HX4K-TQ144 (user-registry part `ice40hx4k_tq144`, generated from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of Lattice DS1040, 107 user I/O), added the three bank supply pins and their decoupling, and assigned the four lanes to adjacent left-side pin pairs in the connector's order. | [`ice40_hdmi_node.synth`](ice40_hdmi_node.synth), [`reproduce/user-registry/`](reproduce/user-registry/) |
| Stackup and classes | JLCPCB's six-layer `JLC06161H-3313` stackup, from [jlcpcb.com/impedance](https://jlcpcb.com/impedance), declared in the source; the TMDS class is 0.11 mm tracks at a 0.127 mm clearance (100.3 ohm differential by the closed-form estimate in the README); the TMDS and source classes declare `edge_rate medium` on Lattice's IBIS figures. | [`ice40_hdmi_node.synth`](ice40_hdmi_node.synth) |
| Placement | The orchestrator pinned the placement in the sidecar through probe exports: the eight source resistors in one column beside the FPGA's left pins, at the pitch and column position of the densest probe that released with the declarations above. Probes with the TMDS classes locked to the top layer, with a 0.2 mm class clearance and with `couple tight` did not release. | [`ice40_hdmi_node.placement.layout.toml`](ice40_hdmi_node.placement.layout.toml) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`, each with `router_timeout_secs` 2700. Each returns no blocking signal-integrity finding. | [`agent-run/traces/export-*.trace.json`](agent-run/traces/) |

| | |
|---|---|
| Synth, final exports | the binary built from `upstream/main` `fbfd8d60`, the merge of pull request #440; the three final exports are made on it, and the routing record carries `source_revision` `fbfd8d60239391d17cdd81b9337fc0ee4ee22cd9`. The merged changes include `edge_rate medium` replacing the fast inference from a `diff_pair` or a differential pin (a clock, RF or analog pin, a crystal and a switching node still prove a fast edge) |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 |
| Registry inputs | [`reproduce/user-registry/`](reproduce/user-registry/): `ice40hx4k_tq144`, `w25q128jvsiq_208mil`, `tlv70012_ddc` |

Synth findings from this board: #425 (silkscreen), #428 (flattened symbol), #445 (the `edge_rate medium` tier), #147 (class skew budgets are not enforced at export), #447 (a declared `diff_pair` is never routed as a pair).

## How an export is run

`agent-run/tools/mcp_export.py <workspace> <synth binary> <out dir> <trace.json>` calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`. It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON` (KiCadRoutingTools `9399bfb2` and the Python of its virtual environment; the defaults in the file are the author's earlier machine), sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`, and sets `SYNTH_SOURCE_REVISION`, which the routing record carries as `source_revision`. The workspace holds a `registry/parts` link to the build's registry. Each export goes into an empty directory. The routing record and the export traces carry paths of the machine the exports ran on.
