Class C case 2 — iCE40 SDRAM Node: a DDR-class length- and impedance-sensitive memory interface (Synth MCP)

1. Objective and execution environment

Design a complete six-layer board using the Synth MCP tools only, and export it to KiCad. Board name: ice40_sdram_node. Native Synth project: ice40_sdram_node.synth in your workspace. The question the board answers (issue #329 under #40): can Synth produce a reviewable board whose memory interface carries declared length budgets and a controlled impedance, and are those budgets measured on the routed copper after export?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- FPGA: registry part ice40hx4k_bg121 as the memory controller (soft controller; do not claim a hard DDR PHY) with registry part is42s16400j_sdram (SDR SDRAM, 16-bit data bus, 12-bit address, bank, RAS, CAS, WE, clock and byte masks) wired to it. If you use another memory part, import or author it with a real datasheet_url and say so.
- Interface constraints, declared in the Synth source and surviving export: a net class for the data bus, one for the address and command group, one for the clock; length-match groups (match_length) with a stated budget in millimetres for the data bus against its clock, and for the address group; single-ended trace width and spacing chosen for a stated target impedance from the six-layer stackup you declare (give the stackup, the dielectric height, the arithmetic and the assumptions); series termination resistors at the controller end with their values reasoned; the clock routed as the shortest, with its return path preserved.
- Configuration: an SPI boot flash wired to the iCE40 master SPI pins (SPI_SS_B, SPI_SCK, SPI_SDI, SPI_SDO), with CRESET_B pulled up and CDONE with its LED. Use the user-registry part `w25q128jvsiq_208mil` (Winbond W25Q128JVSIQ, SOIC-8 208 mil footprint), already in your workspace. There is no companion microcontroller and no USB-UART bridge on this board; the question under review is the memory interface. The board is stand-alone: it takes its power from a USB-C receptacle and is programmed through a 1x6 programming header (SPI_SS_B, SPI_SCK, SPI_SDI, SPI_SDO, CRESET_B, GND). Do not claim a DDR interface: the memory is SDR SDRAM and the README says so.
- Rails: 3.3 V (AMS1117-3.3) for I/O and the SDRAM, 1.2 V core (user-registry part `tlv70012_ddc`, TLV70012DDCR, footprint TSOT-23-5, already in your workspace), the PLL supply filtered; every supply decoupled at the pin; the SDRAM bank VDD/VDDQ decoupling at the pins. The iCE40 part is the user-registry `ice40hx4k_bg121_nc` (the registry part with VPP_FAST left unconnected), already in your workspace.
- Six layers, declared in the source with a stackup, `manufacturer "jlc-advanced"` and `requires via_in_pad` (see section 8).
- Input power and protection as on a Class A board: a USB-C receptacle (registry part `usb_c_receptacle`) used for 5 V power only: VBUS and GND, a 5.1 kohm pull-down on each of CC1 and CC2, the data pins left unconnected, the shield tied to ground; then a resettable fuse first, a TVS clamp with standoff above the maximum input and clamp below what it protects, then a reverse-polarity stage. Write the numbers.
- Every regulator gets its input and output capacitors beside it; every supply pin of every IC is decoupled at the pin; every rail has a test point; a status LED row with series resistors.
- Two mounting holes at opposite corners, test points on the principal rails, CDONE and CRESET_B, and a status LED row.


3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; leave only the genuinely fast interface nets without it. Every net must belong to a declared net class (a net with no class counts as critical for the return-via rule). Give each layer change on a fast net a ground return via beside it.

4. The claim under review

The report must show, from the exported files: the length of every member of every matched group measured on the exported routed copper, the group skews against the declared budgets, the impedance arithmetic and the stackup, the clock length, and for every interface net the layer changes and their return vias. Passing generic ERC and DRC is not qualification: give the interface-specific evidence, with measured numbers.

4b. Connectors. The USB-C receptacle sits flush on one board edge with its opening facing out; the 1x6 programming header sits near the FPGA and the SPI flash on an edge, and is a 0.1 inch pin header: use a registry header part if one has six pins, otherwise author a 1x6 header part in user-registry with a real datasheet_url and say so.
4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

ice40_sdram_node.synth, ice40_sdram_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

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


8. What earlier sessions on a sister board (an iCE40HX4K BGA node, same FPGA) established

That board released on this Synth with these settings, and they apply here:
- `manufacturer "jlc-advanced"` on six layers (signal, ground, signal, signal, ground, signal), `requires via_in_pad`, net-class vias 0.46/0.2 mm, 0.127 mm clearance.
- Declare `plane` on the 3.3 V rail net class only. Do not declare `plane` on the 1.2 V core class. The PLL supply (VCCPLL) gets its own net class (no `plane`). With two plane rails every inner layer becomes a plane and the BGA escape has no signal layer.
- Declare `escape { method underpad }` on the FPGA with no explicit `layers`. Parts that are not ball grids (SOIC flash, TSOP SDRAM, SOT-23 regulator) are not escaped by that stage: if the export reports a failed escape for one of them, declare `escape { off }` on it.
- Mounting holes at opposite corners.
- Registry parts with a footprint that does not match their package are a known problem: use the parts named above.
Where the interface needs more routing room than a six-layer board gives, grow the outline before you remove interface nets.

Export into fresh folders (export_1, export_2, ...), read the blocking reasons after each, and stop when an export is routed and fabrication-ready. Never set allow_incomplete. Use at most six exports. When released, copy it to export/ and write REPORT.md with the measured interface evidence of section 4 and what the board is, not a list of edits.

9. Continuing: the SDRAM clock source

An earlier session wrote `ice40_sdram_node.synth` here (six layers, jlc-advanced, nine net classes with 50 ohm, `max_skew` and `max_length`, the rails, the FPGA, the SDRAM, the SPI boot flash, LEDs, test points, two mounting holes) and stopped at one validation error: `E-SYNTH-CLOCK-002`, a clock input (the SDRAM CLK pin) with no clock source on its net, because the iCE40 part had no pin that declares a clock output.

The user-registry part `ice40hx4k_bg121_nc` now declares ball B6 (pin `iot_198_gbin0`, a global-buffer I/O) with electrical type `clock` and the capability `clock_output`, so it is a clock source. Drive the SDRAM clock from that pin, directly or through the series resistor you intended, and validate. Do not edit the registry part again. Keep everything else; then continue the procedure: placement review, export into export_1, and so on (at most six exports). Report in REPORT.md any other Synth limit you meet, exactly.


10. Where the board stands now (latest session)

The source in this workspace, exported on the current Synth binary (which keeps a via in a pad where via-in-pad is allowed, and sizes a plane via in a 0.35 mm ball pad by JLCPCB's via-in-pad land), gives `validation_failed` with 64 of 65 nets connected and these remaining:
- One open net: `SD_DQ8`, pad `U3.G11` reaches only 1 of 2 pads.
- 14 KiCad DRC errors, nearly all in the SDRAM fanout region between the FPGA `U3` and the SDRAM `U4`: clearance violations and shorts between data-bus tracks on the inner layers and escape vias of neighbouring bus nets (for example `SD_DQ3` track on In2 against the `SD_DQ15` via at 0.119 mm against 0.127 mm, `SD_CKE_F` via against the `SD_UDQM` track on In3 at 0.011 mm).
- The board is 88 x 59 mm; its courtyard fill is 15 %. Density comes after it routes.

Fix the cause with the Synth tools, not by removing nets: the net classes of the SDRAM nets (track width and clearance, within the 0.127 mm process floor and the impedance target you declared), the placement of `U4` relative to `U3` (closer, with the address and data lanes facing each other, so the bus needs fewer layer changes), and which inner layers the escape and the bus use. Read the blocking reasons after each export, use at most six exports (`export_2` onward), and stop when an export is `routed` and fabrication-ready with 0 open nets and 0 DRC errors. Then copy it to `export/` and write REPORT.md with the interface evidence of section 4. If none routes in six, keep the best and say what blocked it.
