# POWER_BUDGET — CM4 carrier (SYN-407)

No rail was measured. The numbers below are datasheet or specification
figures. The Synth ERC E-SYNTH-POWER-009/010 check is clean for the nets it
sees; it sums the declared `max_current_ma` of the parts on each regulator
output, which is not the same as the measured load.

## Rails

| Rail | Source | Limit | Consumers (datasheet/spec) | Sum | Margin |
| --- | --- | --- | --- | --- | --- |
| `VIN_12V` | J1 12 V | F1 2.6 A hold, 16 V | 5 V buck input current | 5 V × 3.77 A / (12 V × 0.9) ≈ 1.75 A | 0.85 A to the PTC hold |
| `+5V` | U1 LMR33640, 4 A | 4.0 A | CM4 1.4 A typical + 4 USB ports 2.0 A + HDMI 0.05 A + fan 0.05 A | 3.50 A | 0.50 A (12.5 %) |
| `+5V_AUX` | L_FB1 from +5V | bead rating not in registry | 3V3 buck 0.27 A + 1V8 buck ~0 A | 0.27 A | not verified |
| `+3V3` | U2 LMR38010, 1 A | 1.0 A | USB2514B 0.10 A + SD 0.10 A + RTC 0.001 A + LEDs 0.02 A + GPIO header 0.05 A | 0.271 A | 0.729 A |
| `+1V8` | U3 LMR38010, 1 A | 1.0 A | J_VREF only, ~0 A | ~0 A | 1.0 A |

The **true** +5V load includes the +5V_AUX branch: 3.50 A + 0.27 A = 3.77 A,
leaving 0.23 A (5.8 %) against the 4 A regulator. The E-SYNTH-POWER-010 rule
sees the +5V net without the bead branch, so its 3.50 A sum is not the whole
story. The 0.23 A margin is the number that matters.

## Where the numbers come from

| Consumer | Current | Source |
| --- | --- | --- |
| CM4 +5V | 1.4 A typical, 0.4 A idle, 8 mA software shutdown | CM4 datasheet 5.3, page 25 |
| CM4 +5V connector pins | 6 pins × 0.3 A = 1.8 A | Hirose DF40C-100DS-0.4V(51) rated current 0.3 A/contact |
| USB 2.0 port | 500 mA each, 2.0 A for four | USB 2.0 specification; TPS22918 is rated 2 A |
| USB2514B | ~100 mA at 3.3 V | Microchip DS00001692C; the exact 3.3 V operating current was not read from a table, so 100 mA is a placeholder |
| HDMI +5V | 50 mA | HDMI specification; the connector +5V pin is current-limited by U10 |
| Fan | 50 mA | assumed for a small 5 V fan; not a datasheet figure |
| 3V3 buck input | 0.27 A at 5 V for 0.27 A at 3.3 V | LDO/buck input current ≈ output current; buck efficiency ignored |
| LEDs | ~20 mA total | 470 Ω from 3.3 V, about 2.8 mA each |

## Regulator settings

| Rail | Part | Feedback | Inductor | Frequency | PG |
| --- | --- | --- | --- | --- | --- |
| +5V | LMR33640 | 100 k / 24.9 k, VREF 1.0 V -> 5.02 V | 6.8 µH | 400 kHz fixed | open drain, 100 k to +5V |
| +3V3 | LMR38010 | 100 k / 43.2 k -> 3.31 V | 6.8 µH | 800 kHz (33 k RT) | open drain, 100 k to +3V3 |
| +1V8 | LMR38010 | 100 k / 125 k -> 1.80 V | 6.8 µH | 800 kHz (33 k RT) | open drain, 100 k to +1V8 |

## Limits and honesty notes

- The CM4 datasheet gives **typical** operating current (1.4 A) and no
  maximum. A CM4 with a heavy workload, a PCIe card or a 5 V fan can draw
  more. The 4 A rail then loses margin; this is a design risk, not a measured
  result.
- The DF40 connector's 0.3 A per contact limits the six +5V pins to 1.8 A.
  The CM4's 1.4 A typical leaves 0.4 A; a 2 A CM4 load would exceed the
  connector rating. This is a real limit of the module interface, not of the
  buck.
- The 0603 ferrite bead's current rating is not in the registry. 0.27 A is
  probably inside a 0603 bead's rating, but it was not read from a datasheet.
- The 5 V buck's 4 A rating is the regulator's, not a measured board limit.
  The inductor (SRN6045TA-6R8M) is rated 3.6 A Irms / 5.7 A Isat; the 4 A
  load is above the Irms rating. This is a real margin gap and a reason the
  board stays experimental.
- The 3V3/1V8 bucks are 1 A parts with 0.27 A and ~0 A loads; their inductors
  are the same 3.6 A Irms part, so they are not the limit.
- E-SYNTH-POWER-010 is clean because the declared `max_current_ma` of the
  parts on +5V sums to 3.50 A. The +5V_AUX branch is not on that net. The
  true total is 3.77 A.
