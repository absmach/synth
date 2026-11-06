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
