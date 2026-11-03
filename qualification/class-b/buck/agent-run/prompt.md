# Prompt: 24 V to 3.3 V buck converter

A self-contained prompt for an AI agent that can read and write files and run shell commands. It builds the board shown on this page with Synth. The board, its parts and the design decisions with their reasons are in the prompt. Synth's own documentation covers the exact command options and the `.synth` syntax.

This is a cleaned version of the brief we gave the agent: same board and decisions, with our local setup removed. It is not the exact text we ran, and a different run will not produce an identical board.

## You need

- Synth (the `synth` command, which compiles a `.synth` file to a KiCad project), with its part registry.
- KiCad 10 (`kicad-cli`) and FreeRouting 2.4.1 with Java, which Synth's export uses to route.
- An AI model that can use Synth through its command line or its MCP server, and read manufacturer datasheets from the web.

## The prompt

You are designing a real 24 V to 3.3 V step-down converter board with Synth, which compiles a `.synth` source file into a deterministic KiCad project. The board will be published, so every claim you make must be true and measured. Do not call the board clean, proven or manufacturable.

Rules: edit only the `.synth` source and its placement layout file, never the generated KiCad files. The placement layout is in board millimetres and the schematic layout is in sheet millimetres; never mix them. If a part is missing from the registry, author a Tier-2 part from its datasheet; never guess pin numbers.

### The board

24 V nominal input, up to 26.4 V (+10%), and 3.3 V at 1 A output. Use four copper layers with GND pours on all of them and a hand-pinned placement. Say in the README what the board is not: not surge-tested, not thermally characterised, not fabricated.

Input chain: J1 (JST XH, 2-pin: pin 1 +24 V, pin 2 GND) -> F1 resettable PTC (Ihold 1.1 A, Vmax 33 V) -> D1 SS36 Schottky in series (reverse-polarity block) -> VIN node.

VIN node: D2 SMBJ30A TVS (30 V standoff, 48.4 V clamp at 12.4 A), C1 47 uF 50 V electrolytic, C2 10 uF 50 V X7R 1206, C3 220 nF 50 V X7R 0603 at the regulator's VIN pin, U1 VIN, and the top of the UVLO divider.

Regulator: TI LMR38010SDDAR, 4.2 to 80 V operating, 85 V absolute maximum, 1 A, internal compensation, exposed pad (pin 9) on GND, open-drain PG. R_RT 33k from RT/SYNC to GND sets 800 kHz; the pin must not float. C4 100 nF between BOOT and SW.

UVLO: R_UV_T 1.37M from VIN to EN, R_UV_B 100k from EN to GND. Nominal start 18.4 V, stop 16.2 V from the datasheet's typical thresholds; state the min/max spread and say the extremes are not a guaranteed hysteresis.

Output: L1 6.8 uH (Bourns SRN6045TA) from SW to the output node, then C5, C6 and C9, three 22 uF 25 V X7R 1206 capacitors. Feedback R_FB_T 100k from the output node (the far side of L1, never SW) to FB, R_FB_B 43.2k from FB to GND, C7 56 pF across R_FB_T. Vout is 3.315 V typical.

Power-good: R_PG 100k from PG to +3V3, and the PG net goes to J2 pin 3. D4 is a green LED with R_PGLED 470 ohm from +3V3 to PG, so it is lit while PG is low (start-up or fault) and dark while power is good. D3 is a green power LED with R_LED 470 ohm from +3V3.

Connector: J2 is a JST XH 4-pin: pin 1 +3V3, pin 2 GND, pin 3 power-good, pin 4 GND. Put J1 on the left edge and J2 on the right edge with hard placement hints.

Mechanical: H1 and H2 are M2.5 non-plated mounting holes in opposite corners.

Placement: pin every part in the sidecar. Put the input chain below U1 so VIN does not cross the EN divider, C3 and C2 close to the VIN pad, C5, C6 and C9 in a short column beside L1's output pad, and the feedback parts close to FB. Add six 0.3 mm GND vias in U1's exposed pad as sidecar `[[vias]]`, and declare `requires via_in_pad` with manufacturer `jlc-filled-via`.

### Checks

- `synth validate` must pass with no error. Warnings are acceptable only if you list them and their reason.
- Build a negative twin with R_FB_T and C7 taken from SW instead of the output node. It must fail validation with E-SYNTH-POWER-011. Publish it and say what it proves.
- Export with FreeRouting and run `kicad-cli pcb drc --severity-all --refill-zones` on the delivered board: target 0 errors, 0 warnings, 0 unconnected items.
- Run `kicad-cli sch erc --severity-all`: target 0 violations.
- Check that every net is connected, the GND pour is stored filled, and the via-in-pad list is exactly the six thermal vias.

### What to write down

Report the measured size, layer count, net count, segment and via count, copper length, DRC and ERC results, and the router and its version. List what is not verified: no expert signal-integrity, power-integrity or RF review; no thermal measurement or junction-temperature estimate; no loop-stability or load-transient result; no input or output ripple measurement; no surge test and no IEC 61000-4-5 claim; no DC-bias data for the ceramics; no LCSC numbers or MPN for the generic passives; and no fabrication. State that the six filled vias in the pad are not confirmed with a fab. Do not present Synth's release gate as a manufacturing sign-off.
