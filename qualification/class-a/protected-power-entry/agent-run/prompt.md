Protected Power Hub PCB — Complete Electrical and Physical Design Specification

1. Objective and execution environment

Design a complete, production-oriented two-layer Protected Power Hub: a 12 V protected power-entry and distribution PCB with regulated 5 V and 3.3 V outputs using Synth MCP tools and export the finished design to KiCad.

Board name: protected_power_entry

Native Synth project: protected_power_entry.synth

The board must be a real, electrically connected, routed PCB design, not just a schematic or conceptual layout.

Use the available Synth MCP tools to create the project, place real components, establish nets, generate the PCB, position components, route copper, run validation, and export the results to KiCad.

The only available agent tools are Synth MCP and Read, Write, and Edit. Do not assume shell access, web access, or external tool access. Discover the available Synth operations from their actual tool descriptions and server instructions.

Do not invent Synth commands, component library entries, successful tool operations, or validation results.

If a required operation is unavailable, complete the remaining supported work and document the limitation.

This board is a qualification board: it will be reviewed by an independent PCB reviewer, and the way it was made is part of what is reviewed. Work only through the Synth tools. Where Synth does something wrong, report it exactly rather than working around it silently.

2. Required system functionality

The board is a protected power entry that is reviewed as a power stage, and it is the complete chain from a protected input to usable outputs. It must include:

A 12 V DC input through a connector rated for the current: a screw terminal or a JST connector, which you choose and justify.
Reverse-polarity protection using a P-channel MOSFET.
Input over-current protection using a fuse that you choose from the load budget and justify (resettable or replaceable), placed where it can be reached.
Transient suppression using a TVS, and input filtering.
An MP2307 buck regulator that produces a 5 V rail.
A TPS22918 load switch with soft start that produces a switched 5 V output.
A 3.3 V rail for logic that a host board would add.
Separate protected output connectors for the 5 V, the switched 5 V and the 3.3 V.
A control header that a host can use: a hardware enable for the switched output, a fault-status output if the design has a real source for one, and scaled samples of the rails for a host ADC.
Monitoring: test points on the raw input, the fused input, the protected 12 V rail, every output and ground; a power LED for each rail; a fault indicator. Add input current sensing only if you can do it correctly.
Clearly labelled, physically accessible external connectors.
A fully routed two-layer PCB suitable for JLCPCB manufacturing.
A complete schematic, BOM, PCB layout, and verification report.

Do not silently omit any requirement.

This board has no microcontroller, no USB-C programming path and no SWD header. That is a stated exception to the Class A rule that every board has them: the board is a power stage, and nothing on it is programmable. The control header satisfies the GPIO requirement and the indicators satisfy the indicator requirement. Do not add a microcontroller.

The order of the protection chain is the claim being reviewed. A TVS placed in front of the fuse has nothing to limit the fault current it conducts, so the transient suppressor must sit after the fuse. It must also not conduct in the reverse-polarity case: a unidirectional TVS across a supply conducts like a diode when the supply is reversed. Place each protection element so that normal operation, a reversed supply, an over-current fault and a transient each meet the right part, and justify the position of every one.

3. First inspect Synth and establish the design

Before creating the circuit:

Inspect the available Synth MCP tools and determine their actual capabilities.
Determine how the environment represents projects, components, footprints, nets, net classes, schematics, PCB placement, stackup, routing, validation, and KiCad export.
Create or open the project using the exact name given above.
Inspect available component libraries and verify the parts required for the design. Prefer registry parts that already carry a manufacturer part number and an LCSC number; import or author a part only when none fits, and say so.
Establish a component and signal plan before placing components.
Implement the design incrementally, inspecting the result after each major subsystem.

Use the following implementation order:

Load assumptions and the supply budget.
Input connector, fuse, reverse-polarity protection and transient suppression.
The 5 V buck regulator.
The load switch and the 3.3 V regulator.
The control header, monitoring and fault indication.
Output connectors, indicators and test points.
Schematic connectivity verification.
PCB placement and routing.
ERC, DRC, and unresolved-issue review.
KiCad export and final artifact verification.

Do not route the PCB before the essential schematic connectivity and pin assignments are established.

4. Load assumptions

Before selecting any rating, document the assumptions the design rests on: a nominal 12 V supply with a provisional continuous range of 9 to 16 V, the maximum load on the 5 V output, the maximum load on the switched 5 V output, the maximum load on the 3.3 V output, the transient environment you protect against, and whether the connectors are hot-plugged.

Start from these provisional targets: 5 V (including the switched output) up to 1.5 A and 3.3 V up to 0.5 A. They are goals, not ratings. If the 3.3 V rail is made from the 5 V rail by a linear regulator, its load is part of the 5 V budget and its dissipation is (5 V minus 3.3 V) times its current; show both. The MP2307 is a 3 A part, so the sum of everything it supplies, with margin, must stay below that. If you can show by calculation that more is supportable, or that less is all the parts allow, say so and use the number you can defend.

Do not claim a maximum output current that the thermal and ampacity calculations, the connector ratings and the fuse do not support. Report any unknown as an unresolved product specification.

5. Input connector and protection chain

Input connector J1

Use a mechanically suitable, keyed JST connector with a verified footprint and a documented mating connector. Define the pinout explicitly: pin 1 the 12 V input, pin 2 ground. Verify the actual pin numbering against the footprint. Clearly mark polarity on the PCB.

Protection and power path

Implement a chain in which the fuse comes first and the transient suppressor after it:

J1 → F1 1.5 A polyfuse → reverse-polarity P-channel MOSFET → protected 12 V rail

with the SMAJ15A placed so that it does not conduct when the input is reversed, and so that a transient meets the fuse's impedance.

Reverse-polarity MOSFET

Use a Si2301-class P-channel MOSFET in the usual orientation for reverse-polarity protection. Verify its pin numbering, its drain-source voltage rating, its on-resistance at the gate voltage you will actually have, and above all its absolute-maximum gate-source voltage.

The gate must be held so that the gate-source voltage stays inside that absolute maximum over the whole input range, including the top of the tolerance and during a transient. A gate resistor to ground alone will not do this at 12 V on a part rated for far less. Do not copy a gate-clamp value from anywhere without checking it against the part's own limit; choose the clamp from the datasheet, put it across the gate and source, and show by calculation that it works for both a normal and a reversed supply.

Transient suppressor

Verify the SMAJ15A's standoff voltage, breakdown voltage and clamping voltage. The standoff must be above the highest normal input, and the clamping voltage at the rated surge current must be compared with the absolute-maximum input voltage of everything downstream, which includes the buck regulator and the MOSFET. If the clamp exceeds a downstream rating, say so in the report; do not claim the chain is safe.

Input capacitors

Provide input capacitors rated for the voltage and ripple, placed where the buck regulator needs them.

Keep the raw input, the fused input, the protected 12 V rail, the 5 V rail and the 3.3 V rail as distinct named nets.

6. 5 V buck regulator

Use an MP2307 buck regulator.

Implement every external component it needs: input capacitors, the inductor, the output capacitors, the feedback resistors, the bootstrap capacitor, the compensation network, the soft-start capacitor and the enable circuit, as its datasheet specifies. The part has an exposed pad that its datasheet requires to be tied to ground; connect it, and give it the copper to dissipate heat.

Calculate the feedback divider from the part's actual reference voltage for a 5 V output. Verify the inductor's value, saturation current and rating against the ripple current at the worst-case input voltage. Verify the output capacitance and ripple, the switching frequency and the startup behaviour. Verify that the catch-diode requirement, if the part has one, is met.

Keep the switch node's copper small and away from sensitive nodes, and route it: the switch node is the net a previous board of this kind left unrouted, and it must be a routed net here.

Name the output +5V. Provide test points for the 5 V rail and for the switch node.

7. Load switch and 3.3 V regulator

Load switch

Use a TPS22918 load switch with soft start on the 5 V output, producing +5V_SW. Verify its pinout, its on-resistance, its input and output capacitor requirements, and how its slew-rate or soft-start capacitor sets the turn-on time. Calculate the inrush current into the maximum load capacitance and show that it is acceptable.

Define its enable input explicitly: what level turns it on, what the default state is with nothing connected, and how a carrier board or a test point can turn it off. Provide a pull to the default state and a test point or connector pin for control.

3.3 V regulator

Use an AMS1117 regulator to produce +3V3 from the 5 V rail, for logic a carrier board would add. Verify its pinout (including that its tab is the output), its input and output capacitor requirements, its dropout voltage and its thermal limits. Calculate its dissipation at the worst-case input and load, and the temperature rise on the copper you give it.

Calculate the total load on each rail and show the margin against each regulator's rating.

Provide bulk output capacitance on the 5 V and 3.3 V rails sized for a load step, check it against the regulators' stability requirements, and say what happens at power-up and power-down: which rail comes up first, whether the 3.3 V rail can be present before the 5 V rail has settled, and what the load switch does while the buck is starting.

8. Control header, monitoring and fault indication

Control header

Provide a labelled header for a host board. It carries: the enable input of the switched output, a fault-status output if there is a real source for one, scaled and clamped samples of the protected 12 V rail, the 5 V rail and the 3.3 V rail for a host ADC, a 3.3 V reference pin and ground. Label every pin and state its voltage range and what drives it. Every pin must be 3.3 V compatible and protected as the GPIO block below requires.

Enable

Define what level turns the switched output on, what the default is with nothing connected, and what the host can do. The load switch's enable pin has a voltage limit of its own; check it, and do not let a header pin present more than it allows.

Fault status

Do not invent a fault signal. Establish from the datasheets whether any part on the board produces one: a power-good or fault output of the buck, of the load switch, or a comparator you add across the fuse or the protected rail. If one exists, bring it to the header with the pull-up it needs and say what it means. If none does, say so, and leave the line off the header rather than wire it to nothing.

Current sensing

Add input current sensing only if you can do it correctly: a shunt in the protected path, its value and dissipation at the fuse's hold current, an amplifier or a filtered sense node, and the range at the header. If you do not add it, say so in the report.

Indicators

Provide a power LED for each of the input, the protected 12 V rail, the 5 V rail, the 3.3 V rail and the switched output, and a fault indicator if there is a fault signal, each with a resistor you calculate.

9. Output connectors and test points

Provide output connectors for the switched 5 V and the 3.3 V. Define each pinout explicitly, with ground and the supply, verify the numbering against the footprint, and label them. State the maximum current each connector and its mating part are rated for, and compare it with what the board can deliver.

Provide test points for: the raw input, the fused input, the protected 12 V rail, the 5 V rail, the switched 5 V, the 3.3 V rail, the buck switch node, the load switch's enable, and ground (at least two). Place them where they can be reached with a probe or a multimeter without touching a neighbouring high-current pad.

10. Grounding and power distribution

Use a common ground with deliberate current-return routing. Keep the buck regulator's switching current loop (input capacitor, regulator, inductor, output capacitor) short and compact, and keep its return currents out of the connectors' ground paths and out of the sample nodes of the control header. Use as continuous a bottom-layer ground pour as you can.

Say what copper area you give the regulator's exposed pad and the 3.3 V regulator's tab, and what temperature rise that implies at the full load.

The ground connection between every part of the board must be explicit and electrically continuous.

Do not create arbitrary isolated ground nets that leave the circuit electrically disconnected.

11. Physical board organization

Use a deliberate placement strategy.

Region A: Input and protection

Place the input connector at one board edge, with the fuse, the reverse-polarity MOSFET and the TVS immediately inside it, so that the surge path is short, and the fuse where it can be reached.

Region B: Converters

Place the buck regulator, its inductor and its capacitors close together with the switching loop compact, then the load switch and the 3.3 V regulator after it. Keep the feedback nodes away from the switch node.

Region C: Outputs and control

Place the output connectors at the opposite edge, near their power distribution, and the control header at an accessible edge, away from the switching nodes.

Placement priorities, in order: connector accessibility and mechanical fit; the order of the protection chain; the buck's switching loop; thermal copper for the regulators; the control header's signal integrity; test-point accessibility; silkscreen readability; visual neatness.

Do not sacrifice electrical performance merely to achieve a symmetrical layout.

12. PCB stack-up, copper, and routing

Use a two-layer PCB suitable for JLCPCB.

Prefer: top layer for components and most routing; bottom layer a largely continuous ground pour, with unavoidable signal routing only where necessary.

Do not split a ground plane into arbitrary islands. A ground pour that is cut into pieces by tracks leaves islands with no connection; check the result.

Use appropriately sized copper for every power and high-current path. Calculate trace widths from current, copper thickness, acceptable temperature rise, and the expected load.

Declare a net class for every group of nets that needs its own trace width, clearance or via size. Assign the class to each net explicitly, by name, in the source. After the export, read the routed `.kicad_pcb` and confirm that each power net was actually routed at its declared width; a class that was declared but not applied is a defect to report, not a success.

Use conservative clearances and via sizes compatible with ordinary prototype manufacturing.

Route in priority order: power input and protection, the highest-current loops, then supply distribution, then clock and reset where present, then sensitive signals, then the remaining low-speed signals. This ordering is a routing priority, not permission to create a poor ground return.

Use short, direct routes for sensitive signals. Avoid unnecessary vias on sensitive signals.

Wherever a signal changes layers, a ground via should be close to it so the return current can follow; check the signal-integrity findings the export returns and report any that remain.

Ensure every external connector's signal and power pins reach the correct circuit.

Do not leave unrouted nets, dangling tracks, accidental shorts, or copper-clearance violations.

If a signal cannot be routed cleanly, revise component placement before adding excessive vias or compromising the ground plane.

Every net on the power path carries the load current and must be routed at a width its class declares, including the nets on either side of the fuse and the switch node.

JLCPCB rules

Use the selected JLCPCB fabrication capabilities as the manufacturing constraint.

Set explicit design-rule values for trace width, clearance, via diameter, drill diameter, annular ring, copper-to-board-edge clearance, and solder-mask requirements.

Prefer margins above the manufacturer's absolute minimum wherever practical.

Do not assume a minimum trace width is appropriate for a power path.

13. Connectors, mechanical design, and silkscreen

Every externally used connector must be reachable from the board edge and must have a verified footprint.

Use distinct connector reference designators and readable labels.

Provide mounting holes if the board dimensions permit them.

Keep connectors, mounting holes, and components clear of one another.

Check connector insertion direction, mating clearance, cable access, and component height. Connectors must open outward, away from the board.

Avoid silkscreen over pads, solder-mask openings, mounting holes, or component bodies.

Say plainly in the report if Synth gives you no way to place silkscreen text; do not claim labels exist that the exported board does not carry.

Do not invent an enclosure dimension. Use a practical board outline and report its final dimensions.

14. Component selection and design calculations

For every critical component, verify the exact part number, package, footprint, pin numbering, operating limits, and suitability. Do not copy a rating or a component value from a description without checking it against the part's own limits: a protection device that is wrong for the part it protects is worse than none.

Pay particular attention to:

The reverse-polarity MOSFET, above all its gate-source limit.
The fuse and its hold and trip currents against the load.
The TVS and its clamping voltage against everything downstream.
The MP2307, its inductor and its exposed pad.
The TPS22918 and its enable and soft-start.
The 3.3 V regulator and its dissipation.
The connectors and their current ratings.

Calculate rather than guess every value that sets a behaviour. Show the arithmetic in the report.

Name a manufacturer part number for every component and an LCSC number where the registry carries one. Do not invent LCSC numbers; list in the report every part that lacks one.

Use documented assumptions if a load, cable, or supply specification is missing.

Do not claim the design supports an unspecified load.

15. Power and status indicators (mandatory on every board)

Every board has a power indicator and status indicators, each with a current-limiting resistor calculated from the supply voltage, the LED's forward voltage and the current you want. Provide a power LED for each rail and a fault indicator, as the control-header section requires, each with a resistor calculated from the rail voltage and the LED's forward voltage.

Keep each indicator's current inside the power budget, give each a different colour or a different label, and make them visible when the board is mounted. Put each on a pin or a rail that does not conflict with the programming, boot, oscillator or bus functions, and say in the report what each one means.

16. GPIO, analog and expansion headers (mandatory on every board)

Every board provides clearly labelled, physically accessible expansion connectors. Every pin of the control header is a GPIO in the sense this rule means: a line a host board connects to. Expose the enable, the fault output (if it exists) and the scaled rail samples, beside 3.3 V and ground, and protect each as described here. Label every pin and state its voltage range and what drives it.

For each exposed pin record: the device pin name, the package pin number, the board header position, the logical signal name, any special function, and any electrical restriction. Do not expose a pin as general-purpose if it is reserved for essential circuitry or cannot safely do the job.

Use one consistent connector pitch and a practical mechanical arrangement. Identify pin 1 and the connector orientation. Put ground pins beside groups of signals. Assign each pin one documented function; do not give one physical pin two functions without saying so.

Every exposed signal must be protected for what a user can do to a header: series resistance and, where the pin can meet a cable or a hand, a clamp or ESD device, chosen so that the device's pin stays inside its allowed voltage range. Prevent a higher-voltage signal from reaching a lower-voltage input without level compatibility. Show the arithmetic.

Provide a complete pin-mapping table as a deliverable (see the deliverables section).

17. Power input and output protection (mandatory on every board)

Every board protects its power on the way in and on the way out. Design both deliberately and justify each element.

Power input. The input is a connector rated for the board's input current, with the chain you justify: fuse, reverse-polarity MOSFET and TVS.
Protect against reverse polarity wherever an external supply can be connected the wrong way round. Provide over-current protection sized from the load budget, and transient or ESD protection suited to how the connector is used (hot-plug, long cable, user contact). Prevent unsafe back-feeding into the source. If more than one power source is possible, implement a deliberate selection or OR-ing circuit and define what happens when two are connected at once; never join two regulated sources directly. Place each protection element so that normal operation, a reversed supply, an over-current fault and a transient each meet the right part.

Power output. Each output connector is an output of this board. Give each a stated current limit and say how it behaves on a short circuit: the load switch's limit, the buck's limit, a fuse or the regulator's own. Say what each connector and its mating part are rated for, and compare it with what the board can deliver.
Every power pin that leaves the board, and every supply a user can reach through a header, needs limits you can state: a maximum current, what happens on a short circuit (current limit, fuse, or foldback), and protection against a voltage being driven back into it. Do not advertise a rail as a general-purpose supply without defining its current limit.

Show the protection chain in the report as a sequence, and state for each element which fault it answers.

18. Power budget and power documentation

Calculate the power budget and document it. Include: every input voltage range; every output rail voltage; the maximum recommended input current; each regulator's current limit and dissipation at the worst-case input and load; the estimated consumption of every part; the current available to external loads; the power-source selection behaviour; and the expected thermal limits and the copper area you give each hot part. Include the load switch's current limit, the buck's current limit and the heat in the 3.3 V regulator.

Do not assume a regulator can deliver its advertised maximum under every operating condition. Do not advertise a rail's current without the calculation behind it.

19. Footprint and library validation

Before finalizing each component:

1. Confirm the manufacturer's part number.
2. Verify the package against the datasheet or the symbol.
3. Verify the symbol pin numbers.
4. Verify the footprint pad numbers.
5. Verify the pad-to-pin mapping.
6. Verify connector orientation.
7. Check physical dimensions and courtyard clearance.
8. Check availability and lifecycle where reliable information is available.

Do not select a footprint merely because its name resembles the component's name. Pay particular attention to connectors, the main IC, regulators and programming connectors, where a wrong pad mapping makes the board unusable. If a footprint cannot be verified, flag it as unresolved instead of declaring the board fabrication-ready.

20. Bill of materials

Produce a complete BOM with: reference designator, quantity, description, manufacturer, manufacturer part number, package, value or rating, footprint, a datasheet reference where available, assembly notes, and sourcing notes where verifiable.

Avoid vague entries such as "5 V regulator" or "USB connector" without an exact part. Every BOM line must be in the schematic and have a valid footprint, and every schematic component must be in the BOM, excluding only documented PCB-only features. Do not invent manufacturer or LCSC numbers; list in the report every part that lacks one.

21. Bring-up plan and firmware

Provide a practical plan for confirming that the fabricated board works. Document, in order: how to inspect the unpowered board for shorts; how to check resistance between each rail and ground; how to power the board safely for the first time; how to verify each regulated rail; how to check the reverse-polarity protection on a current-limited bench supply; how to confirm the enable and the soft start with a probe; how to check each output with a resistive load; how to verify the protection behaviour without exposing the board to destructive conditions.

This board has no programmable device, so there is no firmware; say so, and give the bring-up procedure alone.

Clearly distinguish checks that were run in this session from tests that need hardware. Do not claim hardware operation has been confirmed.

22. Schematic and netlist verification

The schematic must contain all required components and electrical connections.

Use meaningful net names, including:

VIN_12V_RAW
VIN_12V_FUSED
+12V_PROT
+5V
+5V_SW
+3V3
GND
BUCK_SW
BUCK_FB
BUCK_COMP
PFET_GATE
LOAD_EN
SENSE_12V
SENSE_5V
SENSE_3V3

These names describe intended functions. Verify that each name is used consistently and that the actual electrical connections match the architecture.

Inspect the netlist or equivalent connectivity representation before PCB routing.

Verify every connector pin against its intended net.

Check for:

Missing ground connections, including the buck's exposed pad.
Incorrect power-rail connections.
A TVS in front of the fuse, or one that conducts when the input is reversed.
A MOSFET placed the wrong way round for reverse-polarity protection.
A gate-source voltage beyond the MOSFET's absolute maximum.
Floating required pins, especially the load switch's enable.
A header pin that can present more than the part behind it allows.
Accidental short circuits.
Incorrect footprint pad numbering.
Missing decoupling or protection components.

Correct every actionable error before proceeding.

23. Automated verification, quality gates and export

Before declaring the design complete, run every relevant check the Synth tools support.

Electrical checks: the electrical-rule check; missing or conflicting power connections; unconnected required pins; short circuits; incorrect connector pin mapping; incorrect power polarity; supply-voltage compatibility; regulator current and thermal suitability; and the presence of decoupling and protection components.

PCB checks: the design-rule check; unrouted connections; copper shorts; clearance violations, including to the board edge; missing footprints; footprint and pad mapping mismatches; overlapping components; invalid drill or via dimensions; silkscreen conflicts; and manufacturing-constraint violations.

Functional checks: the main device can be powered; the programming method can program it in principle; every header exposes the intended signal; and every interface you advertise is supported by the hardware.

Required closure process: run the checks, inspect the results, correct the errors, rerun, and repeat until no blocking issue remains or a concrete tool limitation prevents it. Do not suppress a warning merely to get a clean report; document the reason for any warning you accept. A result too long to read in one call must be read in parts, and a result you have not read is not reported.

A successful DRC does not prove the circuit is correct, and a successful ERC does not prove the board works. Review the design as well as running the checks.

When the placement review gives a distance guideline, for example that a reset button or crystal sits too far from its MCU or a connector is not at an edge, act on it with placement hints and rerun the review; if you leave one, say why.

Look at the rendered schematic and confirm that the functional regions do not overlap and nothing is outside the sheet.

Export the design to KiCad with routing as a release export, using `allow_incomplete` only to inspect a failing result and saying so. Confirm the exported files exist and contain the design. Do not call the board manufacturing-ready if it has blocking errors, a missing footprint, an unverified critical mapping or an unresolved power-stage concern.

24. Required deliverables (the twelve)

Create and save the actual design artifacts using the Synth tools' supported project and export operations, and write the documents yourself with the Write tool into the working directory beside the project. Deliver all twelve:

1. Native design project: the editable source, named as given above.
2. Schematic: complete and readable, exported to KiCad.
3. PCB layout: components placed and every track routed, exported to KiCad.
4. Netlist or connectivity report: as the tools support, or a written net-by-net table if not.
5. Bill of materials: with exact component identities.
6. Pin-mapping table: device pins, header positions and functions.
7. Power budget: input limits, rail voltages, current limits and estimated consumption.
8. Design notes: architecture, assumptions and the important selections, with the calculations.
9. Firmware example: a minimal bring-up program where the board has a programmable device; otherwise the bring-up procedure alone, and say so.
10. Verification report: the actual ERC and DRC results, unresolved issues and accepted warnings, with the justification for each warning you accept.
11. Manufacturing files: Gerber layers, drill files and board outline if the tools can write them. If they cannot, say so; do not claim they exist.
12. Assembly documentation: component placement and orientation information if the tools support it, and the BOM and pick-and-place data they do write.

Save every artifact to a valid location and report its actual path. Never fabricate a file path or a download link.

25. Required final report

When the work is complete, give a concise but detailed engineering report.

A. Board summary: the board name and revision, the main device, the supply voltages, the available rails, the dimensions and the layer count.

B. Implemented functionality. Mark each function as one of: implemented and verified in the design; implemented but not fully verified; not implemented; blocked by a tool or component limitation. Cover every function this board has, including its GPIO and expansion headers and its power input and output protection.

C. Validation results: the actual ERC result, the actual DRC result, the unrouted nets, the known electrical risks, the footprint validation and the manufacturing readiness. Do not report zero errors unless the output supports it.

D. Remaining risks: every unresolved component, circuit, layout, firmware, manufacturing or hardware-testing issue.

E. Artifact locations: the actual path of each of the twelve deliverables.

Include as well:

Component list and the load assumptions.
Protection chain with the position of each element justified.
The MOSFET's gate-source calculation for a normal and a reversed supply.
TVS standoff and clamp against downstream ratings.
Buck design: feedback divider, inductor, capacitors, thermal.
Load switch design: soft-start time and inrush.
3.3 V regulator dissipation and the 5 V budget.
Control header: pinout, voltage ranges, and whether a fault signal exists.
PCB dimensions and layer arrangement.
Component-placement rationale.
Routing strategy, including the switch node.
ERC results.
DRC results.
Unrouted-net count.
Unresolved issues and assumptions.
The stated exception: no MCU, USB-C programming or SWD.
Native Synth project location.
KiCad export locations.

Clearly distinguish between: implemented in the design; verified by automated checks; verified by inspection or calculation; not verified on physical hardware; blocked or unresolved.

Do not claim that a physical prototype has been tested.

26. Execution rules

Begin by inspecting the Synth MCP environment and identifying its supported design capabilities.
Use the Synth tools to do the actual work wherever they support it.
Make sensible engineering decisions without asking unnecessary questions.
Document assumptions that materially affect electrical compatibility, manufacturability or function.
Prefer readily available, well-documented components.
Use exact datasheet-backed specifications and calculate values instead of guessing them.
Verify every important pin mapping and electrical connection.
Do not leave required power, reset, clock or programming pins unresolved.
Do not invent components, library entries, footprints, tool capabilities, verification results or generated files.
Do not stop after producing a conceptual schematic if PCB creation and routing are supported.
Do not describe an unrouted or unverified PCB as fabrication-ready.
If blocked, complete all safe, supported work and explain the remaining blocker clearly.

27. Final execution instruction

Begin by inspecting Synth MCP and the available component libraries.

Then implement the actual design in the prescribed order.

Do not stop after producing a conceptual schematic if Synth supports PCB creation and routing.

If a critical requirement cannot be implemented or verified, explain the exact blocker and finish all remaining supported work.

The final deliverable must be the actual protected_power_entry design, exported to KiCad, with routing and validation results documented.
