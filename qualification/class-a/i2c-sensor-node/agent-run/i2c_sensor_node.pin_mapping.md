# i2c_sensor_node, pin mapping (deliverable 6)

MCU pin table: see `i2c_sensor_node.design_notes.md` section 3. Header and connector tables follow.

## J1, 5 V input (JST S2B-XH-A-1, 2.50 mm, side entry, mates with XHP-2)

| Pin | Net | Note |
|---|---|---|
| 1 | VIN_5V_RAW (+5 V) | to F1 |
| 2 | GND | |

## J2, USB-C receptacle (HRO TYPE-C-31-M-12)

| Pad | Net | Note |
|---|---|---|
| A4, B4, A9, B9 | VBUS_USB | duplicated pads, all joined |
| A1, B1, A12, B12 | GND | |
| SH | GND | shield tied directly to ground |
| A5 | USB_CC1 | 5.1 k to GND (R1), D6 clamp |
| B5 | USB_CC2 | 5.1 k to GND (R2), D6 clamp |
| A6, B6 | USB_DP | joined, to D5 and CH340C UD+ (pin 5) |
| A7, B7 | USB_DN | joined, to D5 and CH340C UD- (pin 6) |
| A8, B8 (SBU) | unconnected | |

## J3, UART break-out (JST S4B-PH-K-S, 2.00 mm, mates with PHR-4)

| Pin | Net | Direction | Protection |
|---|---|---|---|
| 1 | GND | | |
| 2 | UART_TX | MCU PA2 out to host | R11 330R, D7 clamp, TP6 |
| 3 | UART_RX | host to MCU PA3 in | R12 1k, D7 clamp, TP7 |
| 4 | 3V3_REF | out, sense only | D14 + R23 100R, 3 mA max |

Host logic level: 3.3 V.

## J4, SWD (2.54 mm, 1x5, pin 1 marked on the footprint)

| Pin | Net | MCU pin |
|---|---|---|
| 1 | 3V3_REF (VTref, sense only) | |
| 2 | SWDIO | PA13 (pin 18) via R13 100R |
| 3 | SWCLK | PA14 (pin 19) via R14 100R |
| 4 | GND | |
| 5 | NRST | NRST (pin 6) |

Do not power the board from the probe's 3.3 V output: 3V3_REF is blocked by D14 from feeding the rail.

## J5, GPIO / analog (2.54 mm, 1x12)

| Pos | Net | MCU pin (pkg pin) | Special function | Restriction |
|---|---|---|---|---|
| 1 | 3V3_REF | | sense only | 3 mA |
| 2 | GND | | | |
| 3 | GPIO_PA0 | PA0 (7) | ADC_IN0 | 330R series, ESD clamp D10 |
| 4 | GPIO_PA1 | PA1 (8) | ADC_IN1 | 330R, D10 |
| 5 | GPIO_PA4 | PA4 (11) | ADC_IN4, SPI1_NSS | 330R, D11 |
| 6 | GND | | | |
| 7 | GPIO_PA5 | PA5 (12) | ADC_IN5, SPI1_SCK | 330R, D11 |
| 8 | GPIO_PA6 | PA6 (13) | ADC_IN6, SPI1_MISO, TIM3_CH1 | 330R, D12 |
| 9 | GPIO_PA7 | PA7 (14) | ADC_IN7, SPI1_MOSI, TIM3_CH2 | 330R, D12 |
| 10 | GND | | | |
| 11 | GPIO_PB0 | pad 15 (PB0) | ADC_IN8, TIM3_CH3 | 330R, D13; PA8, PB1, PB2 share the pad |
| 12 | GPIO_PB9 | PB9 (2) | GPIO | 330R, D13 |

Not on the header (reserved): PB7/PB6 (I2C), PA2/PA3 (UART), PA9/PA10 (CH340C), PA13/PA14 (SWD/BOOT0), PC15 (status LED), NRST.

Clamp arithmetic: PRTR5V0U2X rail-to-rail diodes to +3V3 and GND, so a pin sees at most about +3.3 + 0.7 = 4.0 V and at least -0.7 V. The 330R limits injected current to (5.5 - 4.0) / 330 = 4.5 mA for a 5.5 V source. ADC source impedance is raised by 330R; slow signals only.

## Silkscreen

Synth offered no call to place silkscreen text. Only the auto-generated reference designators are on the board. Pin-1 and polarity markings come from the footprints. **No labels such as "5V +" or "TX" exist on the exported board.**
