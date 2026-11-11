# Provenance: how the iCE40 SDR SDRAM node with a USB-C power input was made

The board is the output of agents driving `synth mcp` from prompts, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompts | The first prompt asked for an iCE40HX4K in a 121-ball BGA with a DDR-class memory. The memory became a 64 Mbit single-data-rate SDRAM (ISSI IS42S16400J) driven by a soft controller, with no DDR claim, because the iCE40 has no hard DDR PHY and the IS42S16400J is an SDR part. The later prompts replaced the BGA by the 144-pin TQFP (`ice40hx4k_tq144`) and added the USB-C power input and the 1x6 programming header, so the board stands alone. | [`agent-run/prompts/`](agent-run/prompts/) |
| Agent sessions | Each session is a `claude -p` run with only the Synth MCP and file tools; the two logs kept are the TQFP board's sessions. The BGA package left no room for a return via beside the SDRAM bus vias at 0.8 mm pitch (rule `W-SYNTH-DRC-016` blocked the release), which is why the TQFP-144 replaced it. | [`agent-run/traces/agent-sessions/`](agent-run/traces/agent-sessions/) |
| Part packages | The iCE40HX4K-TQ144 part is generated from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of Lattice DS1040 (107 user I/O; the global-buffer pad used for the SDRAM clock typed as a clock output). The flash is the 208 mil W25Q128JVSIQ outline and the 1.2 V regulator the DDC package, as on case 1. | [`reproduce/user-registry/`](reproduce/user-registry/) |
| Stackup | JLCPCB's six-layer `JLC06161H-3313` stackup, from [jlcpcb.com/impedance](https://jlcpcb.com/impedance), declared in the source. | [`ice40_sdram_node.synth`](ice40_sdram_node.synth) |
| Edge rate | The `SDADDR` class declares `edge_rate medium` on the strength of Lattice's IBIS model `ice40.ibs` rev 2.5 (see README); `SDDATA` and `SDCLK` stay `fast`. | [`ice40_sdram_node.synth`](ice40_sdram_node.synth) |
| Placement | The orchestrator pinned the placement in the sidecar through sweeps of probe exports (translations of the SDRAM group by whole grid steps, pin-order assignments of the FPGA's memory pins to the SDRAM's near row). The pinned pose is the densest one that released; denser poses did not release. The programming header `J2` was probed at five heights (y = 51, 52, 53, 55 and 58 mm): 51 and 52 mm did not release (copper within 0.05 mm of the board edge at the top-left corner), 53 mm left a +3V3 pad unconnected, and 55 mm released and is the pose with the most clearance to the keepout of mounting hole `H2` of the poses that did. `synth_compact_layout` refused to start from this pose ("starting layout does not release" under its internal routing, although the MCP export releases). | [`ice40_sdram_node.placement.layout.toml`](ice40_sdram_node.placement.layout.toml) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`, each with `router_timeout_secs` 2700 (the router needs about 1,800 s on the machine that made them; the default budget is 1,200 s). Each export returns no blocking signal-integrity finding. | [`agent-run/traces/export-*.trace.json`](agent-run/traces/) |

| | |
|---|---|
| Synth, final exports | the binary built from commit `48a0a490b2c3dde076a90e55ba05b82ce4137605` (pull request #455, branch `fix-451-edge-tracks` of `JeffMboya/synth`), which `upstream/main` contains as the merge `be8a5b5a`, on top of `upstream/main` `3e59acdf`. The pull request adds one pass: a ground track whose copper comes nearer the board outline than the board's edge rule does not reach the candidate that is judged (issue #451). `upstream/main` carries the rest of the router, DRC, exporter and symbol-layout work of pull request #440. The exports set `SYNTH_SOURCE_REVISION` to that commit, and the routing record of each export carries it as `source_revision` |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 |
| Registry inputs | [`reproduce/user-registry/`](reproduce/user-registry/): `ice40hx4k_tq144`, `header_1x6_prog`, `w25q128jvsiq_208mil`, `tlv70012_ddc` |

Synth findings from this board: #425 (silkscreen), #428 (flattened symbol), #445 (the `edge_rate medium` tier), #147 (class skew budgets are not enforced at export).
