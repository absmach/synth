# NETLIST: rtd-afe

Source: `rtd_afe.synth`. 40 nets (`reports/rtd_afe.connectivity.json`: 40 connected, 0 open). Passive pin 1 / pin 2 are `p1` / `p2` in the source.

## Power and ground nets
| Net | Members |
| --- | --- |
| VBUS | J1 A4/B4/A9/B9, U5.1 VIN, U5.3 EN, C_L1.p1 |
| +3V3 | U5.5 VOUT, C_L2.p1, L2.p1, R_AV.p1, U1.15/16, C_DV.p1, C_IO.p1, R_RST.p1, U2 IOVDD x6 (1,11,20,30,38,45), U2.54, U2.53, U2.49, C_IO1..C_IO4.p1, C_VI.p1, U4.8, C_FL.p1, R_RUN.p1 |
| +3V3A | L2.p2, C_AV1.p1, C_AV2.p1, U1.26 AVDD, U3.2 VIN, C_RI.p1 |
| +3V3_RP_A | R_AV.p2, C_AVF.p1, U2.46, U2.44 |
| DVDD_1V1 | L1.p2, U2.6/23/39, U2.50, C_DV1.p1, C_DV2.p1, C_DVB.p1 |
| VREG_LX | U2.48, L1.p1 |
| VREF | U3.6, C_RV.p1, U1.30 |
| GND | U1 14/17/24/27/28/29/33, C_RO.p2, C_DV.p2, C_IO.p2, U3.4, C_RI.p2, C_RV.p2, C_AV1.p2, C_AV2.p2, C_CM1.p2, C_CM2.p2, C_F.p2, R_BIAS.p2, U2.61, U2.47, C_IO1..4.p2, C_DV1/2/B.p2, C_VI.p2, C_AVF.p2, C_X1.p2, C_X2.p2, Y1.2/4, U4.4, C_FL.p2, J3.3, U5.2, C_L1.p2, C_L2.p2, J1 A1/A12/B1/B12/SH, R_CC1.p2, R_CC2.p2, D1.2, D2.2 |

## Signal nets
| Net | Members |
| --- | --- |
| RTD_FP | J2.1, U1.7 (AIN0), C_F.p1 |
| RTD_SP | J2.2, R_S1.p1 |
| AIN1 | R_S1.p2, U1.6, C_D.p1, C_CM1.p1 |
| RTD_SN | J2.3, R_S2.p1 |
| AIN2 | R_S2.p2, U1.5, C_D.p2, C_CM2.p1 |
| RTD_FN | J2.4, R_BIAS.p1 |
| REFOUT | U1.23, C_RO.p1 |
| SPI_MOSI | U2.5 (GPIO3), U1.10 |
| SPI_CS | U2.3 (GPIO1), U1.9 |
| SPI_SCK | U2.4 (GPIO2), U1.11 |
| SPI_MISO | U2.2 (GPIO0), U1.12 |
| ADC_DRDY | U2.7 (GPIO4), U1.13 |
| ADC_START | U2.8 (GPIO5), U1.8 |
| ADC_RESET_N | U2.9 (GPIO6), U1.18, R_RST.p2 |
| XIN | U2.21, Y1.1, C_X1.p1 |
| XOUT | U2.22, R_X.p1 |
| XOUT_R | R_X.p2, Y1.3, C_X2.p1 |
| RUN | U2.26, R_RUN.p2 |
| SWCLK | U2.24, J3.1 |
| SWDIO | U2.25, J3.2 |
| QSPI_SS | U2.60, U4.1 |
| QSPI_SCLK | U2.56, U4.6 |
| QSPI_SD0 | U2.57, U4.5 |
| QSPI_SD1 | U2.59, U4.2 |
| QSPI_SD2 | U2.58, U4.3 |
| QSPI_SD3 | U2.55, U4.7 |
| CC1 | J1.A5, R_CC1.p1 |
| CC2 | J1.B5, R_CC2.p1 |
| USB_DP_C (class USB) | J1.A6, J1.B6, D1.1, R_DP.p1 |
| USB_DP (class USB) | R_DP.p2, U2.52 |
| USB_DN_C (class USB) | J1.A7, J1.B7, D2.1, R_DN.p1 |
| USB_DN (class USB) | R_DN.p2, U2.51 |

## Cross-checks (this reconstruction)
- 8 power/ground + 7 RTD/ref + 7 SPI/ADC control + 6 MCU support + 6 QSPI + 2 CC + 4 USB = 40 nets, matching the connectivity report.
- Source comments flag `E-SYNTH-CONNECT-007` on SPI_MISO and ADC_DRDY (bidirectional MCU pin joined to ADC output); QUALIFICATION.md calls it normal SPI wiring.
- The KiCad netlist `reports/rtd_afe.net` is the exported counterpart; no diff against this table was run: not verified.
