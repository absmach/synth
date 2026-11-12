# PIN_MAPPING: rtd-afe

Source: `rtd_afe.synth` nets and `connect` lines, pin numbers from `registry/parts/**`. "Datasheet" is the URL recorded in the part file's provenance, or "none recorded". Pins not listed are open (not connected in the `.synth`). Whether each pin function matches the datasheet beyond what QUALIFICATION.md states: not verified.

## U1 ADS124S08IRHBR (VQFN-32, registry `ic/ads124s08`)
Datasheet: https://www.ti.com/lit/ds/symlink/ads124s08.pdf (reviewed_by Steve Munene 2026-10-27; QUALIFICATION.md says 13 of 33 pins checked by a pin test).
| Pin | Name | Net |
| --- | --- | --- |
| 5 | AIN2 | AIN2 (RTD S- after R_S2) |
| 6 | AIN1 | AIN1 (RTD S+ after R_S1) |
| 7 | AIN0 | RTD_FP (IDAC1 force out, J2.1) |
| 8 | START/SYNC | ADC_START (U2 GPIO5, pin 8) |
| 9 | CS | SPI_CS (U2 GPIO1) |
| 10 | DIN | SPI_MOSI (U2 GPIO3) |
| 11 | SCLK | SPI_SCK (U2 GPIO2) |
| 12 | DOUT/DRDY | SPI_MISO (U2 GPIO0) |
| 13 | DRDY | ADC_DRDY (U2 GPIO4) |
| 14 | DGND | GND |
| 15 | IOVDD | +3V3 |
| 16 | DVDD | +3V3 |
| 17 | CLK | GND (internal oscillator) |
| 18 | RESET | ADC_RESET_N (U2 GPIO6; R_RST 10k to +3V3) |
| 23 | REFOUT | REFOUT (C_RO 1 uF to GND; internal reference unused) |
| 24 | REFCOM | GND |
| 26 | AVDD | +3V3A |
| 27 | AVSS | GND |
| 28 | AVSS_SW | GND |
| 29 | REFN0 | GND |
| 30 | REFP0 | VREF |
| 33 | EP | GND |
Open: 1 AINCOM, 2 AIN5, 3 AIN4, 4 AIN3, 19-22 GPIO3..0/AIN11..8, 25 NC, 31 REFN1/AIN7, 32 REFP1/AIN6.

## U3 REF5025AIDR (SOIC-8, registry `ic/ref5025`)
Datasheet: https://www.ti.com/lit/ds/gpn/ref5025 (reviewed_by Steve Munene 2026-10-27; 5 of 8 pins checked).
| Pin | Name | Net |
| --- | --- | --- |
| 2 | VIN | +3V3A |
| 4 | GND | GND |
| 6 | VOUT | VREF (to U1 REFP0, C_RV) |
Open: 1 DNC, 3 TEMP, 5 TRIM/NR, 7 NC, 8 DNC.

## U2 RP2350A (QFN-60, registry `mcus/rp2350`)
Datasheet: registry file records none in the part's provenance; QUALIFICATION.md cites https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf "from the KiCad symbol; not opened in this work". Pin numbers were compared against the KiCad `MCU_RaspberryPi:RP2350A` symbol; part not reviewed.
| Pin | Name | Net |
| --- | --- | --- |
| 1, 11, 20, 30, 38, 45 | IOVDD..IOVDD6 | +3V3 |
| 2 | GPIO0 | SPI_MISO |
| 3 | GPIO1 | SPI_CS |
| 4 | GPIO2 | SPI_SCK |
| 5 | GPIO3 | SPI_MOSI |
| 6, 23, 39 | DVDD, DVDD3, DVDD2 | DVDD_1V1 |
| 7 | GPIO4 | ADC_DRDY |
| 8 | GPIO5 | ADC_START |
| 9 | GPIO6 | ADC_RESET_N |
| 21 | XIN | XIN |
| 22 | XOUT | XOUT (R_X 1k to XOUT_R) |
| 24 | SWCLK | SWCLK (J3.1) |
| 25 | SWDIO | SWDIO (J3.2) |
| 26 | RUN | RUN (R_RUN 10k to +3V3) |
| 44 | ADC_AVDD | +3V3_RP_A |
| 46 | VREG_AVDD | +3V3_RP_A |
| 47 | VREG_PGND | GND |
| 48 | VREG_LX | VREG_LX (L1) |
| 49 | VREG_VIN | +3V3 |
| 50 | VREG_FB | DVDD_1V1 |
| 51 | USB_DM | USB_DN (via R_DN 27R) |
| 52 | USB_DP | USB_DP (via R_DP 27R) |
| 53 | USB_OTP_VDD | +3V3 |
| 54 | QSPI_IOVDD | +3V3 |
| 55 | QSPI_SD3 | QSPI_SD3 |
| 56 | QSPI_SCLK | QSPI_SCLK |
| 57 | QSPI_SD0 | QSPI_SD0 |
| 58 | QSPI_SD2 | QSPI_SD2 |
| 59 | QSPI_SD1 | QSPI_SD1 |
| 60 | QSPI_SS | QSPI_SS |
| 61 | GND (EP) | GND |
Open: GPIO7..GPIO29 and remaining pins (not connected in the `.synth`).

## U4 W25Q128JVSIQ (SOIC-8, registry `memory/w25q128_flash`)
Datasheet: none recorded in the registry; not reviewed. Pin map from the registry.
| Pin | Name | Net |
| --- | --- | --- |
| 1 | CS | QSPI_SS |
| 2 | IO1 | QSPI_SD1 |
| 3 | IO2 | QSPI_SD2 |
| 4 | GND | GND |
| 5 | IO0 | QSPI_SD0 |
| 6 | CLK | QSPI_SCLK |
| 7 | IO3 | QSPI_SD3 |
| 8 | VCC | +3V3 |

## U5 LP5907MFX-3.3 (SOT-23-5, registry `regulators/lp5907_3v3`)
Datasheet: https://www.ti.com/lit/ds/symlink/lp5907.pdf (reviewed_by Steve Munene 2026-10-27).
| Pin | Name | Net |
| --- | --- | --- |
| 1 | VIN | VBUS |
| 2 | GND | GND |
| 3 | EN | VBUS (always enabled) |
| 5 | VOUT | +3V3 |
Open: 4 (modelled NC).

## J1 USB-C receptacle HRO TYPE-C-31-M-12 (registry `connectors/usb_c_receptacle`)
Datasheet: none recorded; LCSC C165948; not reviewed.
| Pins | Name | Net |
| --- | --- | --- |
| A4, B4, A9, B9 | VBUS | VBUS |
| A1, A12, B1, B12, SH | GND, shield | GND |
| A5 | CC1 | CC1 (R_CC1 5.1k to GND) |
| B5 | CC2 | CC2 (R_CC2 5.1k to GND) |
| A6, B6 | DP | USB_DP_C (D1, R_DP) |
| A7, B7 | DN | USB_DN_C (D2, R_DN) |
Open: A8 SBU1, B8 SBU2.

## J2 PT100 header 1x4 (registry `header_1x4`, no datasheet)
| Pin | Function | Net |
| --- | --- | --- |
| 1 | F+ | RTD_FP (U1 AIN0, C_F) |
| 2 | S+ | RTD_SP (R_S1) |
| 3 | S- | RTD_SN (R_S2) |
| 4 | F- | RTD_FN (R_BIAS to GND) |

## J3 SWD header 1x3 (registry `header_1x3`, no datasheet)
| Pin | Function | Net |
| --- | --- | --- |
| 1 | SWCLK | SWCLK (U2.24) |
| 2 | SWDIO | SWDIO (U2.25) |
| 3 | GND | GND |
No RESET/VCC pin on J3.

## Y1 ECS-120-20-33-TR (registry `crystals/ecs_120_20_33_tr`)
Datasheet: https://ecsxtal.com/store/pdf/ecx-32.pdf (not reviewed; registry description: pads 1 and 3 crystal, 2 and 4 ground).
| Pin | Net |
| --- | --- |
| 1 | XIN (C_X1 33 pF to GND) |
| 3 | XOUT_R (C_X2 33 pF to GND) |
| 2, 4 | GND |

## D1, D2 `esd_usb` (SOD-323, generic; no datasheet, no MPN)
D1: pin 1 IO to USB_DP_C, pin 2 GND. D2: pin 1 IO to USB_DN_C, pin 2 GND.

## L1, L2
L1 (3.3 uH, 0805): p1 VREG_LX, p2 DVDD_1V1. L2 (ferrite 600R@100MHz, 0603): p1 +3V3, p2 +3V3A. No datasheets.
