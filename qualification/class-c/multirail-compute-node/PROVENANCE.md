# Provenance: how the multi-rail compute node was made

The board is the output of an agent driving `synth mcp` from a prompt, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompt | The prompt asked for a multi-rail compute board: a microcontroller with external memory and a USB-C input, a 5 to 12 V input with protection, a buck and three linear rails with a sequencing load switch, thermal paths, keepouts, assembly access and DFM limits, and a report measured from the exported files. | not kept in the repository |
| Agent sessions | Two `claude -p` runs with only the Synth MCP and file tools produced the source (`multirail_compute_node.synth`, 228 lines) and a placement sidecar. Neither reached a routed board: the last agent export was stopped with an open rail and a via in a pad. | not kept in the repository |
| Part packages | The orchestrator replaced two parts after checking them against their manufacturer listings: the flash is the 208 mil W25Q128JVSIQ outline (registry part `w25q128jvsiq_208mil`) and the 1.2 V regulator the DDC package LCSC lists (`tlv70012_ddc`); the crystal is the JYJE 3TJ480000FYLBC (`xtal_8mhz_3225_4p` of the case 1 package) because JLCPCB listed the NDK part out of stock. | [`registry/`](registry/) |
| Release | The released source differs from the agent's in what a routed board needed: the process is `jlc-filled-via` with `requires via_in_pad` for the thermal vias; the six thermal vias of `U1`'s pad are `[[vias]]` in the sidecar; the `SPI` and a new `XTAL` class are locked to `F.Cu`; the `USB` class declares `edge_rate medium` on the cited full-speed edge times. The sidecar pins every part; the pose is the one that released after probe exports that moved the buck, the USB-C connector, the crystal group, `TP8` and `H1`. | [`multirail_compute_node.synth`](multirail_compute_node.synth), [`multirail_compute_node.placement.layout.toml`](multirail_compute_node.placement.layout.toml) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`. | not kept in the repository |

| | |
|---|---|
| Synth, final exports | the binary built from `upstream/main` `fbfd8d60`, the merge of pull request #440; the three final exports are made on it, and the routing record carries `source_revision` `fbfd8d60239391d17cdd81b9337fc0ee4ee22cd9`. The merged changes include `edge_rate medium` replacing the fast inference from a differential pin (a clock, RF or analog pin, a crystal and a switching node still prove a fast edge) |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 |
| Registry inputs | `registry/parts/` of the repository: `tlv70012_ddc`, `w25q128jvsiq_208mil`, `xtal_8mhz_3225_4p` |

Synth findings from this board: #425 (silkscreen), #428 (flattened symbol), #445 (the `edge_rate medium` tier), #449 (a keepout cannot be anchored to a part).

## How an export is run

the export harness, a script not kept in the repository, calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`. It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON` (KiCadRoutingTools `9399bfb2` and the Python of its virtual environment; the defaults in the file are the author's earlier machine), sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`, and sets `SYNTH_SOURCE_REVISION`, which the routing record carries as `source_revision`. The workspace holds a `registry/parts` link to the build's registry. Each export goes into an empty directory. The routing record and the export traces carry paths of the machine the exports ran on.
