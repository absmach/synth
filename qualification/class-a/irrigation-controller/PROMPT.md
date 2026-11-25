# Prompt

The prompts used to produce this board, recovered verbatim from the git history of this repository.

The board was made by an agent driving `synth mcp`. Changes made after the last prompt, for example in response to review, are in the git history of this pull request.

## prompt

Source in history: `qualification/class-a/irrigation-controller/agent-run/prompt.md`

~~~~text
Irrigation Controller PCB — Complete Electrical and Physical Design Specification
1. Objective and execution environment

Design a complete, production-oriented, two-zone irrigation controller PCB using Synth MCP tools and export the finished design to KiCad.

Board name: irrigation_controller

Native Synth project: irrigation_controller.synth

The board must be a real, electrically connected, routed PCB design, not just a schematic or conceptual layout.

Use the available Synth MCP tools to create the project, place real components, establish nets, generate the PCB, position components, route copper, run validation, and export the results to KiCad.

The only available agent tools are Synth MCP and Read, Write, and Edit. Do not assume shell access, web access, or external tool access. Discover the available Synth operations from their actual tool descriptions and server instructions.

Do not invent Synth commands, component library entries, successful tool operations, or validation results.

If a required operation is unavailable, complete the remaining supported work and document the limitation.

2. Required system functionality

The board must include:

A 12 V DC input through a JST connector.
Reverse-polarity protection, input overcurrent protection, and transient suppression.
A regulated 3.3 V supply generated from the 12 V input by a buck regulator.
An STM32G030 microcontroller.
An 8 MHz external crystal circuit, provided the selected MCU package supports it.
A reset button.
An accessible SWD programming/debug header.
Two independent 12 V solenoid-valve outputs, switched using low-side MOSFET drivers.
Two analog soil-moisture probe interfaces with switched excitation to reduce electrode corrosion.
One flow-meter pulse input.
An I²C temperature/humidity sensor.
An ambient-light sensor.
An I²C calibration EEPROM.
An external I²C expansion connector.
A power LED and a valve-open status LED.
Test points on the principal supply rails and important debug signals.
Clearly labelled, physically accessible external connectors.
A fully routed two-layer PCB suitable for JLCPCB manufacturing.
A complete schematic, BOM, PCB layout, and verification report.

Do not silently omit any requirement.

3. First inspect Synth and establish the design

Before creating the circuit:

Inspect the available Synth MCP tools and determine their actual capabilities.
Determine how the environment represents projects, components, footprints, nets, schematics, PCB placement, routing, validation, and KiCad export.
Create or open the project using the exact name irrigation_controller.
Inspect available component libraries and verify the parts required for the design.
Establish a component and signal plan before placing components.
Implement the design incrementally, inspecting the result after each major subsystem.

Use the following implementation order:

MCU selection and pin assignment.
Power input and regulation.
MCU power, clock, reset, and SWD.
Solenoid drivers and protection.
Soil-moisture interfaces.
Flow-meter interface.
I²C sensors, EEPROM, and expansion connector.
LEDs and test points.
Schematic connectivity verification.
PCB placement and routing.
ERC, DRC, and unresolved-issue review.
KiCad export and final artifact verification.

Do not route the PCB before the essential schematic connectivity and pin assignments are established.

4. MCU selection and pin assignment

Use an STM32G030 variant that provides sufficient pins and supports every required interface.

Select the exact orderable part number and package before committing to the schematic. Verify the datasheet, symbol, footprint, supply pins, oscillator pins, ADC channels, GPIO functions, I²C signals, timer capabilities, and SWD pins.

Create an explicit pin-assignment table before connecting the circuit.

The table must include:

Functional signal name.
MCU GPIO name.
Physical package pin number.
Alternate function, if applicable.
Destination component or connector.
Electrical constraints.
Whether the pin is shared with another peripheral.

Reserve the following signals:

Two independent valve-control outputs.
Two soil-moisture ADC inputs.
Required soil-probe excitation controls.
One flow-meter pulse input.
I²C SDA and SCL.
SWDIO and SWCLK.
NRST.
Crystal oscillator pins.
Power and ground pins.
Power and valve status indicators.

Use additional pins for BOOT configuration or other required MCU functions as appropriate.

Resolve all pin conflicts before routing.

If the selected package cannot support all functions, select a suitable alternative within the STM32G030 family where available, or report the conflict. Do not reuse an occupied pin without explicitly resolving the conflict.

5. Power-input and protection circuit

Create a dedicated power-input section.

Input connector J1

Use a mechanically suitable, keyed JST connector with a verified footprint and documented mating connector.

Define its pinout explicitly:

J1.1: VIN_12V_RAW
J1.2: GND

Verify the actual connector pin numbering against the chosen footprint. Never assume the physical orientation.

Clearly mark polarity on the PCB.

Protection and power path

Implement the following functional sequence:

J1 → F1 → reverse-polarity protection → protected 12 V rail → buck regulator

Add a suitably selected TVS diode to protect the incoming supply from realistic transients.

The exact position of the TVS, fuse, and reverse-polarity device must follow the selected protection architecture and datasheet recommendations. Place the transient protection near the input connector so the surge-current path is short.

Select:

F1: fuse or resettable fuse suitable for the anticipated load.
Reverse-polarity protection: suitably rated MOSFET or other appropriate protection circuit.
DTVS1: TVS diode selected for the expected supply range and transient environment.
Input capacitors: suitable voltage rating and ripple-current characteristics.

Ensure the TVS does not conduct during the normal input-voltage range.

Do not select component ratings until the expected supply tolerance, solenoid current, and transient assumptions have been documented.

Keep the 12 V input, protected 12 V distribution, and 3.3 V logic rail as distinct named nets.

6. 3.3 V buck regulator

Use a real buck-regulator IC appropriate for a nominal 12 V input and a 3.3 V output.

Do not use a linear regulator as the primary 12 V-to-3.3 V converter.

Implement every external component required by the selected regulator, including:

Input bypass capacitors.
Inductor.
Catch diode if required by the regulator topology.
Output capacitors.
Feedback resistors where required.
Enable and bootstrap components where required.
Any recommended compensation components.

Use the manufacturer's reference design and layout guidance.

Calculate the feedback divider using the regulator's actual reference voltage. Verify the inductor current rating, saturation current, switching frequency, diode ratings, output ripple, startup behavior, and capacitor requirements.

Name the regulated output +3V3.

The output must supply the MCU, sensors, EEPROM, interface pull-ups, and logic circuitry.

Provide test points:

TP_VIN: protected input supply.
TP_12V: protected valve supply.
TP_3V3: regulated logic rail.
TP_GND: ground.

If the protected input and valve supply are the same electrical rail, use consistent net naming and document the relationship rather than creating misleading duplicate nets.

Calculate the total 3.3 V load budget, including sensors, EEPROM, pull-ups, LEDs, and external connector loads.

7. Grounding and power distribution

Use a common ground reference with deliberate current-return routing.

Do not create arbitrary isolated ground nets that leave the circuit electrically disconnected.

Use separate physical routing regions for:

High-current solenoid return paths.
Buck-regulator switching currents.
Sensitive soil-moisture analog circuitry.
MCU and I²C signal returns.

Keep valve currents out of narrow analog-ground return paths.

Use a continuous bottom-layer ground plane wherever practical. Keep the ground plane intact beneath sensitive analog signals and the MCU, and avoid routing high-current valve returns through the analog measurement area.

The ground connection between the MCU, sensors, soil probes, valve drivers, and power supply must be explicit and electrically continuous.

Place the buck regulator and its high-current switching components so their switching-current loops remain short and compact.

8. Two solenoid-valve output circuits

Implement two electrically independent low-side switching circuits.

Use the following topology for each valve:

+12V_VALVE → external valve coil → valve output node → N-channel MOSFET drain

MOSFET source → GND

MCU GPIO → gate resistor → MOSFET gate

Add a gate-to-source pull-down resistor to ensure the MOSFET stays off during reset and startup.

Use a logic-level N-channel MOSFET with a documented RDS(on) specification at the actual available gate voltage. Do not rely solely on the threshold-voltage specification.

Size the MOSFET for:

Maximum coil current.
Continuous and startup current.
Dissipation at the expected current.
Safe operating area.
Voltage spikes and transient margin.
Thermal performance on the selected PCB copper.
Flyback protection

Provide a correctly rated flyback suppression device for each inductive valve load.

For a conventional flyback diode across a DC valve coil:

Cathode connects to the positive valve supply.
Anode connects to the switched low-side output.

Place the suppression device close to the valve connector or coil-current path, depending on the selected implementation.

If the valve is connected through a long external cable, account for cable-induced transients and verify whether additional transient suppression is needed at the connector.

Do not place a flyback diode in a position that shorts the 12 V supply during normal operation.

Output connectors

Use separate, clearly labelled, two-pin connectors:

J_VALVE1: +12V_VALVE, VALVE1_SW
J_VALVE2: +12V_VALVE, VALVE2_SW

Verify pin numbering and polarity against the actual footprints.

The switched pin must be a low-side output, not an actively driven 12 V output.

Valve status LED

Implement one valve-open status LED.

Define the status behavior explicitly: it should indicate that at least one valve has been commanded on, unless a different behavior is deliberately selected.

Prefer a logic-level indicator driven by the controller rather than placing the LED in the solenoid-current path.

If either valve can operate independently, ensure the indicator behavior is unambiguous.

Valve safety

Both MOSFETs must default to the off state during reset, boot, and loss of MCU control.

Do not connect a solenoid directly to an MCU GPIO.

Do not claim a maximum valve current until the valve current rating and thermal calculations support it.

If the intended coil current is unknown, choose a documented design assumption, label it clearly, and identify the maximum supported load as an unresolved product specification.

9. Soil-moisture measurement circuits

Create two independent analog soil-moisture channels.

Connect:

J_SOIL1 to soil-moisture channel 1.
J_SOIL2 to soil-moisture channel 2.

Each connector must expose the required probe excitation, measurement, and ground connections for the chosen probe architecture.

Switched excitation

Do not continuously energize resistive soil-moisture probes.

Implement a dedicated excitation-switching circuit for each channel, controlled by the MCU. Use an appropriate MOSFET or analog switch selected for the probe current, voltage, leakage, and measurement requirements.

The MCU must be able to:

Disable probe excitation between measurements.
Enable excitation only when a reading is required.
Allow the electrical measurement to settle.
Acquire the ADC measurement.
Disable excitation again.

Use a documented default-off state so the probes are not permanently energized during MCU reset.

Where appropriate for the chosen probe type, consider reversing measurement polarity or using another corrosion-mitigation strategy. Do not add a polarity-reversal circuit without validating its measurement topology.

Analog measurement path

For each channel, implement the necessary biasing, current limiting, ADC input protection, and optional RC filtering.

Ensure the signal reaching the MCU ADC stays within the allowed input range under all intended operating conditions.

Choose resistor values based on the actual probe electrical behavior and the MCU ADC's acquisition requirements.

Avoid excessive RC time constants that prevent the ADC from settling within the sampling window.

Keep the two channels independent so either probe can be measured without unintentionally energizing the other.

Provide test points or accessible measurement pads for both analog signals.

If the probe technology is not specified, use a clearly documented resistive-probe assumption. Do not silently assume that a capacitive probe or an arbitrary commercial module uses the same electrical interface.

10. Flow-meter pulse input

Provide a dedicated connector J_FLOW with an explicitly documented pinout:

Sensor supply, if required.
Ground.
Flow pulse signal.

Determine whether the intended meter has an open-collector, open-drain, push-pull, or other output before selecting the final interface.

For an open-collector or open-drain meter, provide a pull-up to the appropriate logic rail.

Add suitable input protection and, if necessary, a small RC filter or Schmitt-trigger input stage.

Choose filtering that rejects interference without suppressing legitimate meter pulses at the maximum expected pulse frequency.

Ensure the pulse signal is safe for the MCU's GPIO voltage limits.

Provide a test point on the conditioned pulse signal.

If the meter's electrical output type and maximum pulse frequency are unknown, document the assumed interface and identify what must be confirmed before manufacture.

11. I²C sensors, EEPROM, and expansion connector

Implement one 3.3 V I²C bus shared by:

Temperature/humidity sensor.
Ambient-light sensor.
Calibration EEPROM.
External I²C connector.

Use actual supported components with verified footprints and manufacturer documentation.

Address verification

Before selecting the devices, verify their I²C addresses and address-configuration options.

Ensure there are no address collisions among the three onboard devices.

If the addresses cannot be made unique, implement a suitable bus multiplexer or select alternative components.

Do not leave address conflicts unresolved.

I²C electrical connections

Create the nets:

I2C_SDA
I2C_SCL

Connect each onboard device's corresponding SDA and SCL pins to these nets.

Connect all device supply and ground pins correctly.

Provide appropriate pull-up resistors to +3V3, calculated for the intended bus speed, total capacitance, and connected devices.

Avoid installing multiple strong pull-up resistor pairs in parallel without evaluating their effective resistance.

Ensure that the external I²C connector cannot expose the bus to an incompatible voltage.

External connector

Use a connector with the following minimum pinout:

GND
+3V3
I2C_SDA
I2C_SCL

Clearly label connector pin 1 and the signal order.

State the maximum permitted external load and clarify that the external connector does not provide a general-purpose 5 V output.

If long external cables are expected, evaluate the need for additional ESD protection and suitable bus buffering.

EEPROM

Select an EEPROM with adequate capacity for the intended calibration data and appropriate write endurance.

Connect its address pins, supply, ground, SDA, and SCL according to the datasheet.

Define its address and any required write-protect behavior.

Provide the correct local decoupling capacitor.

12. Reset, clock, programming, and debugging
Clock

Implement an 8 MHz crystal circuit only after confirming that the selected MCU package exposes the required oscillator pins.

Connect the crystal to the documented oscillator pins and place any required load capacitors immediately adjacent to the MCU.

Calculate the load capacitors from the crystal's specified load capacitance, MCU pin capacitance, and PCB parasitics.

Keep crystal traces short, avoid vias where practical, and do not route noisy signals beneath or alongside the oscillator circuit.

Reset

Provide a reset push button connected to the MCU's reset circuitry.

Use the recommended reset pull-up or other required components and local decoupling if specified.

Ensure the button is physically accessible and clearly labelled RESET.

SWD programming header

Provide a clearly labelled SWD header with at least:

3V3
GND
SWDIO
SWCLK
NRST

Include additional signals only where useful and supported.

Verify every pin against the selected MCU package and programmer requirements.

Ensure the programming header is accessible after enclosure installation or board mounting.

Add nearby test points for SWDIO, SWCLK, NRST, and 3.3 V where space allows.

Do not assume a USB bootloader exists. The board must be programmable using the documented SWD interface.

13. Status indicators and test points

Provide:

LED_PWR: indicates the regulated logic rail is present.
LED_VALVE: indicates the defined valve-command status.

Calculate all LED current-limiting resistors from the actual supply voltage and LED forward voltage.

Use GPIO assignments that do not conflict with programming, I²C, ADC, or oscillator functions.

Provide test points for:

Protected input supply.
Protected 12 V valve supply.
3.3 V logic supply.
Ground.
Soil analog channel 1.
Soil analog channel 2.
Flow pulse input.
NRST.
SWDIO.
SWCLK.

Place test points so they can be reached with an oscilloscope probe or multimeter without contacting neighboring high-current pads.

14. Physical board organization

Use a deliberate four-region placement strategy.

Region A: Power input and regulation

Place the 12 V connector at one board edge.

Immediately inside the connector, place the input fuse, reverse-polarity protection, and TVS protection.

Place the buck regulator, inductor, diode if required, and associated capacitors close together.

Keep the switching-current loop compact.

Keep the regulator switching node and inductor away from the soil-measurement circuitry, crystal, and high-impedance sensor inputs.

Place the power-rail test points near the corresponding circuits while keeping them accessible.

Region B: Solenoid drivers

Place both valve connectors along a board edge.

Place the corresponding MOSFETs, gate resistors, gate pull-downs, and flyback suppression components close to their respective valve-current paths.

Arrange the circuit so each valve's high-current path is short:

12 V distribution → valve connector → external coil → switched return → MOSFET → power ground

Keep high-current copper away from analog input traces.

Use wide traces or copper pours sized for the documented maximum valve current.

Keep switching return currents out of the sensitive analog-ground region.

Make the two driver channels visually and electrically distinct.

Region C: MCU and digital logic

Place the MCU in a central logic region.

Place its decoupling capacitors as close as practical to the corresponding supply pins.

Place the crystal and load components immediately adjacent to the oscillator pins.

Place the reset button and SWD header near an accessible edge.

Place the I²C sensors and EEPROM in the digital/sensor region, away from the buck switching node and valve-driver power loops.

Place the external I²C connector near the board edge.

Region D: Analog sensor interfaces

Place the two soil-probe connectors along a board edge.

Place their excitation switches, series resistors, filters, and ADC-conditioning components between the connectors and MCU.

Keep the analog traces short.

Keep both channels physically separated from the valve MOSFETs, switching node, inductor, and high-current copper.

Route analog signals over an uninterrupted ground reference where practical.

Place the flow-meter input-conditioning circuit near its connector.

Do not route valve-current paths through the soil-probe region.

Placement priorities

Follow these priorities in order:

Connector accessibility and mechanical fit.
Power and protection circuit topology.
Valve-driver current paths.
MCU supply decoupling.
Crystal placement.
Analog signal integrity.
I²C and SWD routing.
Test-point accessibility.
Silkscreen readability.
General visual neatness.

Do not sacrifice electrical performance merely to achieve a symmetrical layout.

15. PCB stack-up, copper, and routing

Use a two-layer PCB suitable for JLCPCB.

Prefer:

Top layer: components and most signal routing.
Bottom layer: a largely continuous ground plane, with unavoidable signal routing only where necessary.

Do not split the ground plane into arbitrary islands.

Use appropriately sized copper for the 12 V input and valve-current paths. Calculate trace widths from current, copper thickness, acceptable temperature rise, and the expected load.

Use conservative clearances and via sizes compatible with ordinary prototype manufacturing.

Route in the following order:

Power-input and protection connections.
Buck-regulator power loop.
High-current valve outputs and returns.
MCU power and ground.
Crystal and reset.
SWD programming signals.
Soil-measurement analog paths.
Flow-meter input.
I²C bus.
LEDs and remaining low-speed signals.

This ordering is a routing priority, not permission to create a poor ground return.

Use short, direct routes for sensitive analog signals and oscillator connections.

Avoid unnecessary vias on sensitive signals.

Keep the buck switching node small and away from sensitive signals.

Ensure every external connector's signal and power pins reach the correct circuit.

Do not leave unrouted nets, dangling tracks, accidental shorts, or copper-clearance violations.

If a signal cannot be routed cleanly, revise component placement before adding excessive vias or compromising the ground plane.

JLCPCB rules

Use the selected JLCPCB fabrication capabilities as the manufacturing constraint.

Set explicit design-rule values for trace width, clearance, via diameter, drill diameter, annular ring, copper-to-board-edge clearance, and solder-mask requirements.

Prefer margins above the manufacturer's absolute minimum wherever practical.

Do not assume a minimum trace width is appropriate for a valve-current path.

16. Connectors, mechanical design, and silkscreen

Every externally used connector must be reachable from the board edge and must have a verified footprint.

Use distinct connector reference designators and readable labels.

At minimum, label:

12 V input and polarity.
Valve 1 and Valve 2.
Soil probe 1 and Soil probe 2.
Flow-meter connector.
External I²C connector.
SWD programming header.
Reset button.
Power LED.
Valve-status LED.
Test points and principal supply rails.

Provide mounting holes if the enclosure and board dimensions permit them.

Keep connectors, mounting holes, and components clear of one another.

Check connector insertion direction, mating clearance, cable access, and component height.

Avoid silkscreen over pads, solder-mask openings, mounting holes, or component bodies.

Do not invent an enclosure dimension. Use a practical board outline and report its final dimensions.

17. Component selection and design calculations

For every critical component, verify the exact part number, package, footprint, pin numbering, operating limits, and suitability.

Pay particular attention to:

Buck regulator and its inductor.
Input fuse.
TVS diode.
Reverse-polarity protection device.
Solenoid MOSFETs.
Flyback diodes or other suppression devices.
MCU and crystal.
Soil-probe excitation switches.
I²C sensors and EEPROM.
External connectors.

Calculate rather than guess:

Regulator feedback resistor values.
Regulator capacitor and inductor requirements.
MOSFET gate resistor and pull-down values.
MOSFET power dissipation.
Flyback-device ratings.
LED resistors.
I²C pull-up values.
ADC input-conditioning values.
Trace widths for expected currents.

Use documented assumptions if a sensor, valve, cable, or supply specification is missing.

Do not claim the design supports unspecified valve currents or cable lengths.

18. Schematic and netlist verification

The schematic must contain all required components and electrical connections.

Use meaningful net names, including:

VIN_12V_RAW
+12V_VALVE
+3V3
GND
VALVE1_GATE
VALVE2_GATE
VALVE1_SW
VALVE2_SW
SOIL1_EXC_EN
SOIL2_EXC_EN
SOIL1_ADC
SOIL2_ADC
FLOW_PULSE
I2C_SDA
I2C_SCL
SWDIO
SWCLK
NRST

These names describe intended functions. Verify that each name is used consistently and that the actual electrical connections match the architecture.

Inspect the netlist or equivalent connectivity representation before PCB routing.

Verify every connector pin against its intended net.

Check for:

Missing MCU supply or ground connections.
Incorrect power-rail connections.
Floating required pins.
Accidental short circuits.
Incorrect MOSFET source/drain connections.
Reversed flyback diodes.
Incorrect USB or programming assumptions.
I²C address collisions.
Conflicting MCU pin assignments.
Incorrect footprint pad numbering.
Missing decoupling or protection components.

Correct every actionable error before proceeding.

19. Verification and export

Run the available electrical-rule checks and PCB design-rule checks.

Inspect the actual results and fix all blocking errors.

Confirm that:

All required nets are connected.
The PCB has no unintended unrouted nets.
No unintended copper shorts exist.
All components have valid footprints.
All external connectors are accessible.
The power and valve-current paths have appropriate copper.
The analog and oscillator circuits follow their placement constraints.
The board outline and fabrication settings are valid.
The schematic and PCB remain synchronized.

Repeat checks after significant placement or routing changes.

Export the completed design to KiCad using the supported Synth export operation.

Save the native Synth project as irrigation_controller.synth and name the board irrigation_controller.

Verify that the exported schematic and PCB files exist at their reported locations and contain the expected design.

If supported, provide Gerber files, drill files, BOM, and pick-and-place outputs.

Do not claim the PCB is manufacturing-ready if it still contains blocking errors, missing footprints, unverified critical component mappings, or unresolved power-stage concerns.

20. Required final report

Provide a final engineering report containing:

Selected MCU, package, and component list.
Pin-assignment table.
Power architecture and regulator design.
Solenoid-driver topology and assumed maximum valve current.
Soil-probe excitation and ADC measurement strategy.
I²C device addresses and bus configuration.
PCB dimensions and layer arrangement.
Component-placement rationale.
Routing strategy for high-current and sensitive analog paths.
ERC results.
DRC results.
Unrouted-net count.
Unresolved issues and assumptions.
Native Synth project location.
KiCad export locations.
Manufacturing artifact locations, if generated.

Clearly distinguish between:

Implemented in the design.
Verified by automated checks.
Verified by inspection or calculation.
Not verified on physical hardware.
Blocked or unresolved.

Do not claim that a physical prototype has been tested.

Connector edge rule (mandatory, applies to every board)

Every external connector (JST, USB-C, screw terminal, barrel jack, or any connector that a cable or a case meets) must sit flush with the PCB edge. Its courtyard must touch the board outline on the edge its mating face points to, so that nothing overhangs the board and nothing blocks the cable. Only pin headers may sit inboard of the edge.

Give every such connector `placement_hint { edge: <left|right|top|bottom> priority: hard }` in the Synth source. Choose each connector's edge before routing, and make it the edge the connector really sits on: never hint one edge and then pin the connector on another.

If you pin mounting holes, put each hole centre at least 3.5 mm inside the plane of the nearest connector face. A hole closer to the edge than a connector face holds the board outline out and leaves the connector short of flush.

After every placement, read the placement review's warnings. A warning that a connector sits N mm inside an edge must be resolved by adding or correcting its hard `edge:` hint, or by moving the part. The final report must give each external connector's measured gap between its courtyard and the board edge, which must be 0.0 mm, or state exactly why it is not.

PCB layout quality rules (mandatory, applies to every board)

Keep the full functionality, the component set, the connectors and the mounting holes. Add no decorative text: no board name, no revision text, no functional headings, no logos. Silkscreen carries only what identifies parts, connectors and test points. Four rules, in this order: compactness (no unnecessary gaps between related parts), grouping (the parts of one circuit sit together), consistency (repeated parts, connectors and identical circuits are aligned and oriented alike), functionality (never trade electrical performance, manufacturability or serviceability for looks). Every gap that remains must have an electrical, mechanical or manufacturing reason.

Work by role, not by name. Decide each circuit's role from its connectivity, then place by these rules:
1. Support parts hug the part they serve. A decoupling capacitor sits within a few millimetres of the supply pin it decouples, a crystal and its load capacitors beside the clock pins, a reset or boot network beside the pin it drives, pull-ups and dividers beside the node they set, a flyback or clamp diode beside the switch it protects. Do not leave a support part standing alone in open board area. After every placement, read the placement review: when it says a support part is too far from a pin and names a hint such as `near: U2.vcc`, write exactly that hard `near` hint for that part in the source and place again, and repeat until the warnings clear or you can say why one cannot. The review lists the worst eight; after you fix them, place again to see the next ones.
2. The power entry owns a corner or an end. The input connector, its protection, the reverse-polarity part and the regulator form one contiguous block; each regulator has its inductor and its input and output capacitors touching it, laid out as its guidance asks, with a tight switching loop. The block must not crowd the processor or the sensitive inputs.
3. The main processor sits near the middle of the board unless routing says otherwise, with its decoupling in a tight ring and its programming or debug header, reset button and boot parts close to it and reachable. Memory and other parts wired mainly to it sit directly adjacent.
4. A peripheral circuit sits beside the connector it serves. When a driver, interface IC or protection network is wired mainly to one connector, place the whole circuit directly beside that connector, on that connector's edge, with its resistors and capacitors around it.
5. Identical circuits are placed alike. When two or more circuits use the same part types and the same connectivity (several output drivers, several identical sensor interfaces), give them the same internal layout, the same orientation, and the same pitch along the edge, so the copies read as copies. Align their connectors on one baseline with even spacing. After every placement, read the placement review: when it says a part of one copy is laid out differently from the same part in the first copy and gives coordinates, pin that part at exactly those coordinates (and rotation) in your placement sidecar and place again, until the copies match or you can say why they cannot.
6. External connectors sit flush on the board edge, the edge their mating face points to, with even spacing between connectors on the same edge. Pin headers may sit a few millimetres inboard. Leave plug and cable clearance.
7. Test points sit with the circuit they measure, within a few millimetres of a part on their net; test points for one interface (debug, programming) form one aligned row by that interface. Keep them probe-accessible and out of the processor's escape area. Ground test points are easy to reach.
8. Orientation and alignment. Put like parts (resistors, capacitors, diodes) on shared rows or columns inside a block, give equivalent transistors matching orientation, align IC edges and connector centres on common horizontal and vertical lines, and keep pin-1 marks identifiable. Do not rotate arbitrarily and do not force one orientation where it worsens the electrical layout.
9. Indicators. Status LEDs with their resistors form one row at an edge, near the part whose state they show.
10. Silkscreen. Each reference designator sits beside its part, in the same orientation and size for the same kind of part, clear of pads, copper and other bodies. Where Synth cannot place text, say so in the report instead of working around it.
11. Routing. Route after placement is final: short and direct, 45-degree bends, widths for the current, return paths preserved, noisy and sensitive nets apart, little congestion near the processor, continuous ground pour, no isolated copper islands.
12. Outline and holes. Keep the mounting holes with clearance and no part under the hardware. Keep part and copper setbacks from the edge. Shrink the outline only when the placement truly allows it.
13. Validation and report. Run ERC and DRC, confirm every schematic connection and the component count are unchanged, review the board in 2D and 3D, and report measured numbers: each decoupling capacitor's distance to its pin, each connector's gap to the board edge, each test point's distance to its circuit, and for every group of identical circuits whether their layouts match. Say plainly what you could not do.

14. Release exports only. A qualification board is a release export. Never set allow_incomplete=true on synth_export or any other tool: it switches off the release gate (footprint guarantee, routing and signal-integrity checks) and labels the board a draft. If the export is blocked, read the finding and fix its cause with the Synth tools (for a part whose footprint is not in the KiCad library, use synth_resolve_footprints and name one KiCad really ships; for a routing finding, change the placement or the part). Only if you truly cannot clear a blocker, stop and report that blocker exactly, instead of exporting a draft.

21. Final execution instruction

Begin by inspecting Synth MCP and the available component libraries.

Then implement the actual design in the prescribed order.

Prioritize electrical correctness, safe valve switching, reliable power distribution, corrosion-conscious soil measurement, and a manufacturable PCB layout.

Do not stop after producing a conceptual schematic if Synth supports PCB creation and routing.

If a critical requirement cannot be implemented or verified, explain the exact blocker and finish all remaining supported work.

The final deliverable must be the actual irrigation_controller design, exported to KiCad, with routing and validation results documented.
~~~~

## fill-pass-prompt

Source in history: `qualification/class-a/irrigation-controller/agent-run/fill-pass-prompt.md`

~~~~text
Irrigation Controller PCB — Fill the Empty Space with Rail-Only Parts, in Small Batches (Synth MCP)

1. State of the board (README.md describes this board exactly; coordinates are board millimetres, the same as the sidecar; the part names C_N*, R_N*, D_N*, TP_N* exist from the previous run)

Your workspace holds the irrigation controller after a previous run added 26 parts (README.md and REPORT_previous_run.md describe it). It exports released (43 of 43 nets, 0 DRC errors) but it is NOT yet acceptable: (1) its outline is 75.1 x 65.9 mm, 0.5 mm above the cap of 75.1 x 65.4 mm, because one added passive stands inside the rim of a connector edge and holds the outline out, leaving the three connectors on that edge 0.68 mm inside the board edge instead of 0.15 mm; (2) many added capacitors are 15 to 57 mm from the part they decouple, and a TVS added for a connector's signal is about 45 mm from that connector; (3) the grouping and alignment moves were only partly done. Your job, in this order: first repair the outline and connector gaps, then finish the grouping and alignment, then re-place the added parts where they serve a nearby part, then add more parts only if the board stays released and the outline stays at or below the cap. The Synth build you have also moves a via the engine left in a pad off it and routes a rejected board again in the engine's other net orders.

The only tools are the Synth MCP and Read, Write and Edit. Do not invent tool results.

2. Hard rules (unchanged)

- Exported outline stays at or below 75.1 x 65.4 mm, and every flush connector stays within 0.3 mm of its edge (check both on your own export, not only on the tool's report). Keep every existing part, net and function; existing parts may move only as step A below allows (grouping and alignment inside the outline); connectors stay on their edges exactly as they are. Connectors stay flush with the edge; the four mounting holes stay at the corners.
- Every added part is wired to a real function; no floating or decorative parts, no text.
- Add parts through the Synth tools (edit the .synth, use the registry before authoring, place with the placement tools at coordinates in the empty areas). Never edit exported KiCad files.
- Release exports only: never pass allow_incomplete=true. A batch that does not release is not carried forward.
- Prefer SMD parts with ordinary courtyards: 0402, 0603 and 0805 passives, SOD-123 and SOD-323 diodes and TVS, SOT-23 transistors and MOSFETs, SOT-23-6 and SOIC-8 ICs. Avoid radial electrolytics and TO-92 (their circular courtyard is measured badly).

2b. Step A before adding anything: group and align the existing parts, then fill (new in this run)

Do the regrouping first, in small steps, each re-routed and exported (fresh export_step_<n>/ folder) and kept only if it releases (release_ready, route_complete, 0 DRC errors, every connector still within 0.3 mm of its edge, outline unchanged at 75.1 x 65.4 mm). Move parts by pinning coordinates and hints in the sidecar with the Synth placement tools; never edit exported files. Rules by role, never by reference designator:
- Power section: the regulator's inductor, input and output capacitors, bootstrap, soft-start, compensation and feedback-divider parts form one compact block at the regulator; the feedback divider sits right at its pin; the fuse is in the input path; protection parts sit at the input connector; input test points stay accessible.
- Processor: it is the visual centre. Each decoupling capacitor sits at its supply pin, the crystal and its load capacitors at the clock pins, and the reset button, its pull-up and its capacitor in one group by the reset pin. Keep the immediate surroundings uncluttered.
- Identical circuits (same part ids, same connectivity pattern: the valve drivers, the sensor interfaces) get the same relative placement and orientation. Copy the first copy's offsets to the others; a driver circuit sits beside its connector (switch, gate resistor, pull-down, flyback diode).
- Each sensor or indicator circuit sits in one compact group by its connector or its IC; like parts (resistors, capacitors, LEDs, diodes) share rows or columns and orientations; indicator LEDs with their resistors form one row; no isolated passive more than 4 mm from a neighbour that shares its net.
- Keep sensitive analog signals apart from switching and valve-drive nets; keep the debug header and test points reachable; keep the four mounting holes clear of parts and copper.
- Do not rotate parts only for symmetry; do not reduce clearances; do not add text or headings.
The placement review (synth_place_with_hints, run_routing=false) now carries five advice actions: `move_off_edge_rim` (a part within the rim of a connector edge holds the outline out: move it inward by the amount given; do this first), and four more you should act on in step A, each with the coordinates or part to use: `match_repeated_circuit` (a part of an identical circuit that is laid out differently from the first copy: pin it at the given coordinates and turn), `align_like_parts` (a like part that misses the row or column its neighbours share: set the named coordinate), `place_beside_neighbour` (a small part far from everything it is wired to: put it beside the named part) and the support-part and test-point advice (a hard `near: part.pin` hint). Apply them in small groups, re-review, export, and keep a group only if the board still releases.
After each step run the placement review and clear its warnings (support-part distance, identical-circuit mismatch, connector edge). Then continue with the fill below in the space the regrouping leaves. At the end report which of these rules each part group meets and which it does not, with distances measured from the exported board.

3. What changed since the earlier runs

The earlier runs failed on a via placed inside a pad (U2.30, U3.9, J_I2C.1, U5.7): the Synth build they used placed ground return vias on ground pads. That is fixed in the build you have now, which also routes a rejected board again in the engine's other net orders. So the earlier failures should not recur, and batches that failed before can be tried again. The MCU is U2 in this design; U5 is the EEPROM.

Still prefer simple connections: parts on +12V_VALVE, +3V3, ground, connector pins, and each other, placed right next to the rail or pin they use. The +3V3 net near U2 is tight (it was routed down to 0.127 mm tracks), so add +3V3 parts last and in small groups. Parts the earlier runs added (C_D1 to C_D6, C_E1 to C_E8, R_BL3, R_BL12, R_E1 to R_E3 and others) are already there; the sidecar also still holds entries for parts that are no longer in the .synth, which you may remove.

4. The empty space (from the current export; free areas keep 1 mm from every courtyard and from the board edge)

outline 2.0,1.6 to 78.2,69.5  (75.1 x 65.4 mm); free areas keep 1.0 mm from every courtyard and 1.0 mm from the edge
void 1: x 63.0 to 76.5, y 32.5 to 56.5  (13.5 x 24.0 mm = 324 mm2)
void 2: x 8.1 to 24.6, y 44.5 to 54.0  (16.5 x 9.5 mm = 157 mm2)
void 3: x 48.0 to 69.0, y 60.5 to 66.5  (21.0 x 6.0 mm = 126 mm2)
3 voids, 607 mm2 free at >= 4 mm on a side

Keep about 3 mm between neighbouring courtyards and leave channels for tracks. Parts 2 to 4 mm apart is the goal. Coordinates are board millimetres, the same as the sidecar.

5. What to add (rail-only and connector-only parts first; each one needs a stated function)

- Decoupling: 100 nF 0603 and 1 uF or 10 uF 0805 capacitors along the +3V3 rail, the +12V_VALVE rail and the power input, each next to a rail track or an IC supply pin, grouped in the voids nearest those tracks.
- Bleeder or discharge resistors (10 k to 100 k) across the bulk capacitors on +12V_VALVE and +3V3.
- Rail-present LEDs with series resistors in one row: +12V_VALVE, +3V3, and each valve output's rail (use rail and ground only).
- Protection on external connector pins: a TVS or ESD diode (SOD-323 or SOD-123, 5 V for signal pins, 15 V to 24 V for the 12 V side) from each unprotected connector pin to ground, next to the connector. A flyback or clamp diode across each valve output if missing.
- Reverse-polarity or surge helpers on the power input: a second TVS, a varistor footprint with a part that exists in the registry, a ferrite bead in series with the 3V3 sensor supply if a clean split exists.
- More test points on rails and on connector pins, each next to its net.
- A snubber (resistor plus capacitor in series) across each valve coil's connector pins.

6. Procedure (small batches, bisect on failure)

1. Read the .synth, sidecar, REPORT.md and this brief. Export the unchanged board into export_start/ and confirm it releases.
2. Add parts in batches of 6 to 10 related parts. After each batch: run the placement review (synth_place_with_hints, run_routing=false) and clear the warnings for the new parts; then export into a fresh export_batch_<n>/ folder.
3. If a batch releases, keep it and continue. If it does not release, split it in half, find which parts break the release, drop those and keep the rest. Never carry a failing batch forward.
4. Do not add parts until the outline and connector gaps are repaired. After that, add parts only as decoupling next to the pin they serve or protection next to the connector they guard; no part counts that is more than 6 mm from the part it serves. Do not stop after one or two failed batches: split, drop the culprits and keep going until you have tried every kind of part listed above.
5. Finish with the best released state exported into a fresh export/ folder, with the .synth and sidecar beside it. Do not call synth_compact_layout. The empty-space list in section 4 is measured BEFORE step A; remeasure the free space from your own export after step A and use that.

7. Report (REPORT6.md)

A table of every part added in this run: reference, value, MPN or "generic", function, nets, position, batch. The parts you dropped and why. The final DRC result, release_ready, route_complete, number of nets, and the placement review result. State plainly anything you could not do or check.

Begin by reading the files and exporting the unchanged board.
~~~~

## review-fixes/prompt-1-review-fixes

Source in history: `qualification/class-a/irrigation-controller/agent-run/review-fixes/prompt-1-review-fixes.md`

~~~~text
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
~~~~

## review-fixes/prompt-2-input-mosfet

Source in history: `qualification/class-a/irrigation-controller/agent-run/review-fixes/prompt-2-input-mosfet.md`

~~~~text
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
~~~~

## review-fixes/prompt-3-fill

Source in history: `qualification/class-a/irrigation-controller/agent-run/review-fixes/prompt-3-fill.md`

~~~~text
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
~~~~

## final-blockers/prompt

Source in history: `qualification/class-a/irrigation-controller/agent-run/final-blockers/prompt.md`

~~~~text
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
~~~~
