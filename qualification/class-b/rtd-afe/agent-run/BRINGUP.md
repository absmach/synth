# BRINGUP: rtd-afe

NOT RUN ON HARDWARE. The board is unfabricated; QUALIFICATION.md: "not fabricated, not powered and not measured". These are written test steps with expected values taken from the sources. Expected values marked "not verified" have no recorded source. There are no test points: probe component pads, the J2/J3 headers or the USB-C connector. No firmware exists in the repository, so steps 7 onward need firmware written first.

Equipment: bench supply or USB host with current limit, DMM, scope, a 100 ohm precision resistor or a PT100 for J2, SWD probe.

1. Visual and continuity, unpowered.
   - Check J1 VBUS pins (A4/B4/A9/B9) to GND for a short. Expected: no short (open or capacitor charge only).
   - Check +3V3, +3V3A, DVDD_1V1 to GND. Expected: no short.
   - Check the C_DV.2 via-in-pad was filled and capped. Expected: flat pad.
2. USB power-up, current limited.
   - Apply 5 V at VBUS (or plug into a host through a current-limited source). Expected VBUS 5 V (`power "VBUS" 5v`). Idle current: not verified; the `.synth` note says "a few tens of mA".
   - U5 EN is tied to VBUS, so it starts at once.
3. +3V3 rail.
   - Measure U5 pin 5 / C_L2.p1. Expected 3.3 V (LP5907-3.3 output; tolerance not verified). Ripple limit: not verified.
   - U5 temperature after 10 min: registry max_power 200 mW; derived dissipation 1.7 V x I. Limit: not verified.
4. Analog supplies.
   - +3V3A at U3 pin 2 and U1 pin 26. Expected about 3.3 V minus the L2 drop (L2 DC resistance not recorded: not verified).
   - +3V3_RP_A at U2 pins 44/46. Expected about 3.3 V minus the drop across R_AV 33R.
5. Reference.
   - Measure VREF at U3 pin 6 / C_RV.p1 / U1 pin 30. Expected 2.5 V (REF5025). Initial accuracy and noise: not verified (grade tolerance not recorded).
6. MCU core and boot.
   - Measure DVDD_1V1 (C_DVB.p1). Expected 1.1 V (net name and `.synth` note: "1.1 V DVDD net"). Tolerance: not verified.
   - Check Y1 at XIN: 12 MHz oscillation after firmware starts the crystal. Waveform amplitude: not verified.
   - Check RUN (R_RUN 10k to +3V3). Expected high, 3.3 V.
   - Connect an SWD probe to J3 (pin 1 SWCLK, 2 SWDIO, 3 GND). Expected: the probe sees the RP2350. Note J3 has no reset or Vref pin.
   - USB enumeration with a bootloader or firmware. Expected: a full-speed device enumerates on D+/D- with CC1/CC2 5.1k pull-downs (host supplies 5 V). Descriptors do not exist: not verified.
7. ADC bring-up (needs firmware).
   - Hold ADC_RESET_N high (R_RST pull-up, U2 GPIO6). Read the ADS124S08 ID register over SPI (SPI0: GPIO0 MISO, GPIO1 CS, GPIO2 SCK, GPIO3 MOSI). Expected value: not verified (register map is in the datasheet, SBAS660C, not copied here).
   - Verify SPI MISO/MOSI direction by this read; the `.synth` flags that the registry lists gp0/gp3 the other way round.
   - Confirm DRDY toggles on U2 GPIO4 after START (U2 GPIO5 / ADC_START). Data rate: firmware setting, not verified.
8. Excitation.
   - Enable IDAC1 at 1 mA on AIN0 and put a 100 ohm resistor across the force path (J2 pins 1 and 4, S+/S- linked to the same resistor). Expected drop across R_BIAS about 1.0 V (1 mA x 1k) and RTD common mode about 1.1 V at 100 ohm (README). Tolerance: not verified.
9. Measurement.
   - With 100 ohm on J2, read AIN1-AIN2 with the reference VREF. Expected ratio from 1 mA x 100 ohm = 100 mV differential; the reading tolerance, noise and drift are not verified (README: no accuracy measurement, no calibration).
   - Repeat with a PT100 at a known temperature. Expected resistance: not verified here (use the IEC 60751 table; not part of the sources).
10. Record: any step that fails stays open in `VERIFICATION_REPORT.md`; none of the above has been run.

Not covered by any step: ESD, surge, reverse-polarity, USB-C compliance, EMC, mechanical fit.
