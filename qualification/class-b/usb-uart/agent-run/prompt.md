# Prompt: USB-C dual-UART + RS-485 adapter

A self-contained prompt for an AI agent that can read and write files and run shell commands. It builds the board shown on this page with Synth. The board, its parts and the design decisions with their reasons are in the prompt. Synth's own documentation covers the exact command options and the `.synth` syntax.

This is a cleaned version of the brief we gave the agent: same board and decisions, with our local setup removed. It is not the exact text we ran, and a different run will not produce an identical board.

## You need

- Synth (the `synth` command, which compiles a `.synth` file to a KiCad project), with its part registry.
- KiCad 10 (`kicad-cli`) and FreeRouting 2.4.1 with Java, which Synth's export uses to route.
- An AI model that can use Synth through its command line or its MCP server, and read manufacturer datasheets from the web.

## The prompt

You are designing a real USB-C to dual-UART plus RS-485 adapter board with Synth, which compiles a `.synth` source file into a deterministic KiCad project. The board will be published, so every claim you make must be true and measured. Do not call the board clean, proven or manufacturable.

Rules: edit only the `.synth` source and its placement layout file, never the generated KiCad files. The placement layout is in board millimetres and the schematic layout is in sheet millimetres; never mix them. If a part is missing from the registry, author a Tier-2 part from its datasheet; never guess pin numbers.

### The board

One USB-C cable gives a computer two serial ports.

Intended use: a bench or lab adapter, with short cables, point-to-point or as the last node on an RS-485 bus. It is not for industrial installs, long cable runs or a node in the middle of a multidrop bus: A and B get no surge protection beyond the transceiver's built-in ESD, and the 120 Ω termination is always connected. Say so in the README.

- **Port 1: TTL serial** for debugging microcontroller boards. It comes out on two 3-pin headers: J2 = GND, TXD1, RXD1 (pin 1 is GND), and J3 = DTR1, RTS1, CTS1. Put 1 kΩ in series with RXD1 and with CTS1.
- **Port 2: RS-485** for industrial gear. A 3-pin header J4 = A, B, GND. Put a 120 Ω 1 % resistor across A and B. Its direction switches by itself, from the CH342F's DTR0/TNOW0 pin, with no software control. Put a 4.7 kΩ 1 % resistor from TNOW0 to GND to select the bridge's automatic half-duplex direction mode.
- **Power** comes from USB. A 3-pin jumper J_VIO picks the serial I/O voltage: pin 1 = +3V3, pin 2 = VIO, pin 3 = VBUS. Jumper 1-2 gives 3.3 V, jumper 2-3 gives 5 V. With no jumper, VIO is unpowered; state in the README what the bridge does then, from its datasheet. Think through what a forgotten jumper does to the rest of the board, and say it in the README.

### Parts

- WCH CH342F, dual USB-UART bridge, QFN-24 4 x 4 mm, 0.5 mm pitch, exposed pad to GND. Per its datasheet, its VIO pin sets the I/O voltage of both UARTs.
- TI THVD1450D, 3 to 5.5 V RS-485 transceiver. DI comes from TXD0, RO goes to RXD0, and DE and /RE are tied together and driven by the CH342F's DTR0/TNOW0 pin. Power it from VIO with 100 nF.
- Nexperia PRTR5V0U2X, ESD array on the USB D+/D- pair (VCC pin on VBUS).
- Nexperia PTVS6V0S1UR, 400 W TVS diodes on VBUS, CC1 and CC2, close to the connector.
- TI LP5907-3.3 LDO (SOT-23-5): 2.2 µF in and out, EN tied to VIN. It makes +3V3 for the jumper.
- USB-C receptacle `usb_c_receptacle` (HRO TYPE-C-31-M-12, a stock KiCad footprint), flush with the top board edge and centred along it. Connect all its VBUS pins to VBUS, and its ground and shield pins to GND. CC1 and CC2 each get 5.1 kΩ to GND so a USB-C host supplies 5 V. Both the A-side and B-side D+ and D- pins are tied together (the plug works either way round) and go straight to the bridge with no series resistor. Give them a netclass with 0.242 mm trace width and 0.127 mm clearance. Do not declare a 90 Ω impedance target: on two layers at 1.6 mm it is not reachable, and the pair will not be coupled by the router. Say so in the README.
- LEDs (Lite-On LTST-C190 series, 0603): green on VIO, red on TXD0 and TXD1, each with 470 Ω from VIO into the LED, with the LED's cathode on the TX line. The red LEDs sink their current into the TX pin, so they are dark while a line idles and lit on the low bits.
- Four M2.5 mounting holes H1 to H4, the registry part `mounting_hole_m2_5` as `mounting_hole` components, with hard placement hints: H1 and H2 in the top corners, H3 and H4 on the left and right edges at mid-height. Pin them in the placement layout at x 5.5 and 39.6 mm, y 3.0 mm (H1, H2) and y 21.0 mm (H3, H4). The bottom corners belong to the header row. Each hole must be clear of every other courtyard and keep at least 1.5 mm to Edge.Cuts; Synth adds a 2.2 mm board-feature rim around a hole when it fits the outline. Screw size and standoff height are not chosen.
- Headers J2, J3, J4 and J_VIO are 1 x 3, 2.54 mm pitch, through-hole.
- All resistors and capacitors are 0603. Capacitors are X7R, 16 V.
- 100 nF decoupling on the bridge's VDD5, V3 and VIO pins. Place each within 1 to 2 mm of its pin.
- Pull the CH342F's unused modem inputs (RI0, RI1, DCD0, DCD1, DSR0, DSR1) to VIO through 10 kΩ. Hold CTS0 low through 10 kΩ so UART0 flow control cannot stall. Leave RST open (internal pull-up).

Two decisions already made, with the reason:

1. **No 1.8 V or target-powered mode.** The THVD1450 needs at least 3 V, and it is powered from VIO. So when VIO is powered it is always 3.3 V or 5 V, chosen only by the jumper; with no jumper it is off.
2. **UART0 is not broken out as TTL.** It only drives the RS-485 transceiver. Do not add a TTL header for it.

Board: **2 layers**, manufacturer profile JLCPCB, no declared stackup (the exporter writes the 1.6 mm default). Put the USB-C on the top edge and the four headers (J2, J3, J4, J_VIO) on the bottom edge in one row. Compact placement: aim for roughly 41 x 32.3 mm.

### Tasks, in order

1. **Datasheets.** For each IC, protection part, LED and the connector (not the generic resistors, capacitors and headers), read the manufacturer's own datasheet (not a mirror) and record its revision. If you cannot reach the manufacturer's site, mark that part UNVERIFIED in the README.
2. **Write the design** as one `.synth` file (for example `usb_uart.synth`), with grouped sections (USB-C input, ESD, 3V3 LDO, bridge, VIO select, UART1 TTL, RS-485, LEDs) and a short note in each.
3. **Validate** with `synth validate`. Fix every error. List each remaining warning and what it means.
4. **Schematic.** Export it, render it, and look at the image. Fix power symbols that point the wrong way, overlapping labels, and parts outside their group box.
5. **Placement.** Synth's exporter runs its automatic placer when you export; start from that result, then tune `usb_uart.placement.layout.toml` (board millimetres). Measure each protection part and each decoupling capacitor against its target pad (pad-edge gap), the connector's distance to the board edge, and each mounting hole's distance to Edge.Cuts and to the nearest courtyard.
6. **Route** with `synth export-kicad <file> --out <dir> --router free-routing --krt-allow-via-in-pad`. The flag lets the router put the ground via inside the QFN's exposed pad, which a 0.5 mm pitch part has no room for otherwise. Synth keeps the routed board only if it passes independent validation. Then run `kicad-cli pcb drc --severity-all --refill-zones` on the delivered file. Target: every net connected, 0 DRC errors, 0 unconnected items. The router's result changes with every placement: if nets stay open, or the router leaves a net unrouted, shift the headers by a millimetre or two and route again. Do not accept a USB pair whose two lines differ by more than a few millimetres without saying so.
7. **Check what routing cannot give you.** Report the distance from each decoupling and protection pad to the nearest ground via, and the USB pair's track widths, lengths and skew (per connector contact), and whether it is length-matched or impedance-controlled. Report any via placed inside a pad, and what a fabricator would have to do (fill and cap it) to build it.
8. **README** next to the source: the concept; a bill of materials with part number, manufacturer datasheet URL and revision for every part; the VIO jumper table; the measured numbers (nets, tracks, vias, DRC result, route time); and what is NOT verified: not fabricated, not tested on hardware, any via-in-pad not confirmed with a fabricator, the USB pair not impedance-controlled, any UNVERIFIED part, any open warning. State that R_TERM is always in circuit and that there is no external TVS on A or B.

### Report back

The validate result (counts by code and what remains), the measured distances table, the DRC counts, the routing statistics, every decision you made on the open points and why, and every doubt or thing you could not verify.
