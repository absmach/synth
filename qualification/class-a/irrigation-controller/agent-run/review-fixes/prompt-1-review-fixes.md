Irrigation Controller PCB — Fix the Review Findings (Synth MCP)

1. State of the board

Your workspace holds the finished irrigation controller: irrigation_controller.synth, its placement sidecar, README.md (the exact description of the board) and user-registry/ (the parts the board uses). It exports released (all nets routed, 0 DRC errors) at 75.1 x 65.4 mm with 112 parts, connectors flush with their edges (within 0.3 mm) and four mounting holes at the corners. A reviewer found the problems below. Fix every one in the .synth, the sidecar and the parts, re-route, and export. Do not justify a finding that can be fixed. Where a finding truly cannot be fixed with the tools you have, say so in REPORT_FIX.md with the measured reason, and fix everything else.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results, part numbers, datasheet values or URLs. A number you are not certain of is written as "needs sourcing", never guessed.

2. Hard rules

- The exported outline stays at or below 75.1 x 65.4 mm; every external connector stays within 0.3 mm of its edge; the four mounting holes stay at the corners and clear of parts and copper. Keep the board's functions: removing a part is allowed only where a finding below asks for it.
- Release exports only: never pass allow_incomplete=true. Export into a fresh folder each time. A change that does not release is split, bisected and dropped; never carry a failing change forward. Never edit exported KiCad files.
- Add or change parts through the Synth tools (registry first, synth_author_part when a part is missing, with a real datasheet_url under provenance). Never call synth_compact_layout.
- Placement by role, never by reference designator in what you write down as a rule; use the board's names only when you edit it.

3. The findings, in order

F1. Protection ratings.
 (a) A 5 V TVS (d_tvs_5v) sits on the flow-pulse signal that feeds an STM32G030 pin, and another 5 V TVS across the 3.3 V rail. A 5.1 V clamp is above the STM32G030 input maximum, and a TVS across 3.3 V does not protect the rail. Find every d_tvs_5v and every other clamp in the source and check it against the voltage of the net it sits on and the absolute maximum of what hangs on that net. Replace a clamp on a 3.3 V logic signal with a 3.3 V zener (registry d_zener_3v3 or bzt52c3v3), cathode on the signal and anode on ground, behind the series resistor the signal already has, and write the current calculation (source voltage, series resistor, zener current against the zener's rating) into the source notes and REPORT_FIX.md. Remove a clamp that is across a regulated 3.3 V rail; do not replace it with another device across the rail. A 5 V clamp stays only on a 5 V net.
F2. Input TVS against the regulator.
 The input TVS is an SMAJ15A with a 24.4 V maximum clamp, on the same protected rail as the buck regulator whose maximum input is 23 V. Replace it with an SMAJ13A (13 V standoff, 14.4 V to 15.9 V breakdown, 21.5 V maximum clamp at its rated peak current), authored as a registry part with its Littelfuse datasheet URL and the SMA package the board already uses. Check that the 12 V supply stays below 13 V standoff in normal use and state the margin. The worst-case clamp must be below 23 V; write that comparison into REPORT_FIX.md.
F3. The humidity sensor that cannot be escaped.
 The SHT30 is a 0.5 mm pitch DFN that the two-layer standard-tier router cannot escape. Remove it and the parts that serve only it (its decoupling capacitor), keep the I2C pull-ups and the external I2C connector, and say in the notes that an SHT30 breakout attaches to that connector at address 0x44, which stays free. Update every README sentence and source note that mentions it.
F4. Crystal load.
 The crystal is the NDK NX3225GD-8MHZ-STD-CRA-3 (8 pF load, registry part xtal_8mhz_3225_4p). The two load capacitors were sized for a 12 pF crystal (15 pF each). Recompute with CL = (C1 x C2)/(C1 + C2) + Cstray, using Cstray = 5 pF as the board already assumes: C1 = C2 = 2 x (8 - 5) = 6 pF; use the nearest standard value that gives CL within 0.5 pF of 8 pF, show the arithmetic in the notes, and replace both capacitors. Keep them at the clock pins.
F5. Valve current.
 The valve coil current is an unwritten assumption. State it: each valve output is a 12 V DC solenoid with a coil current of at most 0.5 A. Write that into the source (a note on each valve driver and a current attribute on the valve nets if the language supports it), check that the MOSFET, the flyback diode and the tracks of the valve netclass carry 0.5 A with margin (track width for the temperature rise, diode forward current rating, MOSFET drain current), apply the netclass to the valve power and switched nets, and report the check. Fix anything that does not carry it.
F6. Distances the review lists.
 Run the placement review (synth_place_with_hints, run_routing=false) and act on every advice action it gives, in small groups, each re-routed and exported and kept only if it releases: the debug header is 43.8 mm from the processor (bring it within 15 mm with a hard near hint; a pin header may sit inboard); the reset button is 16.7 mm from the reset pin (bring it within 8 mm); decoupling capacitors far from the supply pin they serve (near: part.pin hints); test points far from their circuit; identical circuits laid out differently (copy the first copy's offsets); like parts that miss their row; small parts stranded from everything they are wired to. Report the distances measured from the exported board before and after.
F7. Return paths.
 The board review lists signal vias without a ground return via nearby on a two-layer board carrying valve switching, probe inputs and I2C. Run the review, and for each remaining advisory add a ground via next to the signal via (through the Synth tools or by a hint that makes the router place it) or move the layer change. Report how many advisories remain and why each one does.
F8. Human-readable schematic and silkscreen.
 The schematic has an OTHER region for parts that belong to no group, and crowded reference designators on the PCB. Put every part into a named functional group in the source (power input, regulator, processor, valve drivers, sensor interfaces, indicators, test points and so on) so OTHER is empty, export, render the schematic with kicad-cli sch export pdf and look at it; then review the silkscreen findings of the export and fix them (placement of references; no decorative text).
F9. Orderable bill of materials.
 For every part in the board, the BOM must show manufacturer, MPN, package and rating. Fill what the registry parts carry, and for generic passives choose a specific orderable part by editing the registry part (for example a named 100 nF X7R 0603 capacitor from a named manufacturer). Only write an MPN or an LCSC number you are certain of; otherwise write "needs sourcing" and list it in REPORT_FIX.md. Do not fill the BOM with guesses.

4. Procedure

1. Read the .synth, sidecar, README.md and this brief. Export the unchanged board into export_start/ and confirm it releases.
2. Work through F1 to F5 first (they change parts), exporting after each finding into export_F<n>/ and keeping it only if it releases. Then F8, then F6 and F7 (they move parts), then F9.
3. Finish with the best released state exported into a fresh export/ folder with the .synth and sidecar beside it. Run kicad-cli pcb drc on the export and report every warning.

5. Report (REPORT_FIX.md)

One section per finding: what you changed (parts, values, positions), the check or calculation, and the measured result from the exported files. Then: final DRC, release_ready, route_complete, outline size, connector gaps, part count, the list of "needs sourcing" parts, and anything you could not do with the measured reason.

Begin by reading the files and exporting the unchanged board.
