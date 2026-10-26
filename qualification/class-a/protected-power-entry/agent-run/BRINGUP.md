# Bring-up plan — protected_power_entry rev A

**There is no firmware.** The board has no programmable device (stated exception: no MCU, no USB-C programming path, no SWD).
This file is the bring-up procedure alone. **Nothing below has been run.** No prototype exists; no hardware operation is claimed.
Lab supply: current-limited bench supply with a 12 V setting, a DMM, a scope, and resistive loads.

Checks run in this session (design-time only): ERC, placement review, routing, DRC, BOM. See `VERIFICATION_REPORT.md`.

## Before power
1. Check part orientation against the pick-and-place data: Q1 (pin 1 = gate), D1 (cathode bar), D9/D10 (cathode), D8 (cathode), all LEDs (cathode marker), C1/C10/C17 (+ side), U1 pin 1, U3 pin 1, U2 tab.
2. Unpowered shorts: with the board off, measure resistance GND to each of VIN_12V_RAW (TP1), VIN_12V_FUSED (TP2), +12V_PROT (TP3), +5V (TP4), +5V_SW (TP5), +3V3 (TP6), BUCK_SW (TP7). Expect: TP1 and TP2 are open in the supply direction (D2/R19/D8 and D7/R24 give several kilohm); TP3 gives about 12 k (SENSE_12V divider) to GND; TP4 about 10 k to 20 k (dividers); TP6 about 5 k (divider). Anything under 100 ohm is a short.
3. Check F1, F2, F3 continuity (under 1 ohm).

## First power
4. Set the supply to 0 V, current limit 0.1 A. Raise slowly to 5 V while watching the current. Below the buck start threshold (about 4.8 V) the buck is off; expect D2 (white), D3 (blue) on, D6/D4 off.
5. Raise to 12 V, limit 0.3 A, no loads. Expect +12V_PROT = input - 0.1 V (TP3), +5V = 5.0 V +/-3 % (TP4), +3V3 = 3.3 V (TP6), BUCK_SW switching about 340 kHz (TP7, scope), +5V_SW 0 V (TP5). D2, D3, D4, D6 on; D5 off; D7 off.
6. Q1 gate: measure Vgs (PFET_GATE to +12V_PROT) at 9 V, 12 V and 15 V. Expect about -4.8 to -5.4 V. If it exceeds -8 V stop.

## Reverse polarity (current-limited)
7. Power off. Reverse the leads on J1 with the limit at 0.1 A and the supply at 5 V first, then 12 V. Expect: TP3 stays near 0 V, no current beyond a few mA (the fault LED path shows nothing), D2 off (D8 blocks it), no heating. Return to normal polarity. Do not swap polarity while the rail is still charged (wait 5 s).

## Enable and soft start
8. LOAD_EN default: with J2 pin 3 open, +5V_SW must be 0 V and TP8 0 V.
9. Scope TP5 and TP8. Drive J2 pin 3 high from a 3.3 V source (J2 pin 1 through a jumper wire, or a bench 3.3 V): TP5 should ramp over about milliseconds (C13 2.2 nF on CT; rise time not computed) to +5 V; D5 on. Measure inrush with a current probe on J4 with a 100 uF + 10 ohm load; expect under 2 A. Drive pin 3 low: TP5 falls (QOD discharge, not verified).
10. Check that 12 V on J2 pin 3 does not exceed 5.4 V at TP8 (use a 100 ohm series resistor in the test lead and a current-limited 12 V supply at 10 mA).

## Rails under load
11. +5V: 0.5 A, 1.0 A, 1.5 A on J3 (resistor load 10 ohm / 5 ohm / 3.3 ohm). Check 5.0 V +/-3 %, ripple under 50 mV p-p at TP4, F1 drop (TP1-TP2) under 0.3 V, F2 drop, buck case temperature. Then a 0.5 A to 1.5 A load step; check for ringing (loop compensation is unverified).
12. +5V_SW: repeat on J4 with LOAD_EN high.
13. +3V3: 0.1 A, 0.2 A, 0.35 A on J5. Check 3.3 V, U2 case temperature (0.66 W at 0.35 A); stop above 90 C at the tab. Do not exceed 0.35 A.
14. Total: J3 + J4 + J5 together at 1.5 A total at 9 V input; measure input current (about 1.0 A); F1 must not trip over 30 minutes at 25 C ambient.

## Header
15. SENSE_12V (J2 pin 5) at 12 V: about 2.0 V; SENSE_5V (pin 6): 2.5 V; SENSE_3V3 (pin 7): 1.65 V. J2 pin 1 (3V3_REF) 3.3 V unloaded. No pin above 3.6 V at any supply voltage up to 15 V.

## Protection behaviour (no destructive tests)
16. Over-current: put an electronic load on J3 and ramp slowly to 3 A with the supply limit at 3 A; F2 should trip (watch the temperature); D7 should light when F1 trips (F1 trips only with a fault on +12V_PROT, such as a 0.5 ohm load on the 12 V rail). Do **not** short outputs on the bench supply at full current; use a current-limited supply.
17. TVS: do not surge-test. Verify D1 by diode-test across TP3-GND (should read a high impedance at 12 V and not conduct below 16 V).
18. Short-circuit: with the supply limited to 0.5 A, momentarily short J5 and check U2 thermal shutdown cycling; do not hold more than a few seconds.

Record every reading against the expected value, and write down any value outside +/-10 % before proceeding.
