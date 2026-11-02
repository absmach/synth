# Power budget — protected_power_entry rev A

All device limits are [recalled] (no datasheet was read in this session). Arithmetic is in `DESIGN_NOTES.md` section 3.

## Rails
| Rail | Source | Voltage | Defended current | Limiting element |
|---|---|---|---|---|
| VIN_12V_RAW / FUSED | J1 | 9-15 V (nominal 12 V); 16 V is not used (SMAJ15A standoff 15 V) | 1.0 A at 9 V when the buck is loaded to 1.5 A | F1 hold 1.5 A (derated about 1.2 A at 40 C) |
| +12V_PROT | Q1 | input - 0.1 V | same | Q1 Rds(on) about 0.11 ohm, -20 V Vds, +/-8 V Vgs |
| +5V (buck) | U1 MP2307 | 5.014 V (4.85-5.18 V) | 1.5 A total shared by J3, J4 and the 3V3 LDO input | F1 at 9 V in; U1 is 3 A rated |
| +5V_SW | U3 TPS22918 | +5V minus about 0.06 V | part of the 1.5 A; U3 rated 2 A | F3 1.5 A hold; U3 has no current limit |
| OUT_5V | F2 | +5V minus about 0.1-0.2 V at 1.5 A | part of the 1.5 A | F2 1.5 A hold |
| +3V3 | U2 AMS1117-3.3 | 3.3 V +/-1 % | **0.35 A** (0.5 A target not supported) | SOT-223 on minimal copper |

## Input current (Pin = Pout / 0.85)
| Total 5 V-side load | Pout | Pin | at 9 V | at 12 V | at 15 V |
|---|---|---|---|---|---|
| 1.5 A (defended) | 7.52 W | 8.85 W | 0.98 A | 0.74 A | 0.59 A |
| 2.0 A (target) | 10.0 W | 11.8 W | 1.31 A | 0.98 A | 0.78 A |
Maximum recommended input current: **1.0 A** continuous. At 2.0 A the 9 V input current is 109 % of F1's derated hold current.

## Consumption of each part (typical, from the design)
| Item | Current / power |
|---|---|
| LEDs on +12V_PROT / raw (D2, D3) | 2.9 mA + 2.7 mA at 12 V |
| LEDs on +5V (D4, D5) | 2.9 mA each |
| LED on +3V3 (D6) | 2.8 mA |
| Fault LED D7 | 0 normally; 3 mA at 12 V when F1 has tripped |
| SENSE_12V divider | 1.0 mA at 12 V (1.25 mA at 15 V) |
| SENSE_5V, SENSE_3V3 dividers | 0.25 mA and 0.165 mA |
| EN divider R5/R6 | 0.09 mA at 12 V |
| Gate network R1/D9 | 0.15 mA at 12 V |
| U1 quiescent, U3, U2 | not verified (datasheet values not read); AMS1117 quiescent about 5 mA [recalled] |
| Total fixed overhead | about 25 mA at the 12 V side |

## Dissipation and thermal (estimates, not simulations)
| Part | Dissipation | Copper | Result |
|---|---|---|---|
| F1 | I^2 R about 0.1-0.2 W at 1 A | 1812 pads | within hold rating when derated |
| Q1 | 0.11 ohm x 1 A^2 = 0.11 W | SOT-23 pads | about +50 C at 400 C/W [recalled] SOT-23 minimal; acceptable |
| U1 | 1.33 W total converter loss at 1.5 A; IC about 0.45-0.5 W; L1 about 0.3 W | exposed pad 8 mm^2 joined to the GND fills; **copper area and thermal vias not measured** | Tj about 55 C at 25 C ambient with 60 C/W assumed |
| U2 | (5.18 - 3.3) x 0.35 A = 0.66 W (defended); 0.94 W at 0.5 A | tab pad plus its trace only; **no pour** | 0.66 W -> +59 C at 90 C/W [recalled]; 0.94 W -> +85 C, Tj 135 C at 50 C ambient, over 125 C |
| U3 | (0.06 ohm [recalled]) x 1.5 A^2 = 0.14 W | SOT-23-6 | fine |
Synth's own check (W-SYNTH-THERMAL-001) estimates U2 at 1.36 W / 107 C because it assumes 0.8 A; it is a warning, not an error.

## Source selection
One source only (J1). No OR-ing, no back-feed path to J1 other than stored charge through an on-state Q1 (DESIGN_NOTES 3.2).

## Current limits and short-circuit behaviour per output
| Output | Limit | Behaviour on a hard short |
|---|---|---|
| J3 +5V | F2 hold 1.5 A / trip 3 A; buck limit about 4-4.5 A [recalled] | buck current limit then F2/F3/F1 trip; L1 Isat about 3.5 A is below the buck limit |
| J4 +5V_SW | F3 same; U3 off by default | no U3 limit; F3 and buck |
| J5 +3V3 | AMS1117 about 1 A [recalled], thermal shutdown | about 5 W in U2 until shutdown (no fuse) |
| Back-driven voltage | none (no blocking diodes, no clamps on J3-J5) | accepted risk |

## Open issues
1. 2.0 A total / 0.5 A on 3V3 are not supported by F1 and by the U2 copper.
2. U1 and U2 thermal copper not measured.
3. U1 current limit, EN abs max, VIN abs max, TPS22918 pinout and Ron are [recalled].
