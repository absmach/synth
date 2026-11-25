# Bring-up plan

Not run on hardware. Nothing was powered or tested (README.md). Expected values below come from the design files; any value marked "not verified" has no recorded source. Stop at the first failed step.

1. Visual and polarity: check D3 (green), D4/D7 (red) cathode orientation against the schematic (cathode to GND for D3, to TXD0/TXD1 for D4/D7); check U1, U2, U3, D1, D2, D5, D6 orientation. Expected: as in PIN_MAPPING.md.
2. Continuity, unpowered, no jumper: VBUS to GND must not be short; +3V3 to GND must not be short; USB_DP and USB_DN each not shorted to GND. Resistance values: not verified.
3. Power from a USB-C host with no jumper on J_VIO. Expected: VBUS about 5 V (design value 5 V); +3V3 about 3.3 V (LP5907-3.3); VIO unpowered; VIO LED off; the host does not enumerate the bridge (bridge held in reset, README.md). Tolerance bands: not verified.
4. Fit the jumper on J_VIO 1-2. Expected: VIO about 3.3 V, green D3 lit, host enumerates the CH342F as two serial ports. Enumeration descriptors: not verified.
5. Fit the jumper on J_VIO 2-3 (power off first). Expected: VIO about 5 V (VBUS), D3 lit, enumeration as in step 4.
6. CC check: CC1 and CC2 each read about 5.1 kohm to GND (R1, R2) with the host disconnected. Expected host behaviour: sources 5 V on VBUS.
7. UART1 loopback: with VIO at 3.3 V jumper J2 pin 2 (TXD1) to pin 3 (RXD1) and send data from the host. Expected: data echoed; D7 flickers during transmit (lit on low bits, dark at idle). Voltage levels: not verified. R5 is 1k in series.
8. Flow lines: J3 DTR1/RTS1 toggle from the host serial control; expected levels at the VIO rail. CTS1 floats if J3 is unplugged (README.md).
9. UART0 / RS-485: connect a second RS-485 adapter to J4 (A to A, B to B, GND to GND). Send from UART0. Expected: data received on the other adapter; D4 flickers on transmit; R_TNOW 4.7k to GND puts UART0 in the CH342 half-duplex mode, with DE and /RE driven by TNOW0 (auto-direction, per usb_uart.synth notes). Differential voltage and failsafe behaviour: not verified.
10. Termination: with the bus idle, measure A-B resistance. Expected: about 120 ohm (R_TERM always fitted) plus the other adapter's load.
11. Soak: leave running with data for a time chosen by the tester; watch U2 and U3 temperature. Limits: not verified.

Open concerns to watch (README.md): a powered target on J2/J3 with VIO off can exceed VIO+0.5 V on U1 inputs (R5/R6 limit current to about 2.6 mA at 3.3 V, 4.3 mA at 5 V); a target holding DTR1 low at power-up changes UART1 to half-duplex mode.
