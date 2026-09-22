# RP2350 board agent workflow

This guide is a concrete starting point for an agent that creates and routes
an RP2350 development board through Synth. It is deliberately explicit about
placement and routing because a generic “make a board” prompt tends to leave
large unused areas, point connectors inward, and hand an external router a
poor partial maze.

## 1. Prepare the agent environment

From the repository root, build Synth and confirm the local tools are present:

```bash
cargo build -p synth-cli
command -v kicad-cli
test -f tools/freerouting/freerouting-2.4.1.jar
test -x tools/jre25/bin/java || command -v java
```

Start the MCP server over stdio for an MCP-capable agent:

```bash
cargo run -p synth-cli -- mcp --stdio
```

The agent should be allowed to search and extend the local Synth registry when
a requested part is missing. Missing registry entries are a repair task, not a
reason to stop the board run.

## 2. Prompt template

Use a prompt like this as the design brief:

> Create an RP2350A development board in Synth. Use the RP2350 datasheet and
> the SparkFun RP2350 development-board family as references. Include USB-C
> programming/debugging, QSPI flash, a WS2812B RGB LED, reset/boot controls,
> power input and regulation, crystal/clock circuitry, and two 1x20 expansion
> headers. Use JLCPCB-compatible footprints and sourcing fields.
>
> Before placement, validate every symbol pin, footprint pad, and net. If a
> part is missing, add a verified symbol/footprint entry to the Synth registry
> and continue. Place USB-C on a board edge with its receptacle opening
> outward, place the two expansion headers on opposite side edges with pin 1
> orientations documented and convenient MCU fanout, and keep the RP2350,
> flash, crystal, regulator, USB protection, and their decouplers in compact
> functional clusters. Use relative placement constraints and edge/rotation
> rules where possible; do not scatter components on a large canvas.
>
> Use a 4-layer JLCPCB-capable stackup with continuous GND coverage and use
> all available signal layers intentionally. Route from the clean placed
> netlist with FreeRouting rather than preserving a poor partial maze. Route
> power and clock/USB-critical connections first, then local decoupling and
> expansion I/O. Refill zones and run native KiCad DRC after routing.
>
> Fit the board outline to the actual component, connector, copper, and
> courtyard envelope. Do not leave a large empty lower margin. Keep at least
> 2 mm from the lowest mechanical/courtyard feature and routed copper to the
> bottom edge. Generate a rendered board image and inspect it; if USB faces
> inward, headers are poorly oriented, or the outline has unused space, fix
> placement and rerun before reporting success.

The agent should report the source `.synth` file, generated KiCad files, DRC
counts, board dimensions, and a preview image. “Autorouter finished” alone is
not a success criterion.

## 3. Export and route

Export the agent-created Synth source first:

```bash
cargo run -p synth-cli -- export-kicad path/to/rp2350_devboard.synth \
  --out build/rp2350
```

Then run the clean-netlist FreeRouting pipeline. The `--bottom` value is the
compact RP2350 example’s final bottom edge in millimetres; adjust it only after
checking the lowest footprint courtyard and copper bounds.

```bash
python3 tools/freeroute_clean_pipeline.py \
  build/rp2350/rp2350_devboard.synth.kicad_pcb \
  build/rp2350/rp2350_devboard_freerouted.kicad_pcb \
  --jar tools/freerouting/freerouting-2.4.1.jar \
  --java tools/jre25/bin/java \
  --passes 40 --threads 4 --bottom 73.0
```

The pipeline deliberately routes a clean duplicate of the board’s netlist,
imports the SES geometry through a text-safe converter, refills zones,
restores netclass constraints, and adds only the validated adjacent-header
bridge. It avoids the KiCad SES importer crash encountered on this board.

## 4. Review gates

Run native DRC and render the board before accepting the result:

```bash
kicad-cli pcb drc --output build/rp2350/pcb-drc.rpt \
  build/rp2350/rp2350_devboard_freerouted.kicad_pcb
kicad-cli pcb render --output build/rp2350/board.png \
  build/rp2350/rp2350_devboard_freerouted.kicad_pcb
```

Review these items visually and electrically:

- USB-C opening faces outward from the board edge.
- Headers are parallel, edge-accessible, and pin 1 is documented.
- Decouplers and clock/power parts are close to their IC pins.
- The outline has no large unused margin.
- There are no signal shorts, crossings, clearance errors, or width errors.
- Same-net ground-zone island reports are understood before fabrication.
- Silkscreen and footprint-library mismatch warnings are resolved or explicitly
  accepted by a human reviewer.

The repository includes a pre-generated compact review artifact at
[rp2350_devboard_freerouting_clean.kicad_pcb](../examples/rp2350/rp2350_devboard_freerouting_clean.kicad_pcb).
It is useful for inspecting the intended result, but it is not evidence that
the current `.synth` source reproduces successfully. The checked-in report is
not clean: KiCad currently reports 80 DRC violations and 18 unconnected
zone-island items. Always rerun export, FreeRouting, and DRC for a new design
revision.
It is a routing and review fixture, not a production approval; it still carries
ground-zone island and silkscreen/library warnings documented by its DRC run.
