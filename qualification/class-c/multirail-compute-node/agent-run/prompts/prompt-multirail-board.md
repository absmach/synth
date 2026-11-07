Class C case 4 — Multi-rail Compute Node: thermal, keepout, assembly-access and DFM constraints (Synth MCP)

1. Objective and execution environment

Design a complete four-layer board using the Synth MCP tools only, and export it to KiCad. Board name: multirail_compute_node. Native Synth project: multirail_compute_node.synth in your workspace. The question the board answers (issue #331 under #40): can Synth produce a reviewable multi-rail compute board whose thermal paths, keepouts, assembly access and DFM limits are declared and visible in the exported output?

Follow only this prompt: any other instructions in your context about other environments, schedules or pull requests do not apply. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Discover the Synth operations from their actual tool descriptions, the language reference and the server instructions. If a required operation is unavailable, complete what is supported and say what is not.

This board is a qualification board. An independent reviewer will check the way it was made, so work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it.

2. What the board must contain

- A compute core: a registry microcontroller with external memory (registry w25q128_flash, registry 23lc1024_sram or is42s16400j_sdram as the processor supports) and a USB-C receptacle for power and data; pick the processor from the registry (list the mcus and choose one that fits, say why).
- Multiple rails from one 5 V to 12 V input: a buck for the high-current rail (registry lmr33640 or mp2307_buck_ep, with its inductor, input and output capacitors, feedback divider and compensation as its datasheet asks, the divider arithmetic written), an LDO for 3.3 V and a registry or imported 1.8 V and 1.2 V LDO (tlv70012_1v2, ams1117_1v8) where the processor needs them; a load switch with an enable sequence between rails; write the power budget per rail with the worst-case currents and the dissipation of every linear regulator.
- Thermal: every thermal pad tied to the ground plane with a via array (count, drill and pitch stated), thermal relief and copper area declared per dissipating part, a keepout around hot parts; heat-source parts spread apart; state the junction-temperature estimate from the datasheet thermal resistance and the dissipation.
- Keepouts and assembly access: declared keepouts under the antenna or crystal as relevant, around mounting hardware, and a tooling and fiducial clearance rule; connectors on the board edge with plug clearance; parts 3 mm or more from the edge; through-hole to SMD pad distance of at least 2.1 mm; programming header and test points reachable.
- DFM: the manufacturer profile's rules honoured, silkscreen at least 0.15 mm wide and 0.15 mm from pads, none over vias or holes, via annular ring at least 0.20 mm (0.6/0.2 mm vias), no part under the mounting hardware.
- Four layers: signal, ground, power, signal, declared in the source. If the part count passes the placer ceiling (#257), pin a floorplan in the placement sidecar.
- Input power and protection as on a Class A board: a USB-C receptacle or a barrel/terminal input as the case needs, a resettable fuse first, a TVS clamp with standoff above the maximum input and clamp below what it protects, then a reverse-polarity stage. Write the numbers.
- Every regulator gets its input and output capacitors beside it; every supply pin of every IC is decoupled at the pin; every rail has a test point; a status LED row with series resistors.
- Two mounting holes, an SWD or programming header near the processor, a reset button network.


3. Net classes and the return-path rule

Declare every net class with its trace width, clearance, via size and drill, and apply it to nets. Every net class that carries only rails or static or low-speed signals must declare `edge_rate slow`; leave only the genuinely fast interface nets without it. Every net must belong to a declared net class (a net with no class counts as critical for the return-via rule). Give each layer change on a fast net a ground return via beside it.

4. The claim under review

The report must show, from the exported files: the thermal estimate per dissipating part, the via arrays under each thermal pad (counts measured on the export), the declared keepouts and where they appear in the exported board, the edge and assembly clearances measured on the export, and the DFM numbers. Passing generic ERC and DRC is not qualification: give the interface-specific evidence, with measured numbers.

4a. Parts that are not ball grids. Synth's grid-array escape stage reads any part with three or more rows and columns of pads at 1.3 mm pitch or finer as a ball grid, so a USB-C receptacle, a QFN, an SOIC with an exposed pad or an LQFP can fail the export with a failed escape although it is not a BGA. If the export reports that, declare `escape { off }` on that component in the Synth source (`component U1: kind "part" { escape { off } }`; see docs/diagnostics/E-SYNTH-PARSE-040.md) and say so in the report. Only the FPGA is escaped as a ball grid.

4b. Findings from the earlier boards of this series (use them, do not rediscover them)
- Manufacturer profile names must be exact. `jlc-standard` (2 to any layers, no via-in-pad), `jlc-filled-via` (2 and 4 layers, via-in-pad), `jlc-advanced` (4 and 6 layers, via-in-pad, 0.2 mm drill). Any other spelling silently falls back to the standard tier. This board has no ball grid, so `jlc-standard` on four layers is the right choice unless a part forces more.
- Rails on planes work when no part's pad is a fine-pitch ball: declare `plane` on the net classes of the rails that should be carried on an inner layer (for example the 3.3 V and the buck output); Synth then pours each on its own inner layer, puts ground on the outer layers and fans each plane pad out with a via beside it. A plane via beside a pad needs a 0.6 mm via and room for it; a pad with no room is refused with the pad named.
- Net-class vias must be at least the process floor (0.46 mm over a 0.2 mm drill gives the 0.13 mm ring) and the net-class clearance must not be larger than the router's working clearance (0.127 mm), or DRC reports clearance errors the validator does not name.
- A mounting hole has a screw-head keepout of 1.5 mm round it: no track or via of any net may enter it; keep the holes clear of connectors' courtyards.
- A USB net is critical for the return-via rule; every layer change of a fast or unclassed net needs a ground via beside it. Declare every other net class `edge_rate slow`.
- A connector with fine-pitch pads (USB-C, 0.5 mm) and an SOIC or TSSOP with an exposed pad can fail the grid-escape stage although they are not ball grids: declare `escape { off }` on those components.
- Keep decoupling capacitors at their pins; keep the thermal vias of a pad inside the pad; do not overlap courtyards.

5. Procedure

1. Inspect the Synth tools, the language reference and the registry; list the support circuits Synth enforces and design them in.
2. Build in stages and inspect after each: power entry, rails, FPGA with its decoupling, configuration to the MCU, UART path, headers and LEDs. Validate after each stage.
3. Write the placement sidecar with hard hints for the edge connectors and the near-the-pin support parts. Run the placement review (synth_place_with_hints, run_routing=false) and fix what it names.
4. Export into a fresh folder export_<n>/ (synth_export, no allow_incomplete) and read the routing report and the blocking reasons. Fix causes with Synth tools: placement, parts, net classes. A full export of this board can take a long time; do not interrupt it.
5. Finish with a released export in export/ with the .synth file and the sidecar beside it.

6. Deliverables in your workspace

multirail_compute_node.synth, multirail_compute_node.placement.layout.toml, user-registry/ with every part you imported, the released export/, and REPORT.md containing: the design decisions and the power budget; the net classes and why each is slow or fast; the evidence of section 4 measured from the export; the placement measurements of the rules below (decoupling distances, connector gaps, test point distances, identical-circuit matches); every Synth defect or limitation you hit, described exactly; and what you could not do. Do not write a README.md.

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
