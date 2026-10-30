# i2c_sensor_node, power budget (deliverable 7)

Datasheet figures are **[recalled]**, not re-read in this session. All values are worst-case budgets, not measurements.

## 1. Inputs and rails

| Item | Value |
|---|---|
| J1 input | 4.75 to 5.5 V DC, use 5.0 to 5.5 V for regulator headroom |
| USB-C VBUS input | 4.75 to 5.5 V, USB Standard Downstream Port limit 500 mA |
| `+5V_PROT` | input minus fuse and diode: about 4.3 V (at 4.75 V in) to 5.1 V (at 5.5 V in), at 0.1 A |
| `+3V3` | 3.3 V from AMS1117-3.3, +/-1% [recalled] |
| `3V3_REF` | `+3V3` minus 0.3 V (D14) minus 0.1 ohm x load, **sense/reference only**, 3 mA max, 30 mA into a short |

## 2. 3.3 V load estimate (worst case)

| Load | Budget |
|---|---|
| STM32G030 at 64 MHz, run | 10 mA (typical about 5 mA) |
| BME680, gas heater pulse (measurement mode) | 18 mA peak (ultra-low-power average well under 1 mA) |
| 24LC256 write | 3 mA max (read 0.4 mA) |
| I2C pull-ups, both lines low | 1.4 mA |
| CH340C | 15 mA |
| Power LED D15, 1.6 mA | 1.6 mA |
| Status LED D16 | 1.9 mA |
| Activity LEDs D17, D18 | 2.2 mA |
| 3V3_REF external load | 3 mA |
| GPIO header, 8 pins, assumed 1 mA each | 8 mA |
| **Total** | **about 65 mA; round to 71 mA with PRTR5V0U2X leakage and MCU I/O; design to 100 mA** |

Current available to external loads: 3 mA from `3V3_REF` (sense only), and what the GPIO pins can source (limit per pin and per package from the MCU datasheet, through 330 ohm each). The board does not offer a general-purpose power output.

## 3. Maximum input current

100 mA at 3.3 V through a linear regulator is 100 mA from the input (plus about 1 mA ground-pin current [recalled]). So the maximum recommended input current is **0.11 A**, fuse hold current is 0.5 A (4.5x), trip about 1.0 A.

Worst credible fault current at the fuse is limited by the source: a USB port limits at about 0.5 to 1.5 A before it shuts off.

## 4. Regulator U1, AMS1117-3.3

- Input at worst high: 5.5 V - 0.10 V - 0.30 V = 5.10 V.
- Dissipation at 0.1 A: (5.10 - 3.30) x 0.1 = **0.18 W**. At 0.2 A (2x load): 0.36 W.
- Current limit: about 1 A internally [recalled]. Not relied on.
- Thermal: SOT-223 with minimum copper, theta_JA about 110 C/W [recalled; Synth uses 60 C/W]. Rise 0.18 W x 110 = **20 C**; at 0.36 W it is 40 C. Junction at 40 C ambient: 60 C at 0.1 A, 80 C at 0.2 A. Limit 125 C.
- The tab is VOUT (pin 2). I gave it no extra copper: the router connects it to +3V3 with ordinary track widths. The exported bottom-layer ground pour does not help, since it is a different net.
- Synth's own warning `W-SYNTH-THERMAL-001` says U1 reaches 107 C (1360 mW over 60 C/W). That estimate assumes an 0.8 A load. This board cannot draw that: the load budget is 0.1 A, and the fuse holds at 0.5 A. I accept the warning on that basis.
- Output capacitor: 47 uF tantalum (`C3`, ESR helps AMS1117 stability) + 10 uF ceramic + 100 nF. Input: 2 x 10 uF.

## 5. Source selection

Diode-OR through D1 and D2. With both connected, the higher voltage carries the current; a 0.1 V difference moves most of the load. No current flows from one source into the other.

## 6. Hot parts and copper

| Part | Dissipation | Copper given |
|---|---|---|
| U1 AMS1117 | 0.18 W | standard track and the SOT-223 pads only |
| D1/D2 | 35 mW | pads only |
| F1/F2 | 10 mW at 0.1 A; large when tripped, normal for PPTC | pads only |
| R23 3V3_REF | 90 mW into a short (125 mW rating) | pads only |

## 7. Notes the budget depends on

- BME680 measurement-mode current and EEPROM write current are the two entries above; both **[recalled]**.
- The USB-C path draws from a host that supplies 500 mA unless the host negotiates more; the 5.1 k CC pull-downs advertise a default-current sink only.
- No claim is made that the board supports a load other than the ones listed.
