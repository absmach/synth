# Power budget

Only figures present in `../usb_uart.synth`, `registry/parts/**` and `../README.md`. Total consumption: not verified (no per-block supply current is recorded; no measurement).

| Rail | Declared | Source | Loads | Source limit | Margin |
| --- | --- | --- | --- | --- | --- |
| VBUS | 5 V | J1 (USB-C host through CC1/CC2 5.1k Rd) | U2 VIN/EN, U1 VDD5 and VBUS, D1 VCC, D2 TVS, J_VIO.3 (and VIO when jumper 2-3) | J1 part file max_current_ma 3000 (connector rating); host current actually offered: not verified | not verified |
| +3V3 | 3.3 V | U2 LP5907MFX-3.3 | J_VIO.1 only (reaches VIO when jumper 1-2); C3 | U2 part file max_current_ma 250; max_power_mw 200; thermal 250 C/W; input 2.2-5.5 V | not verified: VIO load total not recorded |
| VIO | 3.3 V or 5 V by jumper | J_VIO | U1 VIO (C6), U3 VCC (C7), LED resistors R3/R4/R7, R8 pull-up | none when no jumper fitted (bridge in reset) | see below |
| V3 | internal | U1 | C5 only (no external load) | not verified | n/a |

## Part-file limits
- U1 CH342F: operating 1.7-5.5 V, max_current_ma 15; VIO and VDD5 pins voltage_max 6.0 V; VBUS pin 6.5 V.
- U3 THVD1450D: operating 3.0-5.5 V, max_current_ma 3.
- D1: max_voltage 5.5 V. D2/D5/D6: max_voltage 6.0 V.
- LEDs: D3 max_current 30 mA, D4/D7 25 mA (part files).

## LED currents (README.md estimates)
D4 and D7 each draw about 3 mA at 3.3 V and 6.6 mA at 5 V from the CH342 TXD pin (estimate from a 1.9 V forward drop; CH342 output drive at 5 V not checked). D3 current: not stated in the files; not verified.

## Notes from README.md
- No VBUS bulk cap, fuse, reverse or inrush protection.
- LDO decoupling: 2.2 uF in and out; part file requires 1 uF.
- With a powered target driving RXD1/CTS1 while VIO is off, R5/R6 limit the current to about 2.6 mA (3.3 V target) or 4.3 mA (5 V); the CH342F datasheet gives no clamp rating.
- E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio) is a validate warning, not a computed margin.
