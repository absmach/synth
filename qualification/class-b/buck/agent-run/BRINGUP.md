# Bring-up: buck rev A

**Not run on hardware.** The board was never fabricated, powered or measured (README, QUALIFICATION B18). This is a written test plan. Every "expected" value below is a figure from the board's README/QUALIFICATION (datasheet-derived, by hand calculation) or the part files; none was observed. There is no firmware: the board is a power stage with no programmable device.

Equipment assumed (not part of the files): current-limited bench supply 0-30 V, DMM, oscilloscope, electronic load. Do not exceed the 33 V PTC Vmax or the 50 V capacitor ratings on the input.

## 0. Before power

1. Inspect against `ASSEMBLY.md`: D1/D2 cathode toward VIN, D3/D4 cathode toward GND/PG side, C1 + on pin 1 side, U1 pin 1, exposed pad soldered.
2. With the board unpowered, DMM resistance VIN_RAW to GND, +3V3 to GND, SW to GND: expect no short. Expected values beyond "not a short": not verified.
3. Confirm J1 pin 1 = VIN_RAW (+), pin 2 = GND; J2 pin 1 = +3V3, pin 3 = PG.

## 1. Reverse polarity

4. Supply 5 V current limit 100 mA, reversed on J1. Expect: no current into the board beyond D1 leakage (SS36 IR 0.5 mA at 25 C max, 60 V rating). Pass if VIN stays near 0 V and the supply current is below 1 mA.

## 2. UVLO and start-up (no load)

5. Raise supply from 0 V, limit 0.3 A. Expect +3V3 to stay off below about 18.4 V (typ; range 16.2 to 20.6 V).
6. At 24 V expect +3V3 = 3.315 V typ (3.26 to 3.36 V). D3 lit (about 1.9 to 3.0 mA).
7. Lower the supply: expect shutdown near 16.2 V (typ; range 14.0 to 17.9 V). EN at 28 V in would be 1.90 V (calculation).
8. At 24 V measure EN: about 24 x 100k / 1.47M = 1.63 V (arithmetic).

## 3. Power good

9. At start-up PG is low for about 4 ms after EN asserts, then high (3.3 V, via R_PG). Scope PG against +3V3.
10. D4 "PG FAULT": expect dark while power is good, lit (about 2.55 mA, PG near 0.2-0.4 V) while PG is low (during start-up or a fault).
11. Thresholds: rising 110/112/114 % and falling 90/92/94 % of VFB, 40 us glitch filter (datasheet figures per README). To test, trim load or inject at FB; pass criteria beyond these figures: not verified.

## 4. Switching and output

12. Scope SW to GND: expect about 800 kHz (R_RT 33k); PFM at light load, spread spectrum on. SW ringing amplitude limit: not verified; never exceed 85 V (abs max).
13. Load 0.5 A, then 1.0 A: +3V3 within 3.26 to 3.36 V. Output ripple limit: not verified.
14. Inductor ripple at 26.4 V in, 1 A: about 0.53 A pp, peak about 1.27 A (calculation) against Isat 5.7 A.
15. Input current at 24 V, 1 A out: about 0.16 A (assumes 85 % efficiency); at 16.2 V about 0.24 A. Measured efficiency: not verified.

## 5. Thermal

16. Run 1 A for 30 minutes at 24 V. Expected loss about 0.58 W (assumption). Record U1 case temperature. Pass limit: not verified (no RthetaJA valid for design).

## 6. Input protection (do only with a suitable current-limited setup)

17. At 24 V and 1 A, F1 temperature rise; hold current 0.63 A at 70 C against about 0.16 A draw. Pass limit beyond that: not verified.
18. TVS and surge: not tested; no surge generator procedure is recorded. Do not apply voltage above the PTC Vmax of 33 V without a procedure.

## 7. Results record

Fill after the run: date, tester, board serial, each step pass/fail, measured values. None recorded. Not run on hardware.
