Irrigation Controller PCB — Close Every Review Blocker on the Released Board (Synth MCP)

1. State of the board

Your workspace holds the irrigation controller: irrigation_controller.synth, its placement sidecar, README.md (the exact description of the board) and user-registry/ (the parts the board uses). It exports released (45 of 45 nets, 0 KiCad DRC errors, no blocking finding) at 75.1 x 65.4 mm with 126 parts. A reviewer has asked for the findings below to be FIXED, not justified. The router is chaotic: a small layout change can break a net, put a via in a pad or leave a signal via without a ground-return via, so work in small steps, export each into a fresh folder, and keep a step only if it still releases.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results, part numbers, datasheet values or URLs. A number you are not certain of is written as "needs sourcing". The datasheet figures you need are given in section 3.

2. Hard rules

- Release exports only: never pass allow_incomplete=true. Export into a fresh folder each time; never edit exported KiCad files. A change that does not release is split, bisected and dropped; never carry a failing change forward.
- Every external connector stays within 0.3 mm of its board edge (pin headers may sit inboard); four mounting holes at the corners, clear of parts and copper; no decorative text; every part in a named schematic group (no OTHER region). Keep every part, net and function. Add no fill parts.
- The outline MAY grow if a bigger board is what makes a finding below pass; report the size. Do not call synth_compact_layout in this run.
- Rules by role, never by reference designator in what you write as a rule; use the board's names only when editing it.

3. Datasheet figures (verified from the manufacturers' PDFs; use exactly these)

- MP2307 (MPS, 3A 23V 340kHz): VIN absolute maximum -0.3 V to +26 V; recommended input 4.75 V to 23 V; VFB 0.925 V (0.900 to 0.950); thermal resistance SOIC8N 50 C/W (theta-JC 10).
- SMAJ13A (Littelfuse, 400 W DO-214AC): VR 13.0 V; VBR 14.40 to 15.90 V at 1 mA; IR 1 uA maximum at VR; VC 21.5 V at IPP 18.6 A. SMAJ14A: VR 14.0 V; VBR 15.60 to 17.20 V; VC 23.2 V at 17.2 A.
- AO3401A: 30 V drain-source, 12 V gate-source (as already recorded in the source notes).

4. The findings, in order

B1. Input protection chain. The supply range is stated as 10.8 to 13.2 V and the SMAJ13A (13 V standoff) sits AFTER the series MOSFET, so a transient reaches the MOSFET before it is clamped. Fix it: (a) change the stated supply range to 10.8 V to 13.0 V maximum everywhere it is written (the source header, every note), so the 13.0 V standoff is at or above the maximum input and the leakage at that input is the datasheet's 1 uA maximum; (b) move the TVS BEFORE the series MOSFET: its cathode joins the fused input net (the net on the MOSFET drain side), not the protected rail, and its anode stays on ground; (c) rewrite the TVS and MOSFET notes with the arithmetic from section 3: clamp 21.5 V is below the MP2307 recommended 23 V and absolute maximum 26 V and below the MOSFET's 30 V drain-source rating; worst-case Vgs is limited by the gate zener; say that a reversed supply forward-biases the TVS and opens the fuse; (d) recompute every note that used 13.2 V (the bleeder currents, the flow-pulse zener current (13.0 - 3.3) / 2.2 k, any other) at 13.0 V. Keep it released.
B2. Every decoupling capacitor at the supply pin it serves. Run the placement review (synth_place_with_hints, run_routing=false) and act on every support-part distance warning: set a hard near hint (near: part.pin) for each decoupling capacitor, the regulator output capacitors, the feedback divider and compensation parts, and move them in small groups, each re-routed and exported. The target is every per-pin decoupling capacitor within 4 mm of its pin, centre to pin. One capacitor (the second MCU supply decoupler) has been 11 mm away because every closer layout failed on ground-return vias; fix that by giving the router room: move the parts around it outward and let the outline grow as needed.
B3. Zero ground-return blockers. Every signal via needs a ground via within about 1.53 mm; the release gate reports "no ground return via". A layout step that adds any such blocker is a failing step. Work with the router's own findings: when it names a via and the nearest parts, move those parts apart.
B4. Spacing from through-hole pads. No SMD pad within 2.1 mm (edge to edge) of a through-hole pad where the layout allows: the bulk capacitor and its neighbour input capacitor are 1.9 mm apart, and the debug header is 2.05 mm from two test points. Move the SMD parts away.
B5. Silkscreen and vias. Run the placement review's silkscreen findings and clear what it can. Declare via_diameter 0.7 mm and via_drill 0.3 mm on every netclass (and a default if the language has one) so the exported vias are 0.7/0.3 mm (annular ring 0.2 mm); confirm from the exported file how many vias have each size, and if the router cannot keep the board released at 0.7/0.3 mm, keep 0.6/0.3 mm and say why.
B6. Values from datasheets. For each LED the series resistor value and the LED current at 3.3 V and at 13.0 V rails, for each bleeder resistor its current at its rail and the 100 uF bulk capacitor's discharge time, and the crystal load capacitors (8 pF crystal load, 5 pF stray assumption stated) are written in the notes as arithmetic; do not assume a number you cannot show.

5. Procedure

1. Read the .synth, the sidecar, README.md and this brief. Export the unchanged board into export_start/ and confirm it releases.
2. B1 first (source edit, then export). Then B2 to B5 in small steps, each into a fresh export_step_<n>/ folder, kept only if release_ready, route_complete and 0 DRC errors hold, every connector stays within 0.3 mm of its edge, and no new ground-return blocker appears.
3. Finish with the best released state exported into a fresh export/ folder with the .synth and sidecar beside it.
4. Write REPORT_FINAL.md: the finding-by-finding result (B1 to B6) with numbers measured from your own export (each decoupler's distance to its pin, count of ground-return blockers, via sizes, the SMD-to-through-hole distances, the outline), what you could not do and why. Do not call synth_compact_layout.

Begin by reading the files and exporting the unchanged board.
