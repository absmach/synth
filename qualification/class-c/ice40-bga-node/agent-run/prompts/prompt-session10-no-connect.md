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
