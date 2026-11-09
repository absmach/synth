# Power budget: BLDC stack (SYN-408)

Every number below is either taken from a datasheet/registry value or is an
explicit assumption. Nothing was measured.

## Rails

| Rail | Source | Limit | Load | Current | Margin |
| --- | --- | --- | --- | --- | --- |
| VBUS 24 V | J_PWR, external supply | 10 A (stated, no fuse) | reverse P-FET, bridge, 12 V buck, divider | 10 A at the stated maximum | 0 % at the limit; the supply must enforce it |
| +12V | LMR38010, 1 A | 1 A | AMS1117 input + gate drivers | 0.12 A | 88 % |
| +12V_GD | Q_GE P-FET from +12V | 1 A | 3 x IR2101 VCC and gate charge | 0.02 A | 98 % |
| +3V3 (control) | AMS1117-3.3, 800 mA | 0.8 A | MCU, RS-485, Hall, encoder, LEDs, LDO quiescent | 0.084 A | 90 % |
| +3V3 (power) | J_B2B pin 13 | 2.5 A contact | 2 x MCP6022, reference, pull-ups | 0.004 A | >99 % |

## Control 3.3 V loads

| Load | Current | Basis |
| --- | --- | --- |
| STM32G030K8 | 20 mA | registry `max_current_ma = 100`, design uses 20 mA as a working figure |
| THVD1450D | 3 mA | registry `max_current_ma = 3` |
| Hall sensors x3 | 30 mA | 10 mA each, assumed (no sensor datasheet chosen) |
| Encoder | 10 mA | assumed |
| LEDs x2 | 6 mA | (3.3 - 2.1) / 470 = 2.55 mA each |
| AMS1117 quiescent | 5 mA | datasheet quiescent current, assumed |
| Filter/divider current | 5 mA | 12 V divider 0.1 mA, sense dividers and pull-ups |
| **Total** | **84 mA** | 10.5 % of the 800 mA rating |

AMS1117 dissipation: (12 - 3.3) V x 0.084 A = 0.73 W. The registry declares
1360 mW and 60 C/W, so `synth validate` warns that the junction can reach about
107 C at 25 C ambient. This is the part's worst-case figure, not the actual
load; it still needs a thermal review and copper.

## 12 V loads (power board)

| Load | Current | Basis |
| --- | --- | --- |
| AMS1117 input | 84 mA | the 3.3 V load above, plus the LDO's own drop |
| IR2101 quiescent x3 | 4.5 mA | 1.5 mA each, datasheet |
| Gate charge x6 | 4.1 mA | 34 nC x 20 kHz x 6, BSC030N08NS5 Qg |
| Margin | 27 mA | 20 % |
| **Total** | **120 mA** | 12 % of the 1 A LMR38010 |

LMR38010 loss at 24 V -> 12 V and 0.12 A: about 0.15 W with an assumed 90 %
efficiency. The inductor is 6.8 uH, 3.6 A RMS, 5.7 A saturation; the ripple at
800 kHz is about 0.2 A.

## Power-stage loss at 10 A

| Item | Value | Basis |
| --- | --- | --- |
| Reverse P-FET Q_REV | 2.0 W | 20 mOhm x 10 A^2 |
| Three conducting MOSFETs | 0.9 W | 3 mOhm x 10 A^2, one high + one low per phase at 50 % duty |
| Three shunts | 1.5 W | 5 mOhm x 10 A^2 each |
| 12 V buck | 0.15 W | assumed 90 % efficiency at 0.12 A |
| Gate drive and switching | not computed | Qg and Coss losses need the real gate resistors and layout |
| **Known subtotal** | **4.55 W** | before switching, gate-drive and inductor losses |

The 2 oz outer copper, 2 mm phase tracks and thermal vias are sized for this
subtotal. Junction temperatures are not computed: the datasheet RthetaJA is not
valid for design, the copper area is not extracted, and no thermal solver was
run.

## Assumptions that a reviewer must check

- Hall sensor and encoder currents are assumed, not read from a chosen part.
- The 2512 shunt is assumed to be a 1 W part; the generic registry entry has no
  power rating.
- The MOSFET switching loss is omitted; at 20 kHz and 24 V it is not negligible.
- The 12 V buck efficiency is assumed 90 %.
- The AMS1117 quiescent current and the thermal resistance are datasheet values,
  not measured.
- No load-transient, inrush or stall current was analysed. A motor stall at
  10 A is the design point, not a fault case.
