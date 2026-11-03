# Bring-up: BLDC stack (SYN-408)

**Not run on hardware.** These are the steps and expected measurements for a
first article. Every expected value is a calculation or a datasheet figure, not
a measurement.

## Before power

1. Inspect both boards for solder bridges, especially the 0.4 mm QFN/LQFP pins,
   the TDSON-8 pads and the 2 x 10 connector.
2. Check the 24 V input for a short to ground with a DMM. Expect > 10 kOhm
   (the divider and the bulk capacitors charge).
3. Check the 3.3 V and 12 V rails for a short. Expect > 100 Ohm.
4. Do not fit the motor. Leave J_MOTOR open.
5. Set the bench supply to 24 V, current limit 0.5 A.
6. Fit a 2 A fuse in the bench lead; the board has no fuse.
7. Check that GATE_EN is low (Q_GE off) before the control board is connected.
   If the control board is absent, the 10 k pulldown on GATE_EN keeps the gate
   drivers off.

## Power-up sequence

1. Apply 24 V to J_PWR. Measure VBUS after Q_REV: expect 23.5-24 V (the P-FET
   drop is I x 20 mOhm; at no load it is essentially 24 V).
2. Measure +12V at the LMR38010 output: expect 11.8-12.2 V. If it is 0 V, check
   the EN divider (EN is tied to VIN) and the 33 k RT resistor.
3. Measure +12V_GD with GATE_EN low: expect 0 V (Q_GE off). Drive GATE_EN high
   from the control board or a 3.3 V source: expect +12V_GD within 0.2 V of
   +12V.
4. Connect the control board. Measure +3V3 at U_LDO: expect 3.23-3.37 V
   (AMS1117-3.3 tolerance). Measure the AMS1117 tab temperature after 5 min at
   no load; it should be near ambient. At 84 mA it will be warm; the estimate is
   about 0.73 W and a 107 C junction in the worst case, so a thermal camera or
   thermocouple is required.
5. Measure the 12 V sense divider at PB1: expect 12 x 22/122 = 2.16 V.
6. Measure VBUS_SENSE at J_B2B pin 10: expect 24 x 13/113 = 2.76 V.
7. Measure the Hall pull-ups: +3V3 on each Hall connector pin 1, 0 V on pin 3.

## Functional tests (with a current-limited supply and no motor)

1. Program the STM32 over SWD. The boot button shares PA14; do not press it
   while the debugger is connected, or use the 1 k series path.
2. Confirm the firmware clocks from the internal HSI16; there is no crystal
   on this board.
3. Drive PWM_UH/PWM_UL at 20 kHz with 1 us dead time. Measure HO/LO at the
   IR2101 with a scope: expect 0/12 V levels and no overlap.
4. Confirm the boot button pulls PA14 high through 1 k and the MCU enters the
   system bootloader on reset if held.
5. Toggle the status LEDs from PB2/PB3: expect about 2.55 mA.
6. With a Hall sensor or a 3.3 V/0 V source on each Hall input, confirm the
   RC filter output at PA4/PA5/PA6: 0-3.3 V, fc = 1.6 kHz.
7. With a 5 V supply and a 120 Ohm termination, check RS-485 A/B idle levels
   and that DE (PB8) toggles. The THVD1450 is 3.3 V here.
8. Inject 50 mV across one shunt from a floating source (or 10 A through the
   shunt) and measure the op-amp output: expect 2.0 V at 10 A. Check the
   overcurrent comparator: at about 8.25 A, FAULT_N should go low.
9. Check the fault latch: FAULT_N is not latched; it follows the comparator.
   The firmware must latch it.

## Motor test (only after the above)

1. Start with a low voltage (12 V) and a current limit of 2 A.
2. Run open-loop commutation at a low duty cycle and check the three phase
   currents with a current probe. Expect the shunt waveform to match the
   firmware's sampling instant.
3. Increase to 24 V and 10 A only with a thermal camera on the MOSFETs and the
   shunts. The calculated loss is about 4.5 W before switching; the junction
   temperatures are not known.
4. Check the phase-node ringing at each switching edge. There is no snubber and
   no loop-inductance measurement.
5. Check the 24 V input for overshoot at load dump; the SMBJ30A clamps at
   48.4 V but its energy rating was not checked against the motor cable.

## What is not covered

- Dead time and shoot-through are firmware properties; no test here proves them.
- The current-sense offset, gain error and bandwidth are not calibrated.
- The 12 V buck loop stability is not measured.
- The connector and stack height are not checked.
- No EMC, surge or safety test is defined.
