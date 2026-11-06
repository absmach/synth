Irrigation Controller PCB — Fill the Voids with Sourced Parts (Synth MCP)

1. State of the board

Your workspace holds the irrigation controller after its review fixes: irrigation_controller.synth, its placement sidecar, README.md (what the board is) and user-registry/. It exports released (every net routed, 0 DRC errors, no blocking findings) at 75.1 x 65.4 mm with 108 parts, connectors within 0.3 mm of their edges, four mounting holes at the corners, and a courtyard fill of 29.7 %, below the 30.3 % it had before the review removed four parts. The sidecar holds hand-checked positions that make it release: do not move any existing part. The router is chaotic: a new part can break a net or put a via in a pad, so work in small batches.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results, part numbers or datasheet values.

2. Goal

Raise the courtyard fill to at least 31 % without growing the outline, by adding ordinary SMD parts in the free space, each wired to a real function. Do not re-add the SHT30 humidity sensor, the 5 V TVS diodes or any clamp across a 3.3 V rail. Do not call synth_compact_layout. Never pass allow_incomplete=true. Never edit exported KiCad files.

3. Parts you may use (so the bill of materials stays fully sourced)

Use only these, from the registry (registry first; synth_author_part only for a missing registry entry, with a real datasheet_url):
- Resistors 0603 (generic registry part r_generic_0603): 100, 680, 1 k, 2.2 k, 4.7 k, 10 k, 47 k, 100 k.
- Capacitors: 0603 10 nF, 100 nF; 0805 4.7 uF, 10 uF 25 V; 1206 22 uF 10 V (c_generic_0603, c_generic_0805, c_generic_1206).
- LEDs 0603: green (0603-GREEN part) and amber; Schottky diode d_schottky (SOD-123); the 3.3 V zener (d_zener_3v3) only behind a series resistor on a signal.
- Test points (testpoint_pad_1_5) on rails and signals.
Write the value exactly as above (for example "100nF", "10k") so the sourcing table matches.

4. What to add (each needs a stated function; none floating)

- A 100 nF 0603 decoupling capacitor within 4 mm of each IC supply pin that lacks one, and 100 nF across the 12 V valve rail and the 3.3 V rail near their bulk capacitors.
- Bleeder resistors (47 k or 100 k) across bulk capacitors.
- Rail-present and fault LEDs with 680 ohm series resistors, in one row, using rail and ground only.
- A flyback or clamp Schottky across each valve output if missing; a snubber (100 ohm in series with 10 nF) across each valve coil connector.
- Pull-ups or pull-downs on enable, reset and boot pins; test points on rails and connector pins, each beside its net.

The free space in the current export (areas keep 1 mm from every courtyard and the edge; outline 2.0,2.0 to 77.2,67.5): void 1 x 52.0-63.0 y 30.1-42.0 (132 mm2); void 2 x 10.6-16.1 y 3.0-20.6 (96); void 3 x 15.1-26.1 y 37.0-44.5 (82); void 4 x 72.5-76.5 y 42.0-59.5 (70); void 5 x 39.0-43.5 y 21.6-36.0 (65); void 6 x 40.0-44.5 y 3.0-16.1 (58); void 7 x 3.0-12.1 y 53.0-59.5 (58); void 8 x 59.0-68.5 y 24.1-30.1 (57); void 9 x 13.6-24.6 y 49.0-54.0 (55). Remeasure from your own export before using them.

5. Procedure

1. Read the files. Export the unchanged board into export_start/ and confirm it releases; measure the free space from your own export (free areas keep 1 mm from every courtyard and from the board edge) and use that.
2. Add parts in batches of 6 to 10 related parts, placed at coordinates in the free areas through the placement tools. After each batch run the placement review (synth_place_with_hints, run_routing=false), then export into a fresh export_batch_<n>/ folder. Keep a batch only if it releases and every connector is still within 0.3 mm of its edge and the outline is unchanged. If it does not release, split it in half, find the parts that break it, drop them and keep the rest. Never carry a failing batch forward. Try every kind of part above before you stop.
3. Finish with the best released state exported into a fresh export/ folder, with the .synth and sidecar beside it.

6. Report (REPORT_FILL.md)

A table of every part added: reference, value, function, nets, position, batch. The parts you dropped and why. The final fill, DRC, release_ready, route_complete, part count, outline and connector gaps, measured from the exported files. State plainly anything you could not do.

Begin by reading the files and exporting the unchanged board.
