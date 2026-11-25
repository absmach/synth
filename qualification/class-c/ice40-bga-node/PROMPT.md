# Prompt

The prompts used to produce this board, recovered verbatim from the git history of this repository.

The board was made by an agent driving `synth mcp`. Changes made after the last prompt, for example in response to review, are in the git history of this pull request.

## prompt-session1-full-board

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session1-full-board.md`

~~~~text
Class C case 1 — iCE40 BGA Node: a BGA/fine-pitch board with escape routing and via/fanout rules (Synth MCP)

1. Objective and execution environment

Design a complete four-layer board around a Lattice iCE40HX4K in a 121-ball, 0.8 mm pitch BGA, using the Synth MCP tools only, and export it to KiCad. Board name: ice40_bga_node. Native Synth project: ice40_bga_node.synth in your workspace. The question the board answers (issue #328 under #40): can Synth produce a reviewable board whose BGA is escaped, with a via and fanout strategy that survives export?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- A USB-C receptacle that powers the board (5 V, CC pins with 5.1 kohm pull-downs) and a USB-to-UART bridge (registry part ch340c) to a companion microcontroller, so the board can be programmed with no firmware in it.
- Input protection in a stated order: a resettable fuse first, then a TVS clamp whose standoff is above the maximum input and whose clamp is below what it protects, then a series Schottky or P-FET reverse-polarity stage. Write the numbers: the 5 V maximum, the fuse hold current, the TVS standoff and clamp, the rails' worst-case currents.
- Rails: 3.3 V (AMS1117-3.3 from the registry) for the I/O banks and the companion MCU, and a 1.2 V rail for the FPGA core and PLL. Find a registry regulator for 1.2 V; if none fits, import or author one with a real datasheet_url and say so. The PLL supply is filtered from the core rail with a ferrite bead and capacitors. Every regulator gets its input and output capacitors beside it.
- FPGA: registry part ice40hx4k_bg121. Every supply ball decoupled, ground balls tied to the inner ground plane by vias. The configuration pins (CRESET_B, CDONE, SPI_SS_B, SPI_SCK, SPI_SDI/SDO) wired to an STM32G030 companion MCU (registry stm32g030f6p6) that loads the bitstream over the iCE40 slave SPI and owns the status LEDs. No SPI flash is required.
- Companion MCU: its supply decoupling, a reset button with its network, and a 5-pin SWD header, all within a few millimetres of the MCU.
- GPIO: two FPGA break-out headers (a 1x20 header each, registry header_1x20) carrying as many FPGA I/O as route cleanly, ground and 3.3 V on each, plus an MCU header. Each I/O line has a series resistor.
- Status LEDs with series resistors in one row: power, CDONE, and two user LEDs.
- Two mounting holes. Test points only on the principal rails and the configuration signals.
- Four layers: signal, ground, power, signal. Declare them in the Synth source. The board declares `requires via_in_pad` and a manufacturer profile that offers it (use the language reference and synth_capability to find the exact profile name, for example jlc-filled-via), because a via on a 0.8 mm grid does not fit between balls at the standard tier.

3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; a net whose class does not is treated as fast and needs a ground return via beside every layer change. The FPGA break-out I/O are for slow user signals in this design: put them in a class declared `edge_rate slow` and write that claim in the report. Nets with clock, differential, analog, crystal or switch-node pins stay critical on their own. Do not add test points beyond the rails and configuration signals, and do not add parts whose only job is to fill space.

4. BGA escape and via strategy (the claim under review)

The report must show, from the exported files: how many of the 121 balls are used, how every one is escaped (via in pad on which layer, or surface), that the escape vias use the process minimum drill and size the profile allows, how the ground and power balls reach their planes, where each supply ball's decoupling capacitor sits (distance to the ball), and that the escape ledger accounts for every signal ball. Any ball you leave unused or unconnected is named. The board must route completely and pass KiCad DRC with zero errors in a release export. Never pass allow_incomplete=true.

4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the escape evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

7. Layout quality rules

PCB layout quality rules (mandatory, applies to every board)

Keep the full functionality, the component set, the connectors and the mounting holes. Add no decorative text: no board name, no revision text, no functional headings, no logos. Silkscreen carries only what identifies parts, connectors and test points. Four rules, in this order: compactness (no unnecessary gaps between related parts), grouping (the parts of one circuit sit together), consistency (repeated parts, connectors and identical circuits are aligned and oriented alike), functionality (never trade electrical performance, manufacturability or serviceability for looks). Every gap that remains must have an electrical, mechanical or manufacturing reason.

Work by role, not by name. Decide each circuit's role from its connectivity, then place by these rules:
1. Support parts hug the part they serve. A decoupling capacitor sits within a few millimetres of the supply pin it decouples, a crystal and its load capacitors beside the clock pins, a reset or boot network beside the pin it drives, pull-ups and dividers beside the node they set, a flyback or clamp diode beside the switch it protects. Do not leave a support part standing alone in open board area. After every placement, read the placement review: when it says a support part is too far from a pin and names a hint such as `near: U2.vcc`, write exactly that hard `near` hint for that part in the source and place again, and repeat until the warnings clear or you can say why one cannot. The review lists the worst eight; after you fix them, place again to see the next ones.
2. The power entry owns a corner or an end. The input connector, its protection, the reverse-polarity part and the regulator form one contiguous block; each regulator has its inductor and its input and output capacitors touching it, laid out as its guidance asks, with a tight switching loop. The block must not crowd the processor or the sensitive inputs.
3. The main processor sits near the middle of the board unless routing says otherwise, with its decoupling in a tight ring and its programming or debug header, reset button and boot parts close to it and reachable. Memory and other parts wired mainly to it sit directly adjacent.
4. A peripheral circuit sits beside the connector it serves. When a driver, interface IC or protection network is wired mainly to one connector, place the whole circuit directly beside that connector, on that connector's edge, with its resistors and capacitors around it.
5. Identical circuits are placed alike. When two or more circuits use the same part types and the same connectivity (several output drivers, several identical sensor interfaces), give them the same internal layout, the same orientation, and the same pitch along the edge, so the copies read as copies. Align their connectors on one baseline with even spacing. After every placement, read the placement review: when it says a part of one copy is laid out differently from the same part in the first copy and gives coordinates, pin that part at exactly those coordinates (and rotation) in your placement sidecar and place again, until the copies match or you can say why they cannot.
6. External connectors sit flush on the board edge, the edge their mating face points to, with even spacing between connectors on the same edge. Pin headers may sit a few millimetres inboard. Leave plug and cable clearance.
7. Test points sit with the circuit they measure, within a few millimetres of a part on their net; test points for one interface (debug, programming) form one aligned row by that interface. Keep them probe-accessible and out of the processor's escape area. Ground test points are easy to reach.
8. Orientation and alignment. Put like parts (resistors, capacitors, diodes) on shared rows or columns inside a block, give equivalent transistors matching orientation, align IC edges and connector centres on common horizontal and vertical lines, and keep pin-1 marks identifiable. Do not rotate arbitrarily and do not force one orientation where it worsens the electrical layout.
9. Indicators. Status LEDs with their resistors form one row at an edge, near the part whose state they show.
10. Silkscreen. Each reference designator sits beside its part, in the same orientation and size for the same kind of part, clear of pads, copper and other bodies. Where Synth cannot place text, say so in the report instead of working around it.
11. Routing. Route after placement is final: short and direct, 45-degree bends, widths for the current, return paths preserved, noisy and sensitive nets apart, little congestion near the processor, continuous ground pour, no isolated copper islands.
12. Outline and holes. Keep the mounting holes with clearance and no part under the hardware. Keep part and copper setbacks from the edge. Shrink the outline only when the placement truly allows it.
13. Validation and report. Run ERC and DRC, confirm every schematic connection and the component count are unchanged, review the board in 2D and 3D, and report measured numbers: each decoupling capacitor's distance to its pin, each connector's gap to the board edge, each test point's distance to its circuit, and for every group of identical circuits whether their layouts match. Say plainly what you could not do.

14. Release exports only. A qualification board is a release export. Never set allow_incomplete=true on synth_export or any other tool: it switches off the release gate (footprint guarantee, routing and signal-integrity checks) and labels the board a draft. If the export is blocked, read the finding and fix its cause with the Synth tools (for a part whose footprint is not in the KiCad library, use synth_resolve_footprints and name one KiCad really ships; for a routing finding, change the placement or the part). Only if you truly cannot clear a blocker, stop and report that blocker exactly, instead of exporting a draft.

Begin by inspecting the Synth tools and the registry.
~~~~

## prompt-session7-simplified-board

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session7-simplified-board.md`

~~~~text
Class C case 1 — iCE40 BGA Node: a BGA/fine-pitch board with escape routing and via/fanout rules (Synth MCP)

1. Objective and execution environment

Design a complete four-layer board around a Lattice iCE40HX4K in a 121-ball, 0.8 mm pitch BGA, using the Synth MCP tools only, and export it to KiCad. Board name: ice40_bga_node. Native Synth project: ice40_bga_node.synth in your workspace. The question the board answers (issue #328 under #40): can Synth produce a reviewable board whose BGA is escaped, with a via and fanout strategy that survives export?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- A USB-C receptacle that powers the board (5 V, CC pins with 5.1 kohm pull-downs) and a USB-to-UART bridge (registry part ch340c) to a companion microcontroller, so the board can be programmed with no firmware in it.
- Input protection in a stated order: a resettable fuse first, then a TVS clamp whose standoff is above the maximum input and whose clamp is below what it protects, then a series Schottky or P-FET reverse-polarity stage. Write the numbers: the 5 V maximum, the fuse hold current, the TVS standoff and clamp, the rails' worst-case currents.
- Rails: 3.3 V (AMS1117-3.3 from the registry) for the I/O banks and the companion MCU, and a 1.2 V rail for the FPGA core and PLL. Find a registry regulator for 1.2 V; if none fits, import or author one with a real datasheet_url and say so. The PLL supply is filtered from the core rail with a ferrite bead and capacitors. Every regulator gets its input and output capacitors beside it.
- FPGA: registry part ice40hx4k_bg121. Every supply ball decoupled, ground balls tied to the inner ground plane by vias. The configuration pins (CRESET_B, CDONE, SPI_SS_B, SPI_SCK, SPI_SDI/SDO) wired to an STM32G030 companion MCU (registry stm32g030f6p6) that loads the bitstream over the iCE40 slave SPI and owns the status LEDs. No SPI flash is required.
- Companion MCU: its supply decoupling, a reset button with its network, and a 5-pin SWD header, all within a few millimetres of the MCU.
- GPIO: two FPGA break-out headers (a 1x20 header each, registry header_1x20) carrying as many FPGA I/O as route cleanly, ground and 3.3 V on each, plus an MCU header. Each I/O line has a series resistor.
- Status LEDs with series resistors in one row: power, CDONE, and two user LEDs.
- Two mounting holes. Test points only on the principal rails and the configuration signals.
- Four layers: signal, ground, power, signal. Declare them in the Synth source. The board declares `requires via_in_pad` and a manufacturer profile that offers it (use the language reference and synth_capability to find the exact profile name, for example jlc-filled-via), because a via on a 0.8 mm grid does not fit between balls at the standard tier.

3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; a net whose class does not is treated as fast and needs a ground return via beside every layer change. The FPGA break-out I/O are for slow user signals in this design: put them in a class declared `edge_rate slow` and write that claim in the report. Nets with clock, differential, analog, crystal or switch-node pins stay critical on their own. Do not add test points beyond the rails and configuration signals, and do not add parts whose only job is to fill space.

4. BGA escape and via strategy (the claim under review)

The report must show, from the exported files: how many of the 121 balls are used, how every one is escaped (via in pad on which layer, or surface), that the escape vias use the process minimum drill and size the profile allows, how the ground and power balls reach their planes, where each supply ball's decoupling capacitor sits (distance to the ball), and that the escape ledger accounts for every signal ball. Any ball you leave unused or unconnected is named. The board must route completely and pass KiCad DRC with zero errors in a release export. Never pass allow_incomplete=true.

4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the escape evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

7. Layout quality rules

PCB layout quality rules (mandatory, applies to every board)

Keep the full functionality, the component set, the connectors and the mounting holes. Add no decorative text: no board name, no revision text, no functional headings, no logos. Silkscreen carries only what identifies parts, connectors and test points. Four rules, in this order: compactness (no unnecessary gaps between related parts), grouping (the parts of one circuit sit together), consistency (repeated parts, connectors and identical circuits are aligned and oriented alike), functionality (never trade electrical performance, manufacturability or serviceability for looks). Every gap that remains must have an electrical, mechanical or manufacturing reason.

Work by role, not by name. Decide each circuit's role from its connectivity, then place by these rules:
1. Support parts hug the part they serve. A decoupling capacitor sits within a few millimetres of the supply pin it decouples, a crystal and its load capacitors beside the clock pins, a reset or boot network beside the pin it drives, pull-ups and dividers beside the node they set, a flyback or clamp diode beside the switch it protects. Do not leave a support part standing alone in open board area. After every placement, read the placement review: when it says a support part is too far from a pin and names a hint such as `near: U2.vcc`, write exactly that hard `near` hint for that part in the source and place again, and repeat until the warnings clear or you can say why one cannot. The review lists the worst eight; after you fix them, place again to see the next ones.
2. The power entry owns a corner or an end. The input connector, its protection, the reverse-polarity part and the regulator form one contiguous block; each regulator has its inductor and its input and output capacitors touching it, laid out as its guidance asks, with a tight switching loop. The block must not crowd the processor or the sensitive inputs.
3. The main processor sits near the middle of the board unless routing says otherwise, with its decoupling in a tight ring and its programming or debug header, reset button and boot parts close to it and reachable. Memory and other parts wired mainly to it sit directly adjacent.
4. A peripheral circuit sits beside the connector it serves. When a driver, interface IC or protection network is wired mainly to one connector, place the whole circuit directly beside that connector, on that connector's edge, with its resistors and capacitors around it.
5. Identical circuits are placed alike. When two or more circuits use the same part types and the same connectivity (several output drivers, several identical sensor interfaces), give them the same internal layout, the same orientation, and the same pitch along the edge, so the copies read as copies. Align their connectors on one baseline with even spacing. After every placement, read the placement review: when it says a part of one copy is laid out differently from the same part in the first copy and gives coordinates, pin that part at exactly those coordinates (and rotation) in your placement sidecar and place again, until the copies match or you can say why they cannot.
6. External connectors sit flush on the board edge, the edge their mating face points to, with even spacing between connectors on the same edge. Pin headers may sit a few millimetres inboard. Leave plug and cable clearance.
7. Test points sit with the circuit they measure, within a few millimetres of a part on their net; test points for one interface (debug, programming) form one aligned row by that interface. Keep them probe-accessible and out of the processor's escape area. Ground test points are easy to reach.
8. Orientation and alignment. Put like parts (resistors, capacitors, diodes) on shared rows or columns inside a block, give equivalent transistors matching orientation, align IC edges and connector centres on common horizontal and vertical lines, and keep pin-1 marks identifiable. Do not rotate arbitrarily and do not force one orientation where it worsens the electrical layout.
9. Indicators. Status LEDs with their resistors form one row at an edge, near the part whose state they show.
10. Silkscreen. Each reference designator sits beside its part, in the same orientation and size for the same kind of part, clear of pads, copper and other bodies. Where Synth cannot place text, say so in the report instead of working around it.
11. Routing. Route after placement is final: short and direct, 45-degree bends, widths for the current, return paths preserved, noisy and sensitive nets apart, little congestion near the processor, continuous ground pour, no isolated copper islands.
12. Outline and holes. Keep the mounting holes with clearance and no part under the hardware. Keep part and copper setbacks from the edge. Shrink the outline only when the placement truly allows it.
13. Validation and report. Run ERC and DRC, confirm every schematic connection and the component count are unchanged, review the board in 2D and 3D, and report measured numbers: each decoupling capacitor's distance to its pin, each connector's gap to the board edge, each test point's distance to its circuit, and for every group of identical circuits whether their layouts match. Say plainly what you could not do.

14. Release exports only. A qualification board is a release export. Never set allow_incomplete=true on synth_export or any other tool: it switches off the release gate (footprint guarantee, routing and signal-integrity checks) and labels the board a draft. If the export is blocked, read the finding and fix its cause with the Synth tools (for a part whose footprint is not in the KiCad library, use synth_resolve_footprints and name one KiCad really ships; for a routing finding, change the placement or the part). Only if you truly cannot clear a blocker, stop and report that blocker exactly, instead of exporting a draft.

Begin by inspecting the Synth tools and the registry.

8. Continuing from earlier sessions: a simpler board

Earlier sessions built `ice40_bga_node.synth` and its sidecar here and exported ten times (`export_1` to `export_10`). None released: the core rail `+1V2` and `+3V3` stay unconnected in the independent check, and a few other nets open and close between exports. The best exports were the ones with fewer parts and fewer I/O. The full design (companion MCU, USB-UART bridge, USB-C receptacle, two break-out headers) asks the router for more than it finishes, so the board is simplified. Read `REPORT.md` in this workspace for what the earlier sessions found; do not repeat it.

New, simpler board, same name and files (edit the existing source; keep what already routes):
- Remove the STM32G030 companion, the CH340C USB-UART bridge, the USB-C receptacle with its CC resistors, and their nets, parts and net classes. Remove the entries of removed parts from the placement sidecar.
- Power: a 2-pin 5 V terminal or header (registry part) through the input protection you already have (resettable fuse, TVS, reverse-polarity stage), then the 3.3 V regulator (AMS1117-3.3) and the 1.2 V regulator (`tlv70012_1v2`) with their capacitors beside them, and the PLL supply filtered from the core rail.
- FPGA: `ice40hx4k_bg121`, every supply ball decoupled, ground balls tied to ground by via in pad. Configuration from an SPI flash (registry `w25q128_flash`) wired to the iCE40 master SPI pins (SPI_SS_B, SPI_SCK, SPI_SDI, SPI_SDO), with CRESET_B pulled up and CDONE with its LED.
- One 1x20 header (registry `header_1x20`) carrying ground, 3.3 V and up to 16 FPGA I/O, each with a series resistor; status LEDs (power, CDONE, two user) in one row; two mounting holes; test points on 5 V, 3.3 V, 1.2 V, CDONE and CRESET_B.
- Keep: `escape { method underpad }` on the FPGA, six layers, `manufacturer "jlc-advanced"`, `requires via_in_pad`, no `plane`, the net classes (0.46/0.2 mm vias, 0.127 mm clearance, the 0.25 mm `PWR_CORE` class, all `edge_rate slow`), decoupling at the pins, the layout quality rules.
- Shrink the outline to what the placement needs; a smaller board is acceptable.
Export into a new folder (`export_11`, then `export_12`, ...). Read the blocking reasons after each; fix causes with Synth tools; stop when an export is `routed` and fabrication-ready. Never set allow_incomplete. When released, copy the released export to `export/` and write REPORT.md with what the board is and what was measured (not a list of edits). If after six more exports none releases, write REPORT.md stating exactly what blocks it and stop.

9. Where the simplified board stands (from the last session)

`export_11` of the simplified board is `validation_failed` but very close: two open nets and two DRC errors, nothing else.
- `+3V3`: "pad U1.D6 reaches only 1 of 31 pads", one FPGA VCCIO ball cut off from the rest of the 3.3 V net.
- `CFG_SDO`: "pad U1.K9 reaches only 1 of 2 pads", the SPI data line to the flash.
- DRC: the courtyard of the mounting hole `H1` overlaps the courtyard of the header `J3` (`courtyards_overlap`, `npth_inside_courtyard`). Move `H1` or `J3` apart in the placement sidecar.
A previous session was mid-way through looking at the placement (it had called `synth_describe_placement`) when it stopped. Continue from the workspace: fix the two DRC errors with the sidecar, then give the two open nets room (move the parts of those two nets, or the header, a little; keep decoupling at the pins) and export into `export_12`, then `export_13` and so on, reading the blocking reasons after each. Stop when an export is `routed` and fabrication-ready; then copy it to `export/` and write REPORT.md. Never set allow_incomplete.
~~~~

## prompt-session9-plane-recipe

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session9-plane-recipe.md`

~~~~text
Class C case 1 — iCE40 BGA Node: a BGA/fine-pitch board with escape routing and via/fanout rules (Synth MCP)

1. Objective and execution environment

Design a complete four-layer board around a Lattice iCE40HX4K in a 121-ball, 0.8 mm pitch BGA, using the Synth MCP tools only, and export it to KiCad. Board name: ice40_bga_node. Native Synth project: ice40_bga_node.synth in your workspace. The question the board answers (issue #328 under #40): can Synth produce a reviewable board whose BGA is escaped, with a via and fanout strategy that survives export?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- A USB-C receptacle that powers the board (5 V, CC pins with 5.1 kohm pull-downs) and a USB-to-UART bridge (registry part ch340c) to a companion microcontroller, so the board can be programmed with no firmware in it.
- Input protection in a stated order: a resettable fuse first, then a TVS clamp whose standoff is above the maximum input and whose clamp is below what it protects, then a series Schottky or P-FET reverse-polarity stage. Write the numbers: the 5 V maximum, the fuse hold current, the TVS standoff and clamp, the rails' worst-case currents.
- Rails: 3.3 V (AMS1117-3.3 from the registry) for the I/O banks and the companion MCU, and a 1.2 V rail for the FPGA core and PLL. Find a registry regulator for 1.2 V; if none fits, import or author one with a real datasheet_url and say so. The PLL supply is filtered from the core rail with a ferrite bead and capacitors. Every regulator gets its input and output capacitors beside it.
- FPGA: registry part ice40hx4k_bg121. Every supply ball decoupled, ground balls tied to the inner ground plane by vias. The configuration pins (CRESET_B, CDONE, SPI_SS_B, SPI_SCK, SPI_SDI/SDO) wired to an STM32G030 companion MCU (registry stm32g030f6p6) that loads the bitstream over the iCE40 slave SPI and owns the status LEDs. No SPI flash is required.
- Companion MCU: its supply decoupling, a reset button with its network, and a 5-pin SWD header, all within a few millimetres of the MCU.
- GPIO: two FPGA break-out headers (a 1x20 header each, registry header_1x20) carrying as many FPGA I/O as route cleanly, ground and 3.3 V on each, plus an MCU header. Each I/O line has a series resistor.
- Status LEDs with series resistors in one row: power, CDONE, and two user LEDs.
- Two mounting holes. Test points only on the principal rails and the configuration signals.
- Four layers: signal, ground, power, signal. Declare them in the Synth source. The board declares `requires via_in_pad` and a manufacturer profile that offers it (use the language reference and synth_capability to find the exact profile name, for example jlc-filled-via), because a via on a 0.8 mm grid does not fit between balls at the standard tier.

3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; a net whose class does not is treated as fast and needs a ground return via beside every layer change. The FPGA break-out I/O are for slow user signals in this design: put them in a class declared `edge_rate slow` and write that claim in the report. Nets with clock, differential, analog, crystal or switch-node pins stay critical on their own. Do not add test points beyond the rails and configuration signals, and do not add parts whose only job is to fill space.

4. BGA escape and via strategy (the claim under review)

The report must show, from the exported files: how many of the 121 balls are used, how every one is escaped (via in pad on which layer, or surface), that the escape vias use the process minimum drill and size the profile allows, how the ground and power balls reach their planes, where each supply ball's decoupling capacitor sits (distance to the ball), and that the escape ledger accounts for every signal ball. Any ball you leave unused or unconnected is named. The board must route completely and pass KiCad DRC with zero errors in a release export. Never pass allow_incomplete=true.

4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the escape evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

7. Layout quality rules

PCB layout quality rules (mandatory, applies to every board)

Keep the full functionality, the component set, the connectors and the mounting holes. Add no decorative text: no board name, no revision text, no functional headings, no logos. Silkscreen carries only what identifies parts, connectors and test points. Four rules, in this order: compactness (no unnecessary gaps between related parts), grouping (the parts of one circuit sit together), consistency (repeated parts, connectors and identical circuits are aligned and oriented alike), functionality (never trade electrical performance, manufacturability or serviceability for looks). Every gap that remains must have an electrical, mechanical or manufacturing reason.

Work by role, not by name. Decide each circuit's role from its connectivity, then place by these rules:
1. Support parts hug the part they serve. A decoupling capacitor sits within a few millimetres of the supply pin it decouples, a crystal and its load capacitors beside the clock pins, a reset or boot network beside the pin it drives, pull-ups and dividers beside the node they set, a flyback or clamp diode beside the switch it protects. Do not leave a support part standing alone in open board area. After every placement, read the placement review: when it says a support part is too far from a pin and names a hint such as `near: U2.vcc`, write exactly that hard `near` hint for that part in the source and place again, and repeat until the warnings clear or you can say why one cannot. The review lists the worst eight; after you fix them, place again to see the next ones.
2. The power entry owns a corner or an end. The input connector, its protection, the reverse-polarity part and the regulator form one contiguous block; each regulator has its inductor and its input and output capacitors touching it, laid out as its guidance asks, with a tight switching loop. The block must not crowd the processor or the sensitive inputs.
3. The main processor sits near the middle of the board unless routing says otherwise, with its decoupling in a tight ring and its programming or debug header, reset button and boot parts close to it and reachable. Memory and other parts wired mainly to it sit directly adjacent.
4. A peripheral circuit sits beside the connector it serves. When a driver, interface IC or protection network is wired mainly to one connector, place the whole circuit directly beside that connector, on that connector's edge, with its resistors and capacitors around it.
5. Identical circuits are placed alike. When two or more circuits use the same part types and the same connectivity (several output drivers, several identical sensor interfaces), give them the same internal layout, the same orientation, and the same pitch along the edge, so the copies read as copies. Align their connectors on one baseline with even spacing. After every placement, read the placement review: when it says a part of one copy is laid out differently from the same part in the first copy and gives coordinates, pin that part at exactly those coordinates (and rotation) in your placement sidecar and place again, until the copies match or you can say why they cannot.
6. External connectors sit flush on the board edge, the edge their mating face points to, with even spacing between connectors on the same edge. Pin headers may sit a few millimetres inboard. Leave plug and cable clearance.
7. Test points sit with the circuit they measure, within a few millimetres of a part on their net; test points for one interface (debug, programming) form one aligned row by that interface. Keep them probe-accessible and out of the processor's escape area. Ground test points are easy to reach.
8. Orientation and alignment. Put like parts (resistors, capacitors, diodes) on shared rows or columns inside a block, give equivalent transistors matching orientation, align IC edges and connector centres on common horizontal and vertical lines, and keep pin-1 marks identifiable. Do not rotate arbitrarily and do not force one orientation where it worsens the electrical layout.
9. Indicators. Status LEDs with their resistors form one row at an edge, near the part whose state they show.
10. Silkscreen. Each reference designator sits beside its part, in the same orientation and size for the same kind of part, clear of pads, copper and other bodies. Where Synth cannot place text, say so in the report instead of working around it.
11. Routing. Route after placement is final: short and direct, 45-degree bends, widths for the current, return paths preserved, noisy and sensitive nets apart, little congestion near the processor, continuous ground pour, no isolated copper islands.
12. Outline and holes. Keep the mounting holes with clearance and no part under the hardware. Keep part and copper setbacks from the edge. Shrink the outline only when the placement truly allows it.
13. Validation and report. Run ERC and DRC, confirm every schematic connection and the component count are unchanged, review the board in 2D and 3D, and report measured numbers: each decoupling capacitor's distance to its pin, each connector's gap to the board edge, each test point's distance to its circuit, and for every group of identical circuits whether their layouts match. Say plainly what you could not do.

14. Release exports only. A qualification board is a release export. Never set allow_incomplete=true on synth_export or any other tool: it switches off the release gate (footprint guarantee, routing and signal-integrity checks) and labels the board a draft. If the export is blocked, read the finding and fix its cause with the Synth tools (for a part whose footprint is not in the KiCad library, use synth_resolve_footprints and name one KiCad really ships; for a routing finding, change the placement or the part). Only if you truly cannot clear a blocker, stop and report that blocker exactly, instead of exporting a draft.

Begin by inspecting the Synth tools and the registry.

8. Continuing from earlier sessions, on a Synth that now handles this board

Earlier sessions built `ice40_bga_node.synth` (the simplified board: 5 V terminal, protection, 3.3 V and 1.2 V regulators, the iCE40 with SPI flash, one 1x20 header, LEDs, two mounting holes, test points) and its sidecar, which are in this workspace. They never released, because Synth refused a plane via in a 0.35 mm ball pad and the router left an isolated supply ball. Synth is fixed for both (a plane via in a pad is sized by JLCPCB's via-in-pad land, drill plus 0.15 mm; the ring rule for a filled and capped via in a pad follows that land; a plane-net pad left joined to nothing gets its via).

A probe of this same source with these three changes released (`routed`, 0 open nets, 0 KiCad DRC errors):
- Declare `plane` on the `PWR_3V3` net class only, so `+3V3` is poured on the inner layers. Keep `PWR_CORE` (the 1.2 V core rail) without `plane`. Give `VCCPLL` its own net class (same width and clearance as `PWR_CORE`, `edge_rate slow`, no `plane`).
- Put the two mounting holes at opposite corners (for example `H1` bottom-left and `H2` top-right) instead of two bottom corners, so no hole keepout meets a supply via.
- Declare `escape { method underpad }` on the FPGA with no explicit `layers`.
- Keep six layers, `manufacturer "jlc-advanced"`, `requires via_in_pad`, and the 0.46/0.2 mm net-class vias.

Apply these through the Synth tools, export into `export_1` (a fresh folder), read the routing report and the blocking reasons, and fix causes with the Synth tools. Stop when an export is `routed` and fabrication-ready. Never set allow_incomplete. When released, copy the released export to `export/` and write REPORT.md with what the board is and what was measured (decoupling distances, connector gaps, the escape ledger, the plane vias), not a list of edits. If after four exports none releases, write the exact blocking reasons in REPORT.md and stop.
~~~~

## prompt-session10-no-connect

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session10-no-connect.md`

~~~~text
Class C case 1 — iCE40 BGA Node, second session: schematic completeness (Synth MCP)

Follow only this prompt. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Work only through the Synth tools, never edit exported KiCad files, never set allow_incomplete=true, and do not invent tool results or datasheet values. Where Synth does something wrong, report it exactly.

The workspace holds the board you released last session: ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/, export/ and REPORT.md. The board is routed and fabrication-ready (0 open nets, KiCad DRC 0 errors). Do not change its routing, placement, net classes, stackup or any wired net.

KiCad ERC on the exported schematic, run with the symbol libraries configured, reports 21 errors that name pins left with no connection and no no-connect marker:
- U1 pins K5 (IOB_79), L5 (IOB_81_GBIN5), K6 (IOB_82_GBIN4), J7 (IOB_86), H7 (IOB_87), K7 (IOB_89), J8 (IOB_91), F4 (IOL_13A), F3 (IOL_13B_GBIN7), G1 (IOL_14A_GBIN6), G2 (IOL_14B), G3 (IOL_17A), H3 (IOL_17B), H1 (IOL_18A), H2 (IOL_18B), J1 (IOL_23A), K1 (IOL_23B), K2 (IOL_25A), J2 (IOL_25B): unused I/O balls;
- U1 pin B10 (VPP_FAST), declared not connected, which ERC also reports as a power input that nothing drives.

Task:
1. Using Synth's language reference and tools, declare each of those unused pins as deliberately not connected, the way Synth expresses it, so the schematic states the intent. Do it for the 19 unused I/O pins and for VPP_FAST. Do not wire them to anything and do not add parts.
2. Validate, then export into a fresh folder export_2 (synth_export, no allow_incomplete) and read the routing report. The result must still be routed and fabrication-ready with the same connected nets (29), 0 open nets and 0 DRC errors. If the export changes the routing, report it exactly and stop.
3. When export_2 matches, copy it to export/ and add a short section to REPORT.md saying what the schematic now declares and what Synth did (what the no-connect declaration produced in the schematic, and anything surprising). Describe the board as it is; do not write a list of edits.

If Synth cannot express a deliberate no-connect on a pin, say exactly what you tried and what the tool answered, and stop.
~~~~

## prompt-session11-package-fixes

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session11-package-fixes.md`

~~~~text
Class C case 1 — iCE40 BGA Node, third session: two part packages that do not match the parts (Synth MCP)

Follow only this prompt. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Work only through the Synth tools, never edit exported KiCad files, never set allow_incomplete=true, and do not invent tool results, part numbers or datasheet values. Where Synth does something wrong, report it exactly.

The workspace holds the board released earlier: ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/, export/ and REPORT.md. It is routed and fabrication-ready (29 nets connected, 0 open, KiCad DRC 0 errors, six layers, manufacturer jlc-advanced, `plane` on PWR_3V3). Two parts were checked against their manufacturer pages and distributor listings and their packages do not match the footprints on the board:

1. U2, the SPI flash. The board uses registry part `w25q128_flash` (Winbond W25Q128JVSIQ) with footprint `Package_SO:SOIC-8_3.9x4.9mm_P1.27mm`. The LCSC, Arrow, RS and JITX listings for W25Q128JVSIQ all give an SOIC-8 208 mil package with a 5.3 mm body, 7.7 mm lead span, 1.27 mm pitch. Add a part to `user-registry/` with a new id `w25q128jvsiq_208mil`: a copy of `w25q128_flash` (same symbol, same pin map, same electrical data, same mpn W25Q128JVSIQ) with `kicad_footprint = "Package_SO:SOIC-8_5.3x5.3mm_P1.27mm"` and `footprint_dimensions` width_mm 7.7, height_mm 5.3 (keep the courtyard margin). Use it for U2 in the source.
2. U4, the 1.2 V regulator. The board's user-registry part `tlv70012_1v2` names TLV70012DBVR and footprint `Package_TO_SOT_SMD:SOT-23-5`, and cites datasheet SBVS173. LCSC lists the TLV70012 only as TLV70012DDCR (SOT-23-THIN, TSOT-23-5), and TI's TLV700xx datasheet is SLVSA00. The DDC pinout is the same: 1 IN, 2 GND, 3 EN, 4 NC, 5 OUT. Replace it with a user-registry part `tlv70012_ddc`: same pins and electrical data, mpn TLV70012DDCR, `kicad_footprint = "Package_TO_SOT_SMD:TSOT-23-5"`, description naming the DDC package and the datasheet SLVSA00. Use it for U4.

Keep every net, net class, the stackup, the declarations from earlier sessions and the rest of the placement. The new footprints are larger: if the export is blocked or leaves nets open, adjust the sidecar placement of U2, U4 and the parts beside them (their decoupling stays at the pins) and export again. Use at most four exports, each into a fresh folder export_3, export_4 and so on.

Stop when an export is routed and fabrication-ready with 0 open nets and 0 DRC errors. Copy it to export/ and update REPORT.md: state the two parts and their packages as the board has them, and what was measured (decoupling distances for the parts you moved). Describe the board as it is; do not write a list of edits. If after four exports none releases, write the exact blocking reasons in REPORT.md and stop.
~~~~

## prompt-session12-dense-board

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session12-dense-board.md`

~~~~text
Class C case 1 — iCE40 BGA Node: a BGA/fine-pitch board with escape routing and via/fanout rules (Synth MCP)

1. Objective and execution environment

Design a complete four-layer board around a Lattice iCE40HX4K in a 121-ball, 0.8 mm pitch BGA, using the Synth MCP tools only, and export it to KiCad. Board name: ice40_bga_node. Native Synth project: ice40_bga_node.synth in your workspace. The question the board answers (issue #328 under #40): can Synth produce a reviewable board whose BGA is escaped, with a via and fanout strategy that survives export?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- A USB-C receptacle that powers the board (5 V, CC pins with 5.1 kohm pull-downs) and a USB-to-UART bridge (registry part ch340c) to a companion microcontroller, so the board can be programmed with no firmware in it.
- Input protection in a stated order: a resettable fuse first, then a TVS clamp whose standoff is above the maximum input and whose clamp is below what it protects, then a series Schottky or P-FET reverse-polarity stage. Write the numbers: the 5 V maximum, the fuse hold current, the TVS standoff and clamp, the rails' worst-case currents.
- Rails: 3.3 V (AMS1117-3.3 from the registry) for the I/O banks and the companion MCU, and a 1.2 V rail for the FPGA core and PLL. Find a registry regulator for 1.2 V; if none fits, import or author one with a real datasheet_url and say so. The PLL supply is filtered from the core rail with a ferrite bead and capacitors. Every regulator gets its input and output capacitors beside it.
- FPGA: registry part ice40hx4k_bg121. Every supply ball decoupled, ground balls tied to the inner ground plane by vias. The configuration pins (CRESET_B, CDONE, SPI_SS_B, SPI_SCK, SPI_SDI/SDO) wired to an STM32G030 companion MCU (registry stm32g030f6p6) that loads the bitstream over the iCE40 slave SPI and owns the status LEDs. No SPI flash is required.
- Companion MCU: its supply decoupling, a reset button with its network, and a 5-pin SWD header, all within a few millimetres of the MCU.
- GPIO: two FPGA break-out headers (a 1x20 header each, registry header_1x20) carrying as many FPGA I/O as route cleanly, ground and 3.3 V on each, plus an MCU header. Each I/O line has a series resistor.
- Status LEDs with series resistors in one row: power, CDONE, and two user LEDs.
- Two mounting holes. Test points only on the principal rails and the configuration signals.
- Four layers: signal, ground, power, signal. Declare them in the Synth source. The board declares `requires via_in_pad` and a manufacturer profile that offers it (use the language reference and synth_capability to find the exact profile name, for example jlc-filled-via), because a via on a 0.8 mm grid does not fit between balls at the standard tier.

3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; a net whose class does not is treated as fast and needs a ground return via beside every layer change. The FPGA break-out I/O are for slow user signals in this design: put them in a class declared `edge_rate slow` and write that claim in the report. Nets with clock, differential, analog, crystal or switch-node pins stay critical on their own. Do not add test points beyond the rails and configuration signals, and do not add parts whose only job is to fill space.

4. BGA escape and via strategy (the claim under review)

The report must show, from the exported files: how many of the 121 balls are used, how every one is escaped (via in pad on which layer, or surface), that the escape vias use the process minimum drill and size the profile allows, how the ground and power balls reach their planes, where each supply ball's decoupling capacitor sits (distance to the ball), and that the escape ledger accounts for every signal ball. Any ball you leave unused or unconnected is named. The board must route completely and pass KiCad DRC with zero errors in a release export. Never pass allow_incomplete=true.

4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

ice40_bga_node.synth, ice40_bga_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the escape evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

7. Layout quality rules

PCB layout quality rules (mandatory, applies to every board)

Keep the full functionality, the component set, the connectors and the mounting holes. Add no decorative text: no board name, no revision text, no functional headings, no logos. Silkscreen carries only what identifies parts, connectors and test points. Four rules, in this order: compactness (no unnecessary gaps between related parts), grouping (the parts of one circuit sit together), consistency (repeated parts, connectors and identical circuits are aligned and oriented alike), functionality (never trade electrical performance, manufacturability or serviceability for looks). Every gap that remains must have an electrical, mechanical or manufacturing reason.

Work by role, not by name. Decide each circuit's role from its connectivity, then place by these rules:
1. Support parts hug the part they serve. A decoupling capacitor sits within a few millimetres of the supply pin it decouples, a crystal and its load capacitors beside the clock pins, a reset or boot network beside the pin it drives, pull-ups and dividers beside the node they set, a flyback or clamp diode beside the switch it protects. Do not leave a support part standing alone in open board area. After every placement, read the placement review: when it says a support part is too far from a pin and names a hint such as `near: U2.vcc`, write exactly that hard `near` hint for that part in the source and place again, and repeat until the warnings clear or you can say why one cannot. The review lists the worst eight; after you fix them, place again to see the next ones.
2. The power entry owns a corner or an end. The input connector, its protection, the reverse-polarity part and the regulator form one contiguous block; each regulator has its inductor and its input and output capacitors touching it, laid out as its guidance asks, with a tight switching loop. The block must not crowd the processor or the sensitive inputs.
3. The main processor sits near the middle of the board unless routing says otherwise, with its decoupling in a tight ring and its programming or debug header, reset button and boot parts close to it and reachable. Memory and other parts wired mainly to it sit directly adjacent.
4. A peripheral circuit sits beside the connector it serves. When a driver, interface IC or protection network is wired mainly to one connector, place the whole circuit directly beside that connector, on that connector's edge, with its resistors and capacitors around it.
5. Identical circuits are placed alike. When two or more circuits use the same part types and the same connectivity (several output drivers, several identical sensor interfaces), give them the same internal layout, the same orientation, and the same pitch along the edge, so the copies read as copies. Align their connectors on one baseline with even spacing. After every placement, read the placement review: when it says a part of one copy is laid out differently from the same part in the first copy and gives coordinates, pin that part at exactly those coordinates (and rotation) in your placement sidecar and place again, until the copies match or you can say why they cannot.
6. External connectors sit flush on the board edge, the edge their mating face points to, with even spacing between connectors on the same edge. Pin headers may sit a few millimetres inboard. Leave plug and cable clearance.
7. Test points sit with the circuit they measure, within a few millimetres of a part on their net; test points for one interface (debug, programming) form one aligned row by that interface. Keep them probe-accessible and out of the processor's escape area. Ground test points are easy to reach.
8. Orientation and alignment. Put like parts (resistors, capacitors, diodes) on shared rows or columns inside a block, give equivalent transistors matching orientation, align IC edges and connector centres on common horizontal and vertical lines, and keep pin-1 marks identifiable. Do not rotate arbitrarily and do not force one orientation where it worsens the electrical layout.
9. Indicators. Status LEDs with their resistors form one row at an edge, near the part whose state they show.
10. Silkscreen. Each reference designator sits beside its part, in the same orientation and size for the same kind of part, clear of pads, copper and other bodies. Where Synth cannot place text, say so in the report instead of working around it.
11. Routing. Route after placement is final: short and direct, 45-degree bends, widths for the current, return paths preserved, noisy and sensitive nets apart, little congestion near the processor, continuous ground pour, no isolated copper islands.
12. Outline and holes. Keep the mounting holes with clearance and no part under the hardware. Keep part and copper setbacks from the edge. Shrink the outline only when the placement truly allows it.
13. Validation and report. Run ERC and DRC, confirm every schematic connection and the component count are unchanged, review the board in 2D and 3D, and report measured numbers: each decoupling capacitor's distance to its pin, each connector's gap to the board edge, each test point's distance to its circuit, and for every group of identical circuits whether their layouts match. Say plainly what you could not do.

14. Release exports only. A qualification board is a release export. Never set allow_incomplete=true on synth_export or any other tool: it switches off the release gate (footprint guarantee, routing and signal-integrity checks) and labels the board a draft. If the export is blocked, read the finding and fix its cause with the Synth tools (for a part whose footprint is not in the KiCad library, use synth_resolve_footprints and name one KiCad really ships; for a routing finding, change the placement or the part). Only if you truly cannot clear a blocker, stop and report that blocker exactly, instead of exporting a draft.

Begin by inspecting the Synth tools and the registry.

8. Continuing: a board that routes, to be made dense

The workspace holds `ice40_bga_node.synth`, its placement sidecar and `user-registry/`. It is the simplified iCE40 board from earlier sessions: 5 V terminal and protection, the 3.3 V and 1.2 V rails, the iCE40 BGA with its decoupling, an SPI boot flash, one header, status LEDs, test points and two mounting holes. Parts that earlier sessions found did not match their packages are corrected: the flash is the user-registry part `w25q128jvsiq_208mil` (Winbond W25Q128JVSIQ, SOIC-8 208 mil), the 1.2 V regulator is `tlv70012_ddc` (TLV70012DDCR, TSOT-23-5), and the header `J3` is the registry `header_1x12` (it uses pins 1, 2, 5, 8, 9 and 10). Do not change the part set or any net.

Synth's router now keeps a via in a pad where the board requires via-in-pad on a profile that offers it, and sizes a plane via in a 0.35 mm ball pad by the manufacturer's via-in-pad land. The board declares `manufacturer "jlc-advanced"`, six layers, `requires via_in_pad`, `plane` on the 3.3 V class only, `VCCPLL` in its own class, and `escape { method underpad }` on the FPGA. Keep all of that.

Two jobs, in this order:

1. **Route completely.** Export the workspace as it is into `export_1` (a fresh folder). The last test export of this placement left one net open (`VBUS_F`, pad `F1.2`, the fuse) with 0 KiCad errors, and earlier placements closed everything but left other single nets open. Read the blocking reasons and fix causes with the Synth tools (placement of the nearby parts, not removing nets). Stop when an export is `routed` and fabrication-ready with 0 open nets and 0 DRC errors.

2. **Make it dense: courtyard fill of at least 30 %.** The routed board is 51.0 x 43.0 mm (2,192 mm2) at 25 % courtyard fill measured on the exported board; 30 % needs the outline at most about 1,840 mm2 with the same parts, for example about 46 x 40 mm. `synth_compact_layout` did not shrink it: every scale step it tried gave the same 51 x 43 mm outline, because the exporter re-derives positions and the outline from the parts, so editing the sidecar by hand does not move parts the way the file's coordinates suggest. Use the placement tools instead. `synth_place_with_hints` takes an explicit `board_width_mm` and `board_height_mm`: ask for a smaller board (try 48 x 41 mm, then 46 x 40 mm), let the placer fit the parts, read the placement review, and export the result into a fresh folder. Keep connectors on the board edge, decoupling at the pins, the mounting holes at opposite corners clear of the header and of copper, and the parts of one circuit together. A mounting hole that lands far from the parts makes the outline larger: if that happens, report it and move the hole with a hint. Keep every export that is routed with 0 open nets and 0 DRC errors, and report the board size and the courtyard fill you measure from the exported board for each.

Use at most eight exports in all. When the densest routed export is found, copy it to `export/` and write REPORT.md: what the board is, the fill and outline measured, the decoupling distances and connector gaps. Never set allow_incomplete. If after eight exports none is both routed and at least 30 % dense, keep the densest routed one and say exactly what blocked the rest.
~~~~

## prompt-session13-usb-mcu-revision

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session13-usb-mcu-revision.md`

~~~~text
Class C case 1, revision with a USB-C input and a companion MCU (Synth MCP)

You continue a finished board, the iCE40HX4K BGA node `ice40_bga_node` (issue #328 under #40). Its Synth source is `ice40_bga_node.synth` in your workspace; the exported board is routed and released as it stands. The exported board has a large empty area to the left of the FPGA U1. Add two functions in that area.

Follow only this prompt. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it. Never set allow_incomplete. You have at most 6 exports; export into a fresh directory each time (export_1, export_2, ...).

1. What to add
- A USB-C receptacle (registry usb_c_receptacle) that is the board's 5 V input in place of the 2-pin power header J1: 5 V from VBUS goes through the existing polyfuse, TVS and P-FET reverse stage (keep that order). CC1 and CC2 each get a 5.1 kohm pull-down to ground. The shield goes to ground. USB-C is a connector: it must be flush with a board edge like the other connectors.
- A companion MCU, the registry-compatible STM32F103C8T6 in `user-registry/stm32f103c8_usb.synth.toml` (LQFP-48, USB full speed), powered from the 3.3 V rail, with: every supply pin decoupled within a few millimetres, a reset button and its network, a 8 MHz crystal (registry xtal_8mhz_3225_4p) with load capacitors computed from the crystal's load capacitance and a stated stray capacitance (write the arithmetic), a 5-pin SWD header (registry header_1x5_swd), BOOT0 pulled low, and USB D+ and D- to the receptacle with 22 ohm series resistors.
- The MCU's role: it programs the SPI flash U2 over SPI and controls the FPGA configuration. Wire MCU SPI1 (PA4 NSS, PA5 SCK, PA6 MISO, PA7 MOSI) to the flash's CS, CLK, DO and DI; wire two MCU GPIOs to the FPGA CRESET_B and CDONE. The iCE40 bitstream is 135,183 bytes, larger than the MCU's 64 KB flash, so the flash U2 stays the bitstream store and the MCU only streams it from the host. Say this in REPORT.md.
- Keep every existing function of the board: FPGA, flash, rails, LEDs, FPGA header, test points, mounting holes.

2. Rules
- Parts that are not ball grids: declare `escape { off }` on the USB-C receptacle and on the MCU (LQFP-48), as `component U5: mcu "..." { escape { off } }`. Only U1 is escaped as a ball grid.
- Net classes: a USB net is critical for the return-via rule and every layer change of a fast or unclassed net needs a ground return via beside it. Declare every other net class `edge_rate slow`. Declare D+/D- as a differential pair with the USB class the language allows.
- Every part in a named group (add groups "USB INPUT" and "MCU"), no OTHER region. Placement: use placement_hint near for decoupling and the crystal; the new parts belong in the free area left of U1, and the board outline must not grow. Check the exported courtyard fill and the outline against the previous board (44 x 40 mm).
- Part packages are checked against datasheets: do not change a package to make routing easier.

3. Process
- Read ice40_bga_node.synth first. Edit it in place. Validate with synth_validate after each stage, then export.
- Report in REPORT.md: the design decisions, the crystal arithmetic, the net classes and why each is slow or fast, the measured result of each export (state, DRC errors, open nets, blocking reasons, outline and fill), every Synth defect or limitation hit, and what you could not do. Do not write a README.md.
~~~~

## prompt-session14-usb-mcu-revision-run2

Source in history: `qualification/class-c/ice40-bga-node/agent-run/prompts/prompt-session14-usb-mcu-revision-run2.md`

~~~~text
Class C case 1, revision with a USB-C input and a companion MCU: second run (Synth MCP)

You continue a board revision. `ice40_bga_node.synth` in your workspace is the iCE40HX4K BGA node with a USB-C receptacle, an STM32F103C8 companion MCU (crystal, reset, SWD, SPI to the flash, CRESET_B/CDONE control) and everything the released board had. A previous run of this revision wrote the source; its report is `REPORT-previous-run.md` (read it first). That run did not release: export 3 routed every net with 0 KiCad DRC errors but had one return-via blocker, a signal via at about (9.40, 30.00) mm in the crystal pad Y1.1, and the board grew to 113.5 x 82.9 mm because the workspace had no placement sidecar.

This workspace now has the sidecar `ice40_bga_node.placement.layout.toml` of the released board (44 x 40 mm): it pins the parts that the released board had, in board millimetres, as courtyard centres. J1 (the old 2-pin power header) is no longer in the source: remove its entry. The USB-C receptacle and the MCU group (the parts that are new in the source) have no entry: the free area is to the left of the FPGA U1, and the new parts belong there. Add sidecar entries for the new parts (source "agent", priority "hard", their courtyard centres), keep every pinned part where it is, and keep the board outline no larger than about 50 x 45 mm. The USB-C receptacle is a connector: flush with a board edge within 0.3 mm. Check the exported outline and the courtyard fill against the released board (44 x 40 mm, 31.4 %).

Follow only this prompt. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers or datasheet values. Work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it. Never set allow_incomplete. You have at most 6 exports; export into a fresh directory each time (export_1, export_2, ...). Start with synth_describe_placement on the sidecar before routing, and revise the sidecar if the placement review asks for it.

Goals, in order: (1) a release: routed, fabrication-ready, 0 KiCad DRC errors, no via in a pad, no blocking finding, including the return-via blocker at the crystal; moving the crystal and its load capacitors next to the MCU's oscillator pins, away from the pad that carries the via, is the first thing to try; (2) outline and fill as above; (3) REPORT.md with the measured result of each export (state, DRC errors, open nets, blocking reasons, outline, fill), the crystal arithmetic, what you changed in the sidecar and why, every Synth defect or limitation you hit and what you could not do. Do not write a README.md.
~~~~
