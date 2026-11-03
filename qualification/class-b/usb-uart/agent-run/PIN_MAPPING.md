# Pin mapping

Net names from `../usb_uart.synth`; pin numbers from `registry/parts/**`. Datasheet = `provenance.datasheet_url` in the part file. Pin-level datasheet page references: not verified (not recorded).

Named nets: VBUS, +3V3, VIO, GND. Connect-statement nets are named in the Net column. Unnamed in the source and not connected: U1 RST (9), U1 RTS0 (19), J1 SBU1/SBU2 (A8/B8).

## U1 CH342F (QFN-24-1EP) - datasheet https://www.wch-ic.com/downloads/CH342DS1_PDF.html (V1E per README.md)
| Pin | Name | Net |
| --- | --- | --- |
| 1 | RI0 | MODEM_IDLE (R8 to VIO) |
| 2 | GND | GND |
| 3 | UD+ | USB_DP |
| 4 | UD- | USB_DN |
| 5 | VIO | VIO (C6) |
| 6 | V3 | V3 (C5 only) |
| 7 | VDD5 | VBUS (C4) |
| 8 | VBUS | VBUS |
| 9 | RST | not connected (internal pull-up per README.md) |
| 10 | CTS1 | CTS1_MCU (R6 to J3.3) |
| 11 | RTS1 | RTS1 (J3.2) |
| 12 | RXD1 | RXD1_MCU (R5 to J2.3) |
| 13 | TXD1 | TXD1 (J2.2, D7 cathode) |
| 14 | DSR1/TNOW1 | MODEM_IDLE |
| 15 | DTR1/TNOW1 | DTR1 (J3.1) |
| 16 | DCD1 | MODEM_IDLE |
| 17 | RI1 | MODEM_IDLE |
| 18 | CTS0 | CTS0_LOW (R9 to GND) |
| 19 | RTS0 | not connected |
| 20 | RXD0 | RXD0 (U3.RO) |
| 21 | TXD0 | TXD0 (U3.DI, D4 cathode) |
| 22 | DSR0 | MODEM_IDLE |
| 23 | DTR0/TNOW0 | TNOW0 (U3.DE, U3./RE, R_TNOW) |
| 24 | DCD0 | MODEM_IDLE |
| 25 | EP | GND (via-in-pad at about (23.9, 17.3) mm per README.md) |

## U3 THVD1450D (SOIC-8) - datasheet https://www.ti.com/lit/gpn/THVD1450 (README.md: SLLSEY3E)
| Pin | Name | Net |
| --- | --- | --- |
| 1 | RO | RXD0 |
| 2 | /RE | TNOW0 |
| 3 | DE | TNOW0 |
| 4 | DI | TXD0 |
| 5 | GND | GND |
| 6 | A | RS485_A (J4.1, R_TERM.1) |
| 7 | B | RS485_B (J4.2, R_TERM.2) |
| 8 | VCC | VIO (C7) |

## U2 LP5907MFX-3.3 (SOT-23-5) - datasheet https://www.ti.com/lit/ds/symlink/lp5907.pdf (README.md: SNVS798Q); part file records no datasheet_url
| Pin | Name | Net |
| --- | --- | --- |
| 1 | VIN | VBUS (C2) |
| 2 | GND | GND |
| 3 | EN | VBUS |
| 4 | NC | not connected |
| 5 | VOUT | +3V3 (C3, J_VIO.1) |

## J1 USB-C (TYPE-C-31-M-12) - no datasheet in the registry (README.md: "none in the registry, not checked")
| Pin | Name | Net |
| --- | --- | --- |
| A4, B4, A9, B9 | VBUS | VBUS |
| A1, A12, B1, B12, SH | GND, shield | GND |
| A6, B6 | D+ | USB_DP |
| A7, B7 | D- | USB_DN |
| A5 | CC1 | CC1 (R1, D5) |
| B5 | CC2 | CC2 (R2, D6) |
| A8, B8 | SBU1, SBU2 | not connected |

## ESD parts
- D1 PRTR5V0U2X (SOT-143), https://assets.nexperia.com/documents/data-sheet/PRTR5V0U2X.pdf: pin 1 GND -> GND; pin 2 IO1 -> USB_DP; pin 3 IO2 -> USB_DN; pin 4 VCC -> VBUS.
- D2, D5, D6 PTVS6V0S1UR, https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf: pin 1 cathode / pin 2 anode. D2 cathode VBUS, D5 cathode CC1, D6 cathode CC2; all anodes GND.

## Headers (header_1x3, passive pins, no datasheet)
| Ref | Pin 1 | Pin 2 | Pin 3 |
| --- | --- | --- | --- |
| J_VIO | +3V3 | VIO | VBUS |
| J2 UART1 | GND | TXD1 | RXD1 (via R5) |
| J3 FLOW1 | DTR1 | RTS1 | CTS1 (via R6) |
| J4 RS485 | RS485_A | RS485_B | GND |

## LEDs (cathode pin 1, anode pin 2)
D3 LTST-C190KGKT: anode LED_PWR (R3 from VIO), cathode GND. D4 LTST-C190KRKT: anode LED_TX0 (R4 from VIO), cathode TXD0. D7 LTST-C190KRKT: anode LED_TX1 (R7 from VIO), cathode TXD1. Datasheets: URLs in README.md BOM (revision not checked).
