# Pin mapping — protected_power_entry rev A

Pin numbers below are footprint pad numbers (verified against the KiCad footprint files for J1/J3-J5/J2 geometry
and against the registry/authored part files; datasheet agreement is [recalled], not read).
Pin 1 of each connector is the square pad / pin-1 marker in the KiCad footprint. **No silkscreen labels exist
on the board** (Synth gives no way to place text).

## J2 control header, 1x8, 2.54 mm (`PinHeader_1x08_P2.54mm_Vertical`, laid horizontal along the bottom edge)

| Header pos | Signal | Device pin (package pin) | Direction / drive | Voltage range | Function and restriction |
|---|---|---|---|---|---|
| 1 | 3V3_REF | U2 AMS1117 VOUT (pad 2) via R9 100 ohm | board -> host | 3.3 V +/-1 % | reference, 5 mA max (0.5 V drop); shorted 33 mA |
| 2 | GND | - | - | 0 V | beside 3V3_REF |
| 3 | LOAD_EN | U3 TPS22918 ON (pad 3) via R8 1 k | host -> board | 0-3.3 V normal; clamped at 5.1 V by D10 | high (>1.0 V [recalled]) = +5V_SW on; low or open = off (R7 100 k pull-down). Host high of 3.3 V reads 3.27 V at ON. A 12 V mistake draws 6.9 mA through R8 |
| 4 | GND | - | - | 0 V | between the enable and the sense group |
| 5 | SENSE_12V | +12V_PROT, 10 k / 2 k divider, R12 1 k, C19 100 nF | board -> host ADC | 0-2.67 V for 0-16 V (k = 1/6); clamped about 3.5 V by D11 | scaled +12V_PROT, not the raw input; source impedance 2.7 k |
| 6 | SENSE_5V | +5V, 10 k / 10 k, R15 1 k, C20 100 nF | board -> host ADC | 0-2.75 V for 0-5.5 V (k = 0.5); clamp D12 | scaled +5V; source impedance 6 k |
| 7 | SENSE_3V3 | +3V3, 10 k / 10 k, R18 1 k, C21 100 nF | board -> host ADC | 0-1.7 V (k = 0.5); clamp D13 | scaled +3V3; source impedance 6 k |
| 8 | GND | - | - | 0 V | last pin |

There is **no FAULT pin**: nothing on the board produces a fault or power-good signal (see `DESIGN_NOTES.md` 3.7).
Every pin is 3.3 V-compatible for a host input: the highest the board presents to a host input is the zener clamp, about 3.5 V (SENSE_*) or 3.3 V (3V3_REF).
Current limited by series resistance: SENSE pins 1 k, LOAD_EN 1 k, 3V3_REF 100 ohm. GND pins sit beside every signal group. Orientation: pin 1 at the left end as placed (x = 20 mm, y = 69 mm); the `+3V3 / GND` pair is the first two pins.

## J1 input, JST S2B-XH-A-1(LF)(SN), side entry, 2.50 mm (`JST_XH_S2B-XH-A-1_1x02_P2.50mm_Horizontal`)
| Pin | Net | Note |
|---|---|---|
| 1 | VIN_12V_RAW | +12 V (9-15 V). Mate with JST XHP-2 and SXH-001T-P0.6 contacts [recalled]. Rated 3 A per contact [recalled]; board draws at most 1.0 A at 9 V when loaded to 1.5 A total |
| 2 | GND | |

## Output connectors (same part and footprint as J1)
| Ref | Pin 1 | Pin 2 | Current the board can deliver (see POWER_BUDGET.md) |
|---|---|---|---|
| J3 | OUT_5V (after F2) | GND | shared 1.5 A with J4, F2 holds 1.5 A |
| J4 | OUT_5V_SW (after F3) | GND | shared 1.5 A with J3, F3 holds 1.5 A, off by default |
| J5 | +3V3 | GND | 0.35 A defended (0.5 A target not supported), no fuse |

## ICs (package pad to net)
| Part | Pad: pin = net |
|---|---|
| U1 MP2307DN SOIC-8E (`mp2307_buck_ep`) | 1 BS = BUCK_BS; 2 IN = +12V_PROT; 3 SW = BUCK_SW; 4 GND = GND; 5 FB = BUCK_FB; 6 COMP = BUCK_COMP; 7 EN = BUCK_EN; 8 SS = BUCK_SS; **9 exposed pad = GND** |
| U3 TPS22918DBV SOT-23-6 (`tps22918_sot23_6`, **pinout [recalled]; the shipped registry part differs**) | 1 VIN = +5V; 2 GND = GND; 3 ON = LOAD_EN; 4 CT = LS_CT; 5 QOD = +5V_SW; 6 VOUT = +5V_SW |
| U2 AMS1117-3.3 SOT-223, tab = pin 2 | 1 GND = GND; 2 VOUT = +3V3 (tab); 3 VIN = +5V |
| Q1 Si2301CDS SOT-23 | 1 G = PFET_GATE; 2 S = +12V_PROT; 3 D = VIN_12V_FUSED |
| D1 SMAJ15A SMA | 1 cathode = +12V_PROT; 2 anode = GND (footprint `D_SMA`: pad 1 is the cathode bar side; **polarity vs the footprint was not independently verified**) |
| F1, F2, F3 | pads 1, 2 (no polarity) |
| LEDs | pad 2 = anode, pad 1 = cathode (registry convention; footprint `LED_0603_1608Metric` pad 1 = cathode) |
| C1, C10, C17 (electrolytic) | p1 = + ; `CP_Radial_D6.3mm_P2.50mm` pad 1 = + (assumed, not verified) |

Test points (1.5 mm pads): TP1 VIN_12V_RAW, TP2 VIN_12V_FUSED, TP3 +12V_PROT, TP4 +5V, TP5 +5V_SW, TP6 +3V3, TP7 BUCK_SW, TP8 LOAD_EN, TP9 GND, TP10 GND.
