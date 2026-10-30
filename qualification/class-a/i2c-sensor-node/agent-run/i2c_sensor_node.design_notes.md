# i2c_sensor_node, design notes (deliverable 8)

Revision A, two-layer, JLCPCB. Source: `i2c_sensor_node.synth`. Placement sidecar: `i2c_sensor_node.placement.layout.toml`.

Everything below that comes from a datasheet was recalled from memory. There was no web access in this session (`synth_search_registry_web` returned "LCSC search panicked"), so no datasheet was re-read. Each such value is marked **[recalled]** and should be checked before ordering boards.

## 1. Architecture

```
J1 (JST XH, 5 V) -> VIN_5V_RAW -> F1 polyfuse -> VIN_5V_FUSED -> D1 Schottky (series) --+
                                                                                        +-> +5V_PROT -> U1 AMS1117-3.3 -> +3V3
J2 (USB-C VBUS)  -> VBUS_USB -> F2 polyfuse -> VBUS_FUSED -> D2 Schottky (series) -----+       |
                      (D3 TVS on VBUS_USB)                           (D4 TVS on +5V_PROT)        +-> U2 MCU, U3 BME680, U5 EEPROM, U4 CH340C
```

Why the fuse comes first: a short on the cable or board side is met by a part that limits it and resets. Why the diode comes second: a reversed supply on J1 meets a blocking diode. The fuse is never in the path of a reverse current, and `D4` and every capacitor sit behind `D1`, so a reversed supply never reaches them.

The USB-C path has its own fuse and diode. The two diodes form a diode-OR onto `+5V_PROT`. When both sources are connected, the higher voltage supplies the board and neither source is back-fed, because each diode blocks current into its own connector. Two regulated sources are never joined directly.

## 2. Selections

| Item | Choice | Source |
|---|---|---|
| MCU | STM32G030F6P6, TSSOP-20 | **authored** part `stm32g030f6p6` (see section 8) |
| Sensor | BME680, 3.0 x 3.0 mm 8-pin LGA | **authored** part `bme680_lga8` (registry footprint was the 2.5 x 2.5 mm BME280 one) |
| EEPROM | 24LC256-I/SN, SOIC-8 | **authored** part `24lc256_soic8` (registry pin map was wrong) |
| USB bridge | CH340C, SOP-16, internal oscillator, no crystal | **authored** part `ch340c` (registry has only the crystal-needing CH340G) |
| Regulator | AMS1117-3.3, SOT-223, tab = VOUT | registry `ams1117_3v3` |
| Fuses | Bourns MF-NSMF050-2, 0.5 A hold, 13.2 V, 1206 | registry `polyfuse_500ma` |
| Series diodes | 1N5819-class Schottky, SOD-123 | registry `d_schottky` (flagged in section 9) |
| ESD, 2-line | Nexperia PRTR5V0U2X | registry `prtr5v0u2x` |
| TVS | Nexperia PTVS6V0S1UR | registry `ptvs6v0s1ur` |
| J1 | JST S2B-XH-A-1 (mates with XHP-2), pin 1 = +5 V, pin 2 = GND | registry `jst_xh_2pin` |
| J2 | HRO TYPE-C-31-M-12 USB-C receptacle | registry `usb_c_receptacle` |
| J3 | JST S4B-PH-K-S (mates with PHR-4) | registry `jst_ph_4pin` |
| J4, J5 | 2.54 mm pin headers, 1x5 and 1x12 | **authored** `header_1x5`, `header_1x12` (none fit) |

## 3. MCU pin-assignment table (STM32G030F6P6, TSSOP-20)

Pin map checked against KiCad `MCU_ST_STM32G0:STM32G030F6Px` by import (20 pins, same numbers as the authored part).

| Signal | MCU pin name | Pkg pin | Alternate function | Destination | Constraints | Shared |
|---|---|---|---|---|---|---|
| I2C_SDA | PB7 | 1 | I2C1_SDA AF6 | U3.3, U5.5, R9, TP4 | open-drain, 4.7k pull-up | yes, bus |
| GPIO_PB9 | PB9 | 2 | GPIO | J5.12 via R22 330R + D13 | 330R series, clamp | no |
| LED_STATUS | PC15 | 3 | GPIO out | R25 -> D16 amber | PC14/PC15 limited to about 3 mA, LED uses 1.9 mA | no |
| +3V3 | VDD/VDDA | 4 | power | C6 100nF + C7 4.7uF | one supply pair | no |
| GND | VSS | 5 | power | GND | | no |
| NRST | NRST | 6 | reset | R3 10k, C8 100nF, SW1, R4 (DTR), J4.5, D9 | | yes |
| GPIO_PA0 | PA0 | 7 | ADC_IN0 | J5.3 | | no |
| GPIO_PA1 | PA1 | 8 | ADC_IN1 | J5.4 | | no |
| UART_TX | PA2 | 9 | USART2_TX AF1 | R11 330R -> J3.2 | | no |
| UART_RX | PA3 | 10 | USART2_RX AF1 | R12 1k <- J3.3 | | no |
| GPIO_PA4 | PA4 | 11 | ADC_IN4, SPI1_NSS | J5.5 | | no |
| GPIO_PA5 | PA5 | 12 | ADC_IN5, SPI1_SCK | J5.7 | | no |
| GPIO_PA6 | PA6 | 13 | ADC_IN6, SPI1_MISO, TIM3_CH1 | J5.8 | | no |
| GPIO_PA7 | PA7 | 14 | ADC_IN7, SPI1_MOSI, TIM3_CH2 | J5.9 | | no |
| GPIO_PB0 | PA8/PB0/PB1/PB2 pad | 15 | ADC_IN8, TIM3_CH3 (PB0) | J5.11 | PA8, PB1, PB2 share the pad: keep them as inputs | no |
| BRIDGE_RXD | PA9 | 16 | USART1_TX AF1 | U4.3 (CH340C RXD) | | no |
| BRIDGE_TXD | PA10 | 17 | USART1_RX AF1 | U4.2 (CH340C TXD) | | no |
| SWDIO | PA13 | 18 | SWDIO | R13 100R -> J4.2 | | no |
| SWCLK / BOOT0 | PA14 | 19 | SWCLK, BOOT0 | R14 100R -> J4.3; R7, R8, Q1 boot network | shared by design, see section 5 | yes |
| I2C_SCL | PB6 | 20 | I2C1_SCL AF6 | U3.4, U5.6, R10, TP5 | open-drain | yes, bus |

Pads 1, 15 and 20 each bond several GPIOs to one pin. Firmware must leave the unused ones (PB8, PA8/PB1/PB2, PB3/PB4/PB5) as inputs or analog. Every package pin is used, so no pin is left floating and no pin has to be tied off.

## 4. Power-input protection chain (calculations)

Load budget: 71 mA worst case, 100 mA with margin (section 7 of the power budget).

**Fuse F1/F2, MF-NSMF050-2 [recalled].** Hold 0.5 A against 0.1 A worst load is 5x margin. Trip is about 1.0 A, well above anything the board draws in normal use. It is rated 13.2 V against a 5.5 V maximum input. It is a 1206 part. Its resistance is assumed at most 1.0 ohm, which drops 0.1 V at 0.1 A.

**Diode D1/D2, 1N5819-class [recalled].** Forward drop is about 0.30 to 0.35 V at 0.1 A. Reverse rating is 40 V against a worst reversed 5.5 V. Dissipation is 0.1 A x 0.35 V = 35 mW. Leakage when reversed is microamps.

**Regulator headroom.** J1 worst low supply is 4.75 V.

```
4.75 V - 0.10 V (F1) - 0.35 V (D1) = 4.30 V at U1 VIN
AMS1117-3.3 dropout [recalled]: about 1.0 V at 100 mA, 1.1 V typ / 1.3 V max at 800 mA
=> regulation needs VIN >= 4.3 to 4.5 V
```

At the very bottom of the 5 V tolerance, the regulator has about zero margin and +3V3 may sag to roughly 3.2 to 3.3 V. All loads still work down to 3.0 V (MCU, BME680, 24LC256, CH340C at 3.3 V nominal). **The J1 supply should be 5.0 to 5.5 V.** A USB host at 4.75 V min is in the same situation, so a 5 V USB port is fine in practice but without margin at the corner.

**Input range stated:** 4.75 to 5.5 V on J1 and on USB VBUS. Above about 6.7 V the TVS (`D4`, 6.0 V standoff) starts to clamp and the fuse trips. A 12 V supply on J1 is not tolerated.

**Fault table.**

| Event | Element that answers it |
|---|---|
| Overload or short downstream | F1 / F2 trip and reset |
| Reversed supply on J1 | D1 blocks; no current through F1, D4 or the capacitors |
| Transient on a long cable | D4 on `+5V_PROT`, D3 on `VBUS_USB` clamp; fuse and diode in front limit the energy |
| ESD on USB D+/D-, CC | D5, D6 (PRTR5V0U2X) |
| Both sources connected | diode-OR (D1, D2): higher wins, no back-feed |
| Board back-feeding the host | D2 blocks into VBUS |
| Host drives voltage into an external pin | PRTR5V0U2X clamps (D7 UART, D10 to D13 GPIO, D8/D9 SWD) |
| Voltage driven into 3V3_REF | D14 Schottky blocks (reverse 40 V) |

## 5. Boot, reset and programming

**Boot source [recalled from RM0444, not re-read].** With the factory option bytes (`nBOOT_SEL = 1`, `nBOOT0 = 1`) the BOOT0 pin is ignored. The device boots main flash if it holds valid code, and the ROM bootloader if flash is blank. So a board with no firmware lands in the system bootloader by itself.

On this 20-pin package BOOT0 is shared with PA14/SWCLK. I tied it explicitly:
- `R8` 100k pull-down on `SWCLK_MCU`.
- `Q1` (IRLML6244, SOT-23), driven by the CH340C RTS line, holds the node low through `R7` 1k whenever RTS is idle (high).
- `R5` 100k holds the gate high, so a floating RTS still gives BOOT0 low.

When RTS is asserted (low), `Q1` turns off and `R6` 10k pulls the node up: 3.3 V x 100k / (10k + 1k + 100k) = 2.97 V, above V_IH (0.7 x 3.3 = 2.31 V). This only matters if firmware sets `nBOOT_SEL = 0`, to make the pin effective. With a debug probe on J4 driving SWCLK high through R14 100R while Q1 is on: 3.3 V x 1k / 1.1k = 3.0 V at the pin, so SWCLK is still readable.

**Reset.** `R3` 10k pull-up and `C8` 100 nF to ground, plus the `SW1` button. DTR from the CH340C pulls NRST low through `R4` 1k: with NRST pull-up 10k, V(NRST) is about 0.1 V + 3.2 V x 1k / 11k = 0.39 V, below V_IL (0.3 x 3.3 = 0.99 V).

**Programming without firmware.**
1. SWD: connect an ST-LINK or J-Link to J4 (pin 1 3V3_REF, 2 SWDIO, 3 SWCLK, 4 GND, 5 NRST). This works on a blank part.
2. USB-C: plug into J2, the CH340C enumerates, and the blank MCU is in its ROM bootloader at 115200 8E1 on USART1 (PA9/PA10). Load with `stm32flash` or STM32CubeProgrammer in UART mode. USART1 on PA9/PA10 as a bootloader port is **[recalled]** from AN2606 and not confirmed.

Automatic reset via DTR is implemented. Automatic bootloader entry via RTS only works after the option byte is changed. Operating-system support for the CH340C is not verified here.

## 6. I2C bus

| Device | Address | How set |
|---|---|---|
| BME680 | 0x76 | SDO (pin 5) to GND; CSB (pin 2) to +3V3 selects I2C |
| 24LC256 | 0x50 | A2:A0 (pins 3, 2, 1) to GND; WP (pin 7) to GND, writes enabled |

No collision. BME680 VDD (8) and VDDIO (6) are both on +3V3 with a 100 nF each; both GND pins (1, 7) are grounded.

Pull-ups: exactly one pair, `R9` on SDA and `R10` on SCL, 4.7k to +3V3.

- Rise time t_r = 0.8473 x R x C. For bus capacitance C_b of about 45 pF (BME680 5, 24LC256 7, MCU 5, traces about 25): t_r = 0.8473 x 4.7k x 45 pF = 179 ns.
- Fast-mode limit is 300 ns, so C_b up to 300 ns / (0.8473 x 4.7k) = 75 pF is allowed; 100 kHz allows 1000 ns.
- Sink current at V_OL 0.4 V: (3.3 - 0.4) / 4.7k = 0.62 mA, under the 3 mA limit.
- Pull-up current when both lines are low: 2 x 0.70 mA = 1.4 mA.

**Thermal isolation of the BME680.** `U3` is placed at (80, 55) mm in the bottom-right corner. Distances: regulator U1 at (30, 12) is about 66 mm away; MCU U2 at (50, 32) is about 38 mm away; the CH340C at (30, 44) is about 51 mm away. The only parts near it are the EEPROM, its pull-ups and the 4 mounting holes. No slot or keep-out was cut (Synth has no way to place a board slot). Heat in the BME680 itself comes from its gas heater.

## 7. UART break-out and 3V3_REF

J3 pins: 1 GND, 2 UART_TX (MCU out to host), 3 UART_RX (host to MCU in), 4 3V3_REF.

- **UART_TX:** 330R series. A host that shorts the line to GND draws 3.3 V / 330 = 10 mA (pin limit 25 mA [recalled]).
- **UART_RX:** 1k series. Host driving 5.5 V: the clamp (`D7` PRTR5V0U2X, rail to +3V3) holds the line near 3.3 V + 0.7 V = 4.0 V, so the MCU sees at most about 4.0 V and (5.5 - 4.0) / 1k = 1.5 mA flows into +3V3. Host driving -5 V: clamp to GND - 0.7 V, current (5 - 0.7) / 1k = 4.3 mA.
- **Speed:** R x C = 1k x about 11 pF = 11 ns against a 1 us bit at 1 Mbaud. No loading problem.
- **Required host level:** 3.3 V logic. 5 V logic is survived (clamped, 1.5 mA) but not specified.
- **3V3_REF (J3.4, J4.1, J5.1):** `D14` Schottky from +3V3, then `R23` 100R 0805. It is a **sense/reference output, not a supply**. Short-circuit current (3.3 - 0.3) / 100 = 30 mA at 90 mW, inside the 125 mW rating. Output at 3 mA is about 2.7 V. A voltage driven into the pin is blocked by D14 (40 V reverse).

## 8. Registry defects found and what I did

| Registry part | Defect | Action |
|---|---|---|
| `stm32g030` | description says TSSOP-20, footprint is LQFP-32, symbol is the K8 (32-pin), MPN is C8T6 (48-pin part); only 7 pins | authored `stm32g030f6p6` from the KiCad `STM32G030F6Px` symbol (20 pins) with TSSOP-20 footprint |
| `24lc256_eeprom` | 4 pins only; VDD at pin 1, GND at pin 2, SDA 3, SCL 4 (real part: A0 1, A1 2, A2 3, VSS 4, SDA 5, SCL 6, WP 7, VCC 8) | authored `24lc256_soic8` |
| `bme680_env` | footprint is `Bosch_LGA-8_2.5x2.5mm_P0.65mm`, which is the BME280/BMP280 land pattern; BME680 is 3 x 3 mm (KiCad `Bosch_LGA-8_3x3mm_P0.8mm` cites the BME680 datasheet) | authored `bme680_lga8` |
| `ch340g` | needs a 12 MHz crystal and two load capacitors | authored `ch340c` (internal oscillator) |
| `tvs_array_5v` | description says SOIC-8, footprint is SOT-363 (6 pins) and pin 8 exists | not used |
| `jst_xh_2pin`, `jst_xh_4pin` | description says 2.0 mm pitch, footprint is 2.50 mm | used XH 2-pin; footprint (2.50 mm) is the correct one for S2B-XH-A |
| `d_schottky` | MPN 1N5819 is a DO-41 through-hole part; footprint is SOD-123 (1N5819HW is the SOD-123 variant) | used, flagged in the verification report |

Also: `synth_author_part` accepted a pin that declared `usb_dp` with electrical type `bidirectional`, wrote it to disk, and every later tool then failed with "Component registry not found at path ..." until I fixed the type. The message named the wrong problem.

## 9. Placement and routing

Board outline written by Synth: x -1.75 to 90.80, y 0.73 to 63.27, so **92.55 x 62.54 mm**, 2 layers, 1.57 mm.

Regions (sidecar, mm): A power input on the left edge (J1 at 6,17, J2 at 4.5,42, U1 at 30,12); B logic centre (U2 at 50,32, U4 at 30,44); C sensing bottom-right (U3 80,55, U5 80,44); D interfaces (J3 right edge 85,22; J4 bottom edge 50,60; J5 top edge 56,4); four M2.5 holes in the corners. Every part is hard-positioned in the sidecar because the auto-placer failed to place the set (`E-SYNTH-PLACE-002`) on a 78 x 53 mm area that the parts only fill to about a quarter.

Net classes, declared in the source and written into the `.kicad_pcb`:

| Class | Width | Clearance | Via | Nets |
|---|---|---|---|---|
| PWR | 0.5 mm | 0.2 mm | 0.8 / 0.4 | VIN_5V_RAW, VIN_5V_FUSED, +5V_PROT, VBUS_USB, VBUS_FUSED |
| V33 | 0.4 mm | 0.2 mm | 0.8 / 0.4 | +3V3, REF_D, 3V3_REF |
| GNDC | 0.4 mm | 0.2 mm | 0.8 / 0.4 | GND |
| SIG | 0.25 mm | 0.2 mm | 0.8 / 0.4 | all other single-ended nets |
| USBD | 0.3 mm | 0.2 mm | 0.8 / 0.4 | USB_DP, USB_DN |

Width sizing (IPC-2221, external, 35 um, 10 C rise): 0.5 A needs about 0.12 mm. I chose 0.5 mm for margin and about 1 mohm/mm resistance. The USB-C 0.5 mm pad pitch forces the router to neck down to 0.2 mm near the pads.

See the verification report for what the routed copper actually looks like (declared widths and via sizes were **not** all applied).
