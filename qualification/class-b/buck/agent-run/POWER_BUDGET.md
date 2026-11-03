# Power budget: buck rev A

Source: `../buck.synth`, registry part files, `../README.md`, `../QUALIFICATION.md`. Datasheet figures are those recorded in the part files and README (URLs in `PIN_MAPPING.md`); not re-read. Derived numbers are plain arithmetic on those figures. Nothing was measured.

## Rails

| Rail | Source | Nominal / range | Loads | Regulator or part limit | Margin |
|---|---|---|---|---|---|
| VIN_RAW | J1 | 24 V, +10 % = 26.4 V max | F1 | J1 3 A / 250 V (JST XH sheet) | 0.162 A at 24 V and 1 A out is far below 3 A |
| VIN_FUSED | F1 | same, after PTC | D1 | F1 Ihold 1.1 A at 23 C, 0.93 A at 40 C, 0.73 A at 60 C, 0.63 A at 70 C, 0.50 A at 85 C; Itrip 2.2 A; Vmax 33 V | input 0.162 A (24 V) / 0.240 A (16.2 V) vs 0.63 A at 70 C = 3.9x / 2.6x |
| VIN | D1 | 24 V minus D1 drop (VF 0.75 V max at 3 A; VF at low current not read) | D2 TVS, C1/C2/C3, U1 VIN, UVLO divider | U1 4.2-80 V operating, 85 V abs max; D1 60 V / 3 A; C1/C2/C3 50 V | 26.4 V vs 80 V operating: 53.6 V. TVS clamp 48.4 V vs 85 V: 36.6 V. TVS clamp vs 50 V capacitors: 1.6 V |
| SW | U1 | switching node | L1, C4 | SW abs max 85 V | not measured |
| +3V3 | U1 via L1 | 3.315 V typ (3.26 to 3.36 V over Vref 0.985-1.015 V, ideal resistors) | C5/C6/C9, J2 pin 1, D3 + R_LED, D4 + R_PGLED, R_PG, FB divider | U1 1 A rated (registry `max_current_ma` 1000); C5/C6/C9 25 V | 3.3 V vs 25 V rating; current: rated 1 A = converter limit, margin 0 by design |

## Input current (README)

Efficiency 85 % is an assumption. Iin = 3.3 V x 1 A / 0.85 / Vin:

| Vin | Iin at 1 A out |
|---|---|
| 24 V | 0.162 A |
| 16.2 V (UVLO stop) | 0.240 A |

Loss at 1 A and 85 %: 3.3 / 0.85 - 3.3 = 0.58 W (assumption). Junction temperature: not verified (RthetaJA 42.9 C/W is not valid for design per the datasheet text, no copper-area curve).

## Inductor

L1 SRN6045TA-6R8M: 6.8 uH +/-20 %, DCR 34 mOhm typ, Irms 3.6 A, Isat 5.7 A (L -30 %). Ripple 0.53 A pp at 26.4 V in and 800 kHz; peak about 1.27 A at 1 A out; margin to Isat 5.7 A. Eq 11 minimum 1.03 uH vs 6.8 uH.

## Small loads (arithmetic, not measured)

| Load | Value | Basis |
|---|---|---|
| D3 power LED | 1.9 to 3.0 mA | (3.3 - 2.4 .. 1.9 V) / 470 ohm, Vf 1.9-2.4 V at 20 mA; LED max 30 mA |
| D4 PG LED (lit only while PG low) | about 2.55 mA | (3.3 - 2.1) / 470, Vf 2.1 V taken from the range |
| R_PG | 33 uA | 3.3 V / 100k when PG low; datasheet limit 5 mA DC |
| UVLO divider | about 16 uA at 24 V | 24 V / (1.37M + 100k) |
| FB divider | about 23 uA | 3.315 V / (100k + 43.2k) |
| U1 quiescent / PFM | not verified | not recorded in the files |

## Protection

- D2 SMBJ30A: 600 W 10/1000 us, VWM 30 V (above the 26.4 V max input), VBR 33.3-36.8 V at 1 mA, VC 48.4 V at 12.4 A. Surge energy, F1 under surge: not verified.
- D1 SS36: reverse block, VRRM 60 V vs 26 V. IFSM 100 A, IR 0.5 mA at 25 C and 10 mA at 100 C.
- Reverse polarity: D1 blocks; F1 and the capacitors see no reverse voltage beyond D1's leakage (not measured).
- Short circuit and current limit of U1: not verified (not recorded).

## Sequencing

U1 UVLO start 18.4 V typ (16.2 to 20.6), stop 16.2 V typ (14.0 to 17.9). Internal soft start; PG delay about 4 ms from EN asserted (datasheet 8.3.5 per README). Inrush into C1 and the output capacitors: not analysed (QUALIFICATION B11).

## Not verified

Efficiency, quiescent current, thermal rise, ripple, load transient, C2/C5/C6/C9 DC-bias derating, C1 ESR, PTC behaviour under surge, inrush.
