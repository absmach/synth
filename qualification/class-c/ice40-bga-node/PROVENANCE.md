# Provenance: how the iCE40 BGA node was made

The board is the output of agents driving `synth mcp` from prompts, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompts | The first prompt asked for a full board (USB-C, a companion MCU, a UART bridge, two headers); later sessions, after the iCE40 supply and configuration nets would not close, used a simplified board: 5 V entry, 3.3 V and 1.2 V rails, the FPGA, an SPI boot flash, one header, LEDs. | [`agent-run/prompts/`](agent-run/prompts/) |
| Agent sessions | Twelve agent sessions in all, each a `claude -p` run with only the Synth MCP and file tools. The early ones did not release. The session that released was given the three Synth fixes below and the declarations that closed the board (`plane` on the 3.3 V class only, `VCCPLL` in its own class, `escape { method underpad }` with no layers). | [`agent-run/traces/agent-sessions/`](agent-run/traces/agent-sessions/) |
| Part packages | Two parts were corrected against their manufacturer listings: the flash's footprint to the 208 mil outline (user-registry part `w25q128jvsiq_208mil`) and the regulator to the DDC package (user-registry part `tlv70012_ddc`); the header was reduced to the 1x12 the board uses. | [`reproduce/user-registry/`](reproduce/user-registry/) |
| Placement | The placement of the flash and the parts beside it was set in the sidecar by the orchestrator through probe exports; the outline was then densified by `synth_compact_layout` in scale mode, step 920. | [`reports/compaction_scale_mode.json`](reports/compaction_scale_mode.json) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`. | [`agent-run/traces/export-*.trace.json`](agent-run/traces/) |

| | |
|---|---|
| Synth, final exports | the tip of branch `fix-bga-synth` of `JeffMboya/synth`, commit `d4b0ab22bcf4b3443d4311465fa637f990058c02`, on `upstream/main` `2390cc98`. It carries six commits that are not in `upstream/main` yet, and has no pull request (none can be opened until #270 merges): the filled-via land for a plane via in a pad, the filled-via ring rule, the repair for an isolated plane-net pad, the router's open-net names from the connectivity pass, the warning for an unknown manufacturer name, and the repair that leaves a via in a pad where via-in-pad is allowed |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 |
| Registry inputs | [`reproduce/user-registry/`](reproduce/user-registry/): `ice40hx4k_bg121_nc`, `tlv70012_ddc`, `w25q128jvsiq_208mil`, `header_1x5_swd`, `tlv70012_1v2` |

Synth findings from this board: #424 (report counts), #425 (silkscreen), #426, #428, #429 (the annular ring floor and the via-in-pad land), #430 (plane rails take every inner layer), #431 (no-connect declarations and the multi-unit symbol).
