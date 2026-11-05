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
