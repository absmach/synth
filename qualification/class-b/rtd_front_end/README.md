# RTD Analog Front End Board

2-layer RTD analog front end for Class B qualification.

## Features
- LM358 dual opamp: buffer + non-inverting gain stage (11x)
- RC low-pass filter (fc ~160 Hz)
- Voltage divider for PT100/PT1000 RTD measurement
- 0-3.3V analog output to host ADC
- 3.3V power input

## Note
This is a simplified 2-layer design. The parent Class-B requirement calls for
ADS124S08, REF5025, RP2350, 4-wire RTD, and AGND/DGND single-point tie.
These parts are not available in the Synth registry.

## Files
- `rtd_front_end.synth` — SynthSpec source
- `kicad/` — KiCad project files (PCB, schematic, BOM, PnP)
- `images/` — schematic and PCB screenshots
- `reports/` — DRC and ERC reports
- `qualification.json` — qualification gate status
- `SHA256SUMS` — file checksums
