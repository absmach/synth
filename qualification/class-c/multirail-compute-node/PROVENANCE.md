# Provenance: how the multi-rail compute node was made

The board is the output of an agent driving `synth mcp` from a prompt, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompt | The prompt asked for a multi-rail compute board: a microcontroller with external memory and a USB-C input, a 5 to 12 V input with protection, a buck and three linear rails with a sequencing load switch, thermal paths, keepouts, assembly access and DFM limits, and a report measured from the exported files. | not kept in the repository |
| Agent sessions | Two `claude -p` runs with only the Synth MCP and file tools produced the source (`multirail_compute_node.synth`, 232 lines as committed) and a placement sidecar. Neither reached a routed board: the last agent export was stopped with an open rail and a via in a pad. | not kept in the repository |
| Part packages | The orchestrator replaced two parts after checking them against their manufacturer listings: the flash is the 208 mil W25Q128JVSIQ outline (registry part `w25q128jvsiq_208mil`) and the 1.2 V regulator the DDC package LCSC lists (`tlv70012_ddc`); the crystal is the JYJE 3TJ480000FYLBC (registry part `xtal_8mhz_3225_jyje_4p`) in place of the NDK part of the case 1 package (`xtal_8mhz_3225_4p`) because JLCPCB listed the NDK part out of stock. | [`registry/parts/`](../../../registry/parts/) |
| Release | The released source differs from the agent's in what a routed board needed (see the notes below the table). | [`multirail_compute_node.synth`](multirail_compute_node.synth), [`multirail_compute_node.placement.layout.toml`](multirail_compute_node.placement.layout.toml) |
| Final exports | One fresh-directory export with `synth export-kicad`, checked against a `synth_export` run. | not kept in the repository |

What the released source changes:

- The process is `jlc-filled-via` with `requires via_in_pad` for the thermal vias.
- The six thermal vias of the pad of `U1` are `[[vias]]` in the sidecar.
- The `SPI` class and a new `XTAL` class are locked to `F.Cu`.
- The `USB` class declares `edge_rate medium` on the cited full-speed edge times and is locked to `F.Cu`.
- The sidecar pins every part. The pose is the one that released after probe exports that moved the buck, the USB-C connector, the crystal group, `TP8` and `H1`.

| | |
|---|---|
| Synth, final export | the binary built from commit `76efdec5` of pull request #494 (a return via only on a net that carries a fast edge). The changes it includes: `edge_rate medium` replacing the fast inference from a differential pin (a clock, RF or analog pin, a crystal and a switching node still prove a fast edge) |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.8 |
| Registry inputs | `registry/parts/` of the repository: `tlv70012_ddc`, `w25q128jvsiq_208mil`, `xtal_8mhz_3225_jyje_4p` (and the capability tags added to `stm32f103c8`) |

Synth findings from this board: #425 (silkscreen), #428 (flattened symbol), #445 (the `edge_rate medium` tier), #449 (a keepout could not be anchored to a part; closed since, the compiler now accepts `anchor "U1"`, and the board's `keepout hot_buck` is anchored to `U1`).

## How an export is run

The export harness is a script not kept in the repository. It calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`.

- It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON`: KiCadRoutingTools `9399bfb2` and the Python of its virtual environment. The defaults in the file are the author's earlier machine.
- It sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`.
- It sets `SYNTH_SOURCE_REVISION`, which the routing record carries as `source_revision`.
- The workspace holds a `registry/parts` link to the build's registry.
- Each export goes into an empty directory.
- The routing record and the export traces carry paths of the machine the exports ran on.
