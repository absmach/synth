Irrigation Controller PCB — Replace the Input P-MOSFET with a 30 V Part (Synth MCP)

1. State of the board

Your workspace holds the irrigation controller: irrigation_controller.synth, its placement sidecar, README.md and user-registry/. It exports with no blocking findings at 75.1 x 65.4 mm. Do not move any part except where this brief says; the sidecar holds hand-checked positions that make the board release.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results, part numbers or datasheet values.

2. The change

Q1 is the reverse-polarity P-channel MOSFET at the supply input, a Vishay Si2301 (registry part p_mosfet_si2301, 20 V drain-source). The input TVS clamps at 21.5 V at worst and sits after Q1, so a surge on the supply input can reach Q1 above its 20 V rating. Replace Q1 only (not Q_E1 or Q_E2) with the Alpha and Omega AO3401A: P-channel, 30 V drain-source, 12 V gate-source maximum, 4 A, SOT-23, pin 1 gate, pin 2 source, pin 3 drain (the same map as the Si2301).

Steps:
1. Read REFERENCE_p_mosfet_si2301.synth.toml in your workspace (a copy of the shipped Si2301 registry part; you cannot read the shipped registry directly) and author a new registry part p_mosfet_ao3401a from it (synth_author_part, user registry): same kicad_symbol, kicad_footprint and pin names and numbers (g, s, d), same footprint_dimensions; description "Alpha and Omega AO3401A P-channel MOSFET, 30 V, 4 A, SOT-23"; mpn "AO3401A"; operating_conditions max_voltage_v 30 and max_current_ma 4000; provenance datasheet_url "https://aosmd.com/sites/default/files/res/data_sheets/AO3401A.pdf". Do not write an lcsc_pn.
2. In the .synth change Q1's part to p_mosfet_ao3401a and nothing else. Check the gate network: the 5.1 V zener across source and gate is within the 12 V gate rating; write one line in the source notes saying so.
3. Export into a fresh folder with the sidecar unchanged (never allow_incomplete=true). If it does not release, report the blocking reasons and stop; do not move parts.
4. If it releases, export the final state into a fresh export/ folder, with the .synth and sidecar beside it.

3. Report (REPORT_Q1.md)

What changed, the registry part you authored, release_ready, route_complete, DRC result, outline size and anything you could not do.

Begin by reading the files.
