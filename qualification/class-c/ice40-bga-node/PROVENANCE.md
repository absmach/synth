# Provenance: how the iCE40 BGA node with a USB-C input and an MCU was made

The board is the output of agents driving `synth mcp` from prompts, followed by orchestration steps that all go through Synth's own tools or are named here. This file is the record of that path.

| Stage | What happened | Evidence |
|---|---|---|
| Prompts | The first prompt asked for a full board (USB-C, a companion MCU, a UART bridge, two headers); later sessions, after the iCE40 supply and configuration nets would not close, used a simplified board: 5 V entry, 3.3 V and 1.2 V rails, the FPGA, an SPI boot flash, one header, LEDs. Two further sessions added the USB-C input and the companion MCU to that board. | not kept in the repository |
| Agent sessions | Fifteen session logs (not kept in the repository), each a `claude -p` run with only the Synth MCP and file tools; sessions 8, 13 and 14 are described below. The early ones did not release. Session 8 released the simplified board with the three Synth fixes below and the declarations that closed it (`plane` on the 3.3 V class only, `VCCPLL` in its own class, `escape { method underpad }` with no layers). Session 13 added the USB-C and the MCU: all nets routed with 0 DRC errors but one return-via blocker at the crystal pad remained, and the outline had grown because the run had no placement sidecar. Session 14 started from the released board's sidecar; its USB nets stayed open around the series resistors and I stopped it. | not kept in the repository |
| Part packages | Two parts were corrected against their manufacturer listings: the flash's footprint to the 208 mil outline (user-registry part `w25q128jvsiq_208mil`) and the regulator to the DDC package (user-registry part `tlv70012_ddc`); the header was reduced to the 1x12 the board uses. The crystal is the JYJE 3TJ480000FYLBC, in the user-registry part `xtal_8mhz_3225_4p`, because JLCPCB listed the NDK part as out of stock. | [`registry/`](registry/) |
| Placement | The orchestrator pinned the placement in the sidecar through probe exports: the series resistors `R25` and `R26` next to the MCU's USB pins, the MCU's decoupling column, the USB-C receptacle at rotation 270 flush on the left edge, the 12-pin header `J3` on the top edge and the mounting holes at opposite corners. Scale-mode compaction (`synth_compact_layout`) ran on an earlier pose of this layout; its steps below 1000 failed on return-via and plane-via findings, so the pose was kept. | [`ice40_bga_node.placement.layout.toml`](ice40_bga_node.placement.layout.toml) |
| Final exports | Three fresh-directory exports through `synth mcp`, `synth_export`. | not kept in the repository |

| | |
|---|---|
| Synth, final exports | the binary built from `upstream/main` `fbfd8d60`, the merge of pull request #440 (the return-via, via-in-pad, plane-via, multi-unit symbol and `edge_rate medium` changes this board needed are all in it). `synth --version` prints `synth 0.1.1`; `synth mcp --stdio` is the same binary |
| KiCadRoutingTools | `9399bfb2` |
| KiCad | 10.0.6 |
| Registry inputs | [`registry/`](registry/): `ice40hx4k_bg121_nc`, `tlv70012_ddc`, `w25q128jvsiq_208mil`, `header_1x5_swd`, `tlv70012_1v2`, `stm32f103c8_usb`, `xtal_8mhz_3225_4p`, and the copies `c_generic_0402`, `r_generic_0402`, `led_amber_0603`, `led_green_0603`, `led_red_0603`, `testpoint_pad_1_5`, `p_mosfet_si2301`, `polyfuse_500ma`, `usb_c_receptacle`, `ferrite_bead_0603` that carry the review record |

Synth findings from this board: #424 (report counts), #425 (silkscreen), #426, #428, #429 (the annular ring floor and the via-in-pad land), #430 (plane rails take every inner layer), #431 (no-connect declarations and the multi-unit symbol).

## How an export is run

the export harness, a script not kept in the repository, calls the `synth_export` tool of `synth mcp --stdio` on the workspace's `.synth`, its placement sidecar and `user-registry/`. It reads `KICAD_ROUTING_TOOLS_REPO` and `SYNTH_KRT_PYTHON` (KiCadRoutingTools `9399bfb2` and the Python of its virtual environment; the defaults in the file are the author's earlier machine), sets `SYNTH_USER_REGISTRY_DIR` to the workspace's `user-registry/`, and sets `SYNTH_SOURCE_REVISION` to `fbfd8d60239391d17cdd81b9337fc0ee4ee22cd9`, which the routing record carries as `source_revision`. The workspace holds a `registry/parts` link to the build's registry. Each export goes into an empty directory.
