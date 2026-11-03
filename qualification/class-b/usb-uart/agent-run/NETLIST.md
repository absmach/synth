# Netlist

From `../usb_uart.synth` (net blocks and connect statements). Not re-exported.

| Net | Members |
| --- | --- |
| VBUS | J1.A4/B4/A9/B9, D1.VCC, D2.cathode, U2.VIN, U2.EN, C2.1, U1.VDD5, U1.VBUS, C4.1, J_VIO.3 |
| +3V3 | U2.VOUT, C3.1, J_VIO.1 |
| VIO | J_VIO.2, U1.VIO, C6.1, U3.VCC, C7.1, R3.1, R8.1, R4.1, R7.1 |
| GND | J1 GND pins and shield, R1.2, R2.2, D1.GND, D2/D5/D6 anodes, C2-C7 pin 2, U2.GND, U1.GND, U1.EP, U3.GND, J2.1, J4.3, R9.2, R_TNOW.2, D3.cathode |
| CC1 | J1.CC1, R1.1, D5.cathode |
| CC2 | J1.CC2, R2.1, D6.cathode |
| USB_DP | J1.A6, J1.B6, D1.IO1, U1.UD+ (class USB) |
| USB_DN | J1.A7, J1.B7, D1.IO2, U1.UD- (class USB) |
| V3 | U1.V3, C5.1 |
| MODEM_IDLE | U1.RI0, RI1, DCD0, DCD1, DSR0, DSR1/TNOW1, R8.2 |
| CTS0_LOW | U1.CTS0, R9.1 |
| TXD1 | U1.TXD1, J2.2, D7.cathode |
| RXD1 | J2.3, R5.2 |
| RXD1_MCU | R5.1, U1.RXD1 |
| DTR1 | U1.DTR1/TNOW1, J3.1 |
| RTS1 | U1.RTS1, J3.2 |
| CTS1 | J3.3, R6.2 |
| CTS1_MCU | R6.1, U1.CTS1 |
| TXD0 | U1.TXD0, U3.DI, D4.cathode |
| RXD0 | U1.RXD0, U3.RO |
| TNOW0 | U1.DTR0/TNOW0, U3.DE, U3./RE, R_TNOW.1 |
| RS485_A | U3.A, J4.1, R_TERM.1 |
| RS485_B | U3.B, J4.2, R_TERM.2 |
| LED_PWR | R3.2, D3.anode |
| LED_TX0 | R4.2, D4.anode |
| LED_TX1 | R7.2, D7.anode |

The release routing record (`../reports/usb_uart.routing.json`) reports 26 connected nets, 0 open, 273 segments, 43 vias. README.md notes that U1 RST and RTS0 are left open. ERC (`../reports/usb_uart.erc.json`): 1 warning, label_multiple_wires (MODEM_IDLE), dispositioned benign in `../qualification.json`.

Validate warnings listed in README.md (not re-run): E-SYNTH-POWER-004 x2, E-SYNTH-SCHEM-005 x2, W-SYNTH-ANOMALY-001, W-SYNTH-SUPPLY-002 x8, W-SYNTH-PART-UNVERIFIED x9.
