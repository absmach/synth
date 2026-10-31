Irrigation Controller PCB — Fill the Empty Space with Rail-Only Parts, in Small Batches (Synth MCP)

1. State of the board (README.md describes this board exactly; coordinates are board millimetres, the same as the sidecar; the part names C_N*, R_N*, D_N*, TP_N* exist from the previous run)

Your workspace holds the irrigation controller after a previous run added 26 parts (README.md and REPORT_previous_run.md describe it). It exports released (43 of 43 nets, 0 DRC errors) but it is NOT yet acceptable: (1) its outline is 75.1 x 65.9 mm, 0.5 mm above the cap of 75.1 x 65.4 mm, because one added passive stands inside the rim of a connector edge and holds the outline out, leaving the three connectors on that edge 0.68 mm inside the board edge instead of 0.15 mm; (2) many added capacitors are 15 to 57 mm from the part they decouple, and a TVS added for a connector's signal is about 45 mm from that connector; (3) the grouping and alignment moves were only partly done. Your job, in this order: first repair the outline and connector gaps, then finish the grouping and alignment, then re-place the added parts where they serve a nearby part, then add more parts only if the board stays released and the outline stays at or below the cap. The Synth build you have also moves a via the engine left in a pad off it and routes a rejected board again in the engine's other net orders.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results.

2. Hard rules (unchanged)

- Exported outline stays at or below 75.1 x 65.4 mm, and every flush connector stays within 0.3 mm of its edge (check both on your own export, not only on the tool's report). Keep every existing part, net and function; existing parts may move only as step A below allows (grouping and alignment inside the outline); connectors stay on their edges exactly as they are. Connectors stay flush with the edge; the four mounting holes stay at the corners.
- Every added part is wired to a real function; no floating or decorative parts, no text.
- Add parts through the Synth tools (edit the .synth, use the registry before authoring, place with the placement tools at coordinates in the empty areas). Never edit exported KiCad files.
- Release exports only: never pass allow_incomplete=true. A batch that does not release is not carried forward.
- Prefer SMD parts with ordinary courtyards: 0402, 0603 and 0805 passives, SOD-123 and SOD-323 diodes and TVS, SOT-23 transistors and MOSFETs, SOT-23-6 and SOIC-8 ICs. Avoid radial electrolytics and TO-92 (their circular courtyard is measured badly).

2b. Step A before adding anything: group and align the existing parts, then fill (new in this run)

Do the regrouping first, in small steps, each re-routed and exported (fresh export_step_<n>/ folder) and kept only if it releases (release_ready, route_complete, 0 DRC errors, every connector still within 0.3 mm of its edge, outline unchanged at 75.1 x 65.4 mm). Move parts by pinning coordinates and hints in the sidecar with the Synth placement tools; never edit exported files. Rules by role, never by reference designator:
- Power section: the regulator's inductor, input and output capacitors, bootstrap, soft-start, compensation and feedback-divider parts form one compact block at the regulator; the feedback divider sits right at its pin; the fuse is in the input path; protection parts sit at the input connector; input test points stay accessible.
- Processor: it is the visual centre. Each decoupling capacitor sits at its supply pin, the crystal and its load capacitors at the clock pins, and the reset button, its pull-up and its capacitor in one group by the reset pin. Keep the immediate surroundings uncluttered.
- Identical circuits (same part ids, same connectivity pattern: the valve drivers, the sensor interfaces) get the same relative placement and orientation. Copy the first copy's offsets to the others; a driver circuit sits beside its connector (switch, gate resistor, pull-down, flyback diode).
- Each sensor or indicator circuit sits in one compact group by its connector or its IC; like parts (resistors, capacitors, LEDs, diodes) share rows or columns and orientations; indicator LEDs with their resistors form one row; no isolated passive more than 4 mm from a neighbour that shares its net.
- Keep sensitive analog signals apart from switching and valve-drive nets; keep the debug header and test points reachable; keep the four mounting holes clear of parts and copper.
- Do not rotate parts only for symmetry; do not reduce clearances; do not add text or headings.
The placement review (synth_place_with_hints, run_routing=false) now carries five advice actions: `move_off_edge_rim` (a part within the rim of a connector edge holds the outline out: move it inward by the amount given; do this first), and four more you should act on in step A, each with the coordinates or part to use: `match_repeated_circuit` (a part of an identical circuit that is laid out differently from the first copy: pin it at the given coordinates and turn), `align_like_parts` (a like part that misses the row or column its neighbours share: set the named coordinate), `place_beside_neighbour` (a small part far from everything it is wired to: put it beside the named part) and the support-part and test-point advice (a hard `near: part.pin` hint). Apply them in small groups, re-review, export, and keep a group only if the board still releases.
After each step run the placement review and clear its warnings (support-part distance, identical-circuit mismatch, connector edge). Then continue with the fill below in the space the regrouping leaves. At the end report which of these rules each part group meets and which it does not, with distances measured from the exported board.

3. What changed since the earlier runs

The earlier runs failed on a via placed inside a pad (U2.30, U3.9, J_I2C.1, U5.7): the Synth build they used placed ground return vias on ground pads. That is fixed in the build you have now, which also routes a rejected board again in the engine's other net orders. So the earlier failures should not recur, and batches that failed before can be tried again. The MCU is U2 in this design; U5 is the EEPROM.

Still prefer simple connections: parts on +12V_VALVE, +3V3, ground, connector pins, and each other, placed right next to the rail or pin they use. The +3V3 net near U2 is tight (it was routed down to 0.127 mm tracks), so add +3V3 parts last and in small groups. Parts the earlier runs added (C_D1 to C_D6, C_E1 to C_E8, R_BL3, R_BL12, R_E1 to R_E3 and others) are already there; the sidecar also still holds entries for parts that are no longer in the .synth, which you may remove.

4. The empty space (from the current export; free areas keep 1 mm from every courtyard and from the board edge)

outline 2.0,1.6 to 78.2,69.5  (75.1 x 65.4 mm); free areas keep 1.0 mm from every courtyard and 1.0 mm from the edge
void 1: x 63.0 to 76.5, y 32.5 to 56.5  (13.5 x 24.0 mm = 324 mm2)
void 2: x 8.1 to 24.6, y 44.5 to 54.0  (16.5 x 9.5 mm = 157 mm2)
void 3: x 48.0 to 69.0, y 60.5 to 66.5  (21.0 x 6.0 mm = 126 mm2)
3 voids, 607 mm2 free at >= 4 mm on a side

Keep about 3 mm between neighbouring courtyards and leave channels for tracks. Parts 2 to 4 mm apart is the goal. Coordinates are board millimetres, the same as the sidecar.

5. What to add (rail-only and connector-only parts first; each one needs a stated function)

- Decoupling: 100 nF 0603 and 1 uF or 10 uF 0805 capacitors along the +3V3 rail, the +12V_VALVE rail and the power input, each next to a rail track or an IC supply pin, grouped in the voids nearest those tracks.
- Bleeder or discharge resistors (10 k to 100 k) across the bulk capacitors on +12V_VALVE and +3V3.
- Rail-present LEDs with series resistors in one row: +12V_VALVE, +3V3, and each valve output's rail (use rail and ground only).
- Protection on external connector pins: a TVS or ESD diode (SOD-323 or SOD-123, 5 V for signal pins, 15 V to 24 V for the 12 V side) from each unprotected connector pin to ground, next to the connector. A flyback or clamp diode across each valve output if missing.
- Reverse-polarity or surge helpers on the power input: a second TVS, a varistor footprint with a part that exists in the registry, a ferrite bead in series with the 3V3 sensor supply if a clean split exists.
- More test points on rails and on connector pins, each next to its net.
- A snubber (resistor plus capacitor in series) across each valve coil's connector pins.

6. Procedure (small batches, bisect on failure)

1. Read the .synth, sidecar, REPORT.md and this brief. Export the unchanged board into export_start/ and confirm it releases.
2. Add parts in batches of 6 to 10 related parts. After each batch: run the placement review (synth_place_with_hints, run_routing=false) and clear the warnings for the new parts; then export into a fresh export_batch_<n>/ folder.
3. If a batch releases, keep it and continue. If it does not release, split it in half, find which parts break the release, drop those and keep the rest. Never carry a failing batch forward.
4. Do not add parts until the outline and connector gaps are repaired. After that, add parts only as decoupling next to the pin they serve or protection next to the connector they guard; no part counts that is more than 6 mm from the part it serves. Do not stop after one or two failed batches: split, drop the culprits and keep going until you have tried every kind of part listed above.
5. Finish with the best released state exported into a fresh export/ folder, with the .synth and sidecar beside it. Do not call synth_compact_layout. The empty-space list in section 4 is measured BEFORE step A; remeasure the free space from your own export after step A and use that.

7. Report (REPORT6.md)

A table of every part added in this run: reference, value, MPN or "generic", function, nets, position, batch. The parts you dropped and why. The final DRC result, release_ready, route_complete, number of nets, and the placement review result. State plainly anything you could not do or check.

Begin by reading the files and exporting the unchanged board.
