I2C Sensor Node PCB — Complete Electrical and Physical Design Specification

1. Objective and execution environment

Design a complete, production-oriented two-layer field I2C sensor-node PCB using Synth MCP tools and export the finished design to KiCad.

Board name: i2c_sensor_node

Native Synth project: i2c_sensor_node.synth

The board must be a real, electrically connected, routed PCB design, not just a schematic or conceptual layout.

Use the available Synth MCP tools to create the project, place real components, establish nets, generate the PCB, position components, route copper, run validation, and export the results to KiCad.

The only available agent tools are Synth MCP and Read, Write, and Edit. Do not assume shell access, web access, or external tool access. Discover the available Synth operations from their actual tool descriptions and server instructions.

Do not invent Synth commands, component library entries, successful tool operations, or validation results.

If a required operation is unavailable, complete the remaining supported work and document the limitation.

This board is a qualification board: it will be reviewed by an independent PCB reviewer, and the way it was made is part of what is reviewed. Work only through the Synth tools. Where Synth does something wrong, report it exactly rather than working around it silently.

2. Required system functionality

The board must include:

A 5 V DC input through a JST connector, and a USB-C receptacle that can also power the board and through which it is programmed.
Input protection in a stated order: a resettable fuse first, then a series Schottky diode.
A regulated 3.3 V supply generated from the protected 5 V by an AMS1117-3.3 low-dropout regulator.
An STM32G030 microcontroller.
A BME680 environmental sensor and a 24LC256 EEPROM on one I2C bus.
One pair of 4.7 kOhm I2C pull-up resistors for the whole bus.
A four-pin JST break-out carrying a UART to a host, with protection on both signal lines.
An SWD programming header and a USB-C programming path, so the MCU can be programmed with no firmware in it.
A power LED and a status LED.
Test points on the principal supply rails and the bus.
Clearly labelled, physically accessible external connectors.
A fully routed two-layer PCB suitable for JLCPCB manufacturing.
A complete schematic, BOM, PCB layout, and verification report.

Do not silently omit any requirement.

The order of the front end is the point of the board and it is what a reviewer will check: the fuse first, so that a fault current meets a part that limits it and resets; then the series diode, so that a reversed supply meets a blocking diode. Implement that order and justify it.

3. First inspect Synth and establish the design

Before creating the circuit:

Inspect the available Synth MCP tools and determine their actual capabilities.
Determine how the environment represents projects, components, footprints, nets, net classes, schematics, PCB placement, stackup, routing, validation, and KiCad export.
Create or open the project using the exact name given above.
Inspect available component libraries and verify the parts required for the design. Prefer registry parts that already carry a manufacturer part number and an LCSC number; import or author a part only when none fits, and say so.
Establish a component and signal plan before placing components.
Implement the design incrementally, inspecting the result after each major subsystem.

Use the following implementation order:

MCU selection and pin assignment.
Power input, protection and 3.3 V regulation.
MCU power, reset and SWD.
The I2C bus: BME680, EEPROM and pull-ups.
The UART break-out and its protection.
Status LED and test points.
Schematic connectivity verification.
PCB placement and routing.
ERC, DRC, and unresolved-issue review.
KiCad export and final artifact verification.

Do not route the PCB before the essential schematic connectivity and pin assignments are established.

4. MCU selection and pin assignment

Use an STM32G030 variant that provides every interface the board needs: one I2C bus, one UART, SWD, a status LED pin and a reset. Select the exact orderable part number and package before committing to the schematic. Verify the datasheet or symbol, the supply pins, the GPIO functions, the I2C and UART signals and the SWD pins.

The registry may carry an STM32G030 part whose pin map is wrong or incomplete. Check every pin of the part you use against the KiCad symbol, and import or author a corrected part if the registry's disagrees. State in the report which you did.

Create an explicit pin-assignment table before connecting the circuit, with: functional signal name, MCU GPIO name, physical package pin number, alternate function, destination, electrical constraints, and whether the pin is shared. Resolve every pin conflict before routing.

An internal oscillator is enough for this board; do not add a crystal unless you can justify it.

Establish from the part's documentation how the MCU chooses where to boot from, and what each boot-related pin must be tied to so the board boots from flash with nothing attached. Tie it explicitly; do not leave it floating. State what you chose.

Decide what to do with every pin you do not use (leave it unconnected, or tie it) and say so. Provide the supply decoupling the part's documentation requires, one capacitor per supply pin pair, as close as practical to the pins.

5. Power input and protection

Create a dedicated power-input section.

Input connector J1

Use a mechanically suitable, keyed JST connector with a verified footprint and a documented mating connector. Define its pinout explicitly: pin 1 the 5 V input, pin 2 ground. Verify the actual pin numbering against the chosen footprint. Clearly mark polarity on the PCB.

Protection and power path

Implement the sequence:

J1 → F1 polyfuse → D1 SS14-class Schottky in series → protected 5 V rail → regulator

Select F1 from the load budget: its hold current must exceed the worst-case load with margin, its trip current must be above anything the board draws in normal use, and its voltage rating must cover 5 V. Select D1 from the same budget: its forward voltage drop at the load current, its reverse voltage rating, and its dissipation. Show that the regulator still has enough input voltage once D1 and F1 have dropped theirs.

Add input capacitors rated for the voltage and the ripple, and place them where the regulator needs them.

Keep the raw input, the protected rail and the 3.3 V rail as distinct named nets.

Provide test points for the protected 5 V rail, the 3.3 V rail and ground.

6. 3.3 V regulator

Use an AMS1117-3.3 as the regulator. Verify its pinout (including that its tab is the output pin), its input and output capacitor requirements from its datasheet, its dropout voltage and its thermal limits.

Calculate the regulator's dissipation at the worst-case input voltage and load, and the temperature rise on the copper you give it. If the board cannot dissipate it, say so and change the design rather than the claim.

Calculate the 3.3 V load budget, including the MCU, the BME680, the EEPROM, the pull-ups, the LED and the UART break-out.

Name the regulated output +3V3.

7. I2C bus: sensor, EEPROM and pull-ups

Implement one 3.3 V I2C bus shared by the BME680 and the 24LC256, with the MCU as the controller.

Verify the BME680's package, pin numbering and footprint, its supply pins, its I2C address options and how the address is selected. Verify the 24LC256's package, pin numbering, address pins and write-protect pin. Establish both devices' addresses and show they do not collide.

Create the nets I2C_SDA and I2C_SCL and connect every device to them.

Use exactly one pair of 4.7 kOhm pull-up resistors to +3V3 for the whole bus. Show by calculation that this value suits the bus speed and the capacitance; do not add a second pair.

Connect each device's supply, ground, address and write-protect pins as its datasheet requires, and decouple each device locally.

Place the BME680 away from the regulator, the MCU and any other heat source, because its temperature and humidity readings depend on it. Say how you did that.

8. UART break-out and its protection

Provide a four-pin JST break-out carrying the MCU's UART to a host.

Define the pinout explicitly, for example ground, transmit, receive, and a fourth signal you can justify, and verify it against the chosen footprint. Make clear on the board and in the report which pin is which and which direction each signal flows.

Protect both signal lines. Choose clamps and series resistance appropriate to a connector that a user plugs and unplugs: the clamps must hold the MCU pin inside its allowed voltage range for the voltages a host can present, and must not load the line at normal speeds. Show the arithmetic.

Ensure the connector cannot expose the MCU to an incompatible voltage, and state what voltage the host must use.

Provide a test point or accessible pad for each UART line.

9. Reset, programming and status LED

Provide an SWD header with at least 3V3, GND, SWDIO, SWCLK and NRST. Verify every pin against the selected MCU package and a standard programmer. Ensure the header is accessible after the board is mounted.

Provide the reset circuitry the MCU needs: a pull-up and a decoupling capacitor on NRST if the documentation recommends them.

Provide one status LED on a general-purpose pin that does not conflict with SWD, I2C or the UART. Calculate its resistor from the supply voltage and the LED's forward voltage.

10. Grounding and power distribution

Use a common ground reference with deliberate current-return routing. On two layers, give the bottom layer over to as continuous a ground pour as you can, and keep it unbroken beneath the MCU and the sensor.

Keep the regulator's input and output loops short.

The ground connection between every part of the board must be explicit and electrically continuous.

Do not create arbitrary isolated ground nets that leave the circuit electrically disconnected.

11. Physical board organization

Use a deliberate placement strategy.

Region A: Power input

Place the 5 V connector at one board edge, with the fuse, the series diode and the input capacitor immediately inside it, then the regulator and its capacitors.

Region B: Logic

Place the MCU centrally with its decoupling capacitors as close as practical to its supply pins. Place the SWD header near an accessible edge.

Region C: Sensing

Place the BME680 and the EEPROM together on the bus, with the BME680 clear of the regulator and any other heat source, and the pull-ups near the bus.

Region D: Host interface

Place the UART connector at an edge with its protection immediately inside it.

Placement priorities, in order: connector accessibility and mechanical fit; the protection chain order; supply decoupling; sensor thermal isolation; I2C and UART routing; SWD; test-point accessibility; silkscreen readability; visual neatness.

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

A previous run of a board like this routed copper inside the board's own declared clearance in several places. Read the DRC result carefully and report any clearance finding exactly; do not describe a result as clean if it is not.

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

The polyfuse and the series Schottky diode, and their order.
The AMS1117-3.3 and its capacitors.
The STM32G030 and its pin map.
The BME680 and its footprint.
The 24LC256 and its footprint.
The UART protection devices.
The JST connectors and their pin numbering.

Calculate rather than guess every value that sets a behaviour. Show the arithmetic in the report.

Name a manufacturer part number for every component and an LCSC number where the registry carries one. Do not invent LCSC numbers; list in the report every part that lacks one.

Use documented assumptions if a load, cable, or supply specification is missing.

Do not claim the design supports an unspecified load.

15. Microcontroller (mandatory on every board)

Every board carries a microcontroller. The STM32G030 is this board's microcontroller; it has no native USB, so the USB path is a bridge.

Select the exact orderable part number and package before committing to the schematic. Before you commit, verify its operating-voltage range, its clock limits at that voltage, its memory, its GPIO, ADC, PWM, UART, SPI and I2C capabilities, its programming and boot requirements, its availability and its package footprint. If the preferred device is unavailable or unsuitable, choose a documented alternative and say so; never substitute silently.

The registry may carry a part whose pin map is wrong or incomplete. Check every pin against the KiCad symbol and the part's own documentation, and import or author a corrected part if the registry's disagrees. State which you did.

Create an explicit pin-assignment table before connecting the circuit, with: signal name, device pin name, package pin number, alternate function, destination, electrical constraints and whether the pin is shared. Resolve every conflict before routing.

Connect every supply and ground pin, with the decoupling and any analogue-supply filtering the documentation requires. Establish how the device chooses where to boot from and tie every boot pin explicitly so it boots from flash with nothing attached. Implement the reset circuit it needs, with a reset button, and give it an SWD (or equivalent) header as well as the USB path, so it can be programmed even with no firmware in it. Decide what to do with every unused pin and say so.

16. USB-C programming interface (mandatory on every board)

Every board has a USB-C receptacle through which it is programmed. The STM32G030 has no native USB, so use a USB-to-UART bridge (a CH340, CP210x or equivalent you can verify) wired to the MCU's UART, with the reset and boot-control signals that let the MCU's ROM bootloader be entered from the host. The USB-C VBUS may also power the board: define how it combines with the JST input, since the prescribed fuse-then-Schottky chain protects the JST path and the USB path needs its own protection and a deliberate power-selection circuit.

USB-C requirements, whichever route you use:

Use the correct receptacle pinout and verify the footprint pad numbers against it. For a receptacle that acts as a device or a power sink, provide the CC pull-down resistors the specification requires, and verify their values and connections. For USB 2.0 data, connect D+ and D- correctly, including the duplicated connector pins. Connect the shield and the ground pins correctly, and make sure the receptacle is mechanically retained.

Add ESD protection on the exposed data lines, on VBUS and on the CC pins. Never connect VBUS directly to a low-voltage supply. Prevent the board from back-feeding the host's port. If the board can also be powered another way, implement a deliberate power selection or OR-ing circuit and define what happens when both are connected at once.

Route the differential pair as a pair, short, over an unbroken reference. Do not claim impedance control or high-speed compliance the stackup and the process cannot support.

Say how the first firmware is loaded into a board that has none, how a bootloader is installed if one is needed, and what the user does to enter it. Do not assume a bridge chip supports automatic reset or every operating system; state what you verified.

17. Power and status indicators (mandatory on every board)

Every board has a power indicator and status indicators, each with a current-limiting resistor calculated from the supply voltage, the LED's forward voltage and the current you want. Provide a power LED on the 3.3 V rail, a status LED on a general-purpose pin, and UART transmit and receive activity indicators if you can fit them.

Keep each indicator's current inside the power budget, give each a different colour or a different label, and make them visible when the board is mounted. Put each on a pin or a rail that does not conflict with the programming, boot, oscillator or bus functions, and say in the report what each one means.

18. GPIO, analog and expansion headers (mandatory on every board)

Every board provides clearly labelled, physically accessible expansion connectors. Expose every free GPIO of the MCU on a header, including its ADC-capable pins, any timer or PWM pin and the SPI pins if the package has them, beside 3.3 V and ground. Keep the pins the I2C bus, the UART, SWD and the LED use off the general-purpose header, and say which they are.

For each exposed pin record: the device pin name, the package pin number, the board header position, the logical signal name, any special function, and any electrical restriction. Do not expose a pin as general-purpose if it is reserved for essential circuitry or cannot safely do the job.

Use one consistent connector pitch and a practical mechanical arrangement. Identify pin 1 and the connector orientation. Put ground pins beside groups of signals. Assign each pin one documented function; do not give one physical pin two functions without saying so.

Every exposed signal must be protected for what a user can do to a header: series resistance and, where the pin can meet a cable or a hand, a clamp or ESD device, chosen so that the device's pin stays inside its allowed voltage range. Prevent a higher-voltage signal from reaching a lower-voltage input without level compatibility. Show the arithmetic.

Provide a complete pin-mapping table as a deliverable (see the deliverables section).

19. Power input and output protection (mandatory on every board)

Every board protects its power on the way in and on the way out. Design both deliberately and justify each element.

Power input. The input is a JST 5 V connector, and the prescribed chain is the fuse first, then the series Schottky. Add transient protection suited to a field cable, and state the input voltage range the board tolerates.
Protect against reverse polarity wherever an external supply can be connected the wrong way round. Provide over-current protection sized from the load budget, and transient or ESD protection suited to how the connector is used (hot-plug, long cable, user contact). Prevent unsafe back-feeding into the source. If more than one power source is possible, implement a deliberate selection or OR-ing circuit and define what happens when two are connected at once; never join two regulated sources directly. Place each protection element so that normal operation, a reversed supply, an over-current fault and a transient each meet the right part.

Power output. The 3.3 V pin on the UART connector or on any header must carry a stated current limit. Protect the UART lines and every header pin as described above.
Every power pin that leaves the board, and every supply a user can reach through a header, needs limits you can state: a maximum current, what happens on a short circuit (current limit, fuse, or foldback), and protection against a voltage being driven back into it. Do not advertise a rail as a general-purpose supply without defining its current limit.

Show the protection chain in the report as a sequence, and state for each element which fault it answers.

20. Power budget and power documentation

Calculate the power budget and document it. Include: every input voltage range; every output rail voltage; the maximum recommended input current; each regulator's current limit and dissipation at the worst-case input and load; the estimated consumption of every part; the current available to external loads; the power-source selection behaviour; and the expected thermal limits and the copper area you give each hot part. Include the sensor's measurement-mode current and the EEPROM's write current.

Do not assume a regulator can deliver its advertised maximum under every operating condition. Do not advertise a rail's current without the calculation behind it.

21. Footprint and library validation

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

22. Bill of materials

Produce a complete BOM with: reference designator, quantity, description, manufacturer, manufacturer part number, package, value or rating, footprint, a datasheet reference where available, assembly notes, and sourcing notes where verifiable.

Avoid vague entries such as "5 V regulator" or "USB connector" without an exact part. Every BOM line must be in the schematic and have a valid footprint, and every schematic component must be in the BOM, excluding only documented PCB-only features. Do not invent manufacturer or LCSC numbers; list in the report every part that lacks one.

23. Bring-up plan and firmware

Provide a practical plan for confirming that the fabricated board works. Document, in order: how to inspect the unpowered board for shorts; how to check resistance between each rail and ground; how to power the board safely for the first time; how to verify each regulated rail; how to confirm both I2C devices answer at their addresses; how to confirm the UART link to a host; how to verify the protection behaviour without exposing the board to destructive conditions.

Provide a minimal firmware example that reads the BME680 over I2C, reads and writes the EEPROM, and reports both over the UART, with the toolchain and the way to load it over SWD.

Clearly distinguish checks that were run in this session from tests that need hardware. Do not claim hardware operation has been confirmed.

24. Schematic and netlist verification

The schematic must contain all required components and electrical connections.

Use meaningful net names, including:

VIN_5V_RAW
VIN_5V_FUSED
+5V_PROT
+3V3
GND
I2C_SDA
I2C_SCL
UART_TX
UART_RX
SWDIO
SWCLK
NRST
LED_STATUS

These names describe intended functions. Verify that each name is used consistently and that the actual electrical connections match the architecture.

Inspect the netlist or equivalent connectivity representation before PCB routing.

Verify every connector pin against its intended net.

Check for:

Missing MCU supply or ground connections.
Incorrect power-rail connections.
A fuse and diode in the wrong order.
A reversed series diode.
Floating required pins.
Accidental short circuits.
An I2C address collision.
More than one pair of pull-ups.
Incorrect footprint pad numbering, especially the BME680 and the regulator tab.
Missing decoupling or protection components.

Correct every actionable error before proceeding.

25. Automated verification, quality gates and export

Before declaring the design complete, run every relevant check the Synth tools support.

Electrical checks: the electrical-rule check; missing or conflicting power connections; unconnected required pins; short circuits; incorrect connector pin mapping; incorrect power polarity; supply-voltage compatibility; regulator current and thermal suitability; and the presence of decoupling and protection components.

PCB checks: the design-rule check; unrouted connections; copper shorts; clearance violations, including to the board edge; missing footprints; footprint and pad mapping mismatches; overlapping components; invalid drill or via dimensions; silkscreen conflicts; and manufacturing-constraint violations.

Functional checks: the main device can be powered; the programming method can program it in principle; every header exposes the intended signal; and every interface you advertise is supported by the hardware.

Required closure process: run the checks, inspect the results, correct the errors, rerun, and repeat until no blocking issue remains or a concrete tool limitation prevents it. Do not suppress a warning merely to get a clean report; document the reason for any warning you accept. A result too long to read in one call must be read in parts, and a result you have not read is not reported.

A successful DRC does not prove the circuit is correct, and a successful ERC does not prove the board works. Review the design as well as running the checks.

When the placement review gives a distance guideline, for example that a reset button or crystal sits too far from its MCU or a connector is not at an edge, act on it with placement hints and rerun the review; if you leave one, say why.

Look at the rendered schematic and confirm that the functional regions do not overlap and nothing is outside the sheet.

Export the design to KiCad with routing as a release export, using `allow_incomplete` only to inspect a failing result and saying so. Confirm the exported files exist and contain the design. Do not call the board manufacturing-ready if it has blocking errors, a missing footprint, an unverified critical mapping or an unresolved power-stage concern.

26. Required deliverables (the twelve)

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

27. Required final report

When the work is complete, give a concise but detailed engineering report.

A. Board summary: the board name and revision, the main device, the supply voltages, the available rails, the dimensions and the layer count.

B. Implemented functionality. Mark each function as one of: implemented and verified in the design; implemented but not fully verified; not implemented; blocked by a tool or component limitation. Cover every function this board has, including its GPIO and expansion headers and its power input and output protection.

C. Validation results: the actual ERC result, the actual DRC result, the unrouted nets, the known electrical risks, the footprint validation and the manufacturing readiness. Do not report zero errors unless the output supports it.

D. Remaining risks: every unresolved component, circuit, layout, firmware, manufacturing or hardware-testing issue.

E. Artifact locations: the actual path of each of the twelve deliverables.

Include as well:

Selected MCU, package and component list.
Pin-assignment table.
Protection chain, with the fuse and diode calculations.
Regulator design and its dissipation.
I2C addresses and the pull-up calculation.
UART pinout and protection calculation.
PCB dimensions and layer arrangement.
Component-placement rationale, including the sensor's thermal isolation.
Routing strategy.
ERC results.
DRC results, with every clearance finding stated.
Unrouted-net count.
Unresolved issues and assumptions.
Native Synth project location.
KiCad export locations.

Clearly distinguish between: implemented in the design; verified by automated checks; verified by inspection or calculation; not verified on physical hardware; blocked or unresolved.

Do not claim that a physical prototype has been tested.

28. Execution rules

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

29. Final execution instruction

Begin by inspecting Synth MCP and the available component libraries.

Then implement the actual design in the prescribed order.

Do not stop after producing a conceptual schematic if Synth supports PCB creation and routing.

If a critical requirement cannot be implemented or verified, explain the exact blocker and finish all remaining supported work.

The final deliverable must be the actual i2c_sensor_node design, exported to KiCad, with routing and validation results documented.
