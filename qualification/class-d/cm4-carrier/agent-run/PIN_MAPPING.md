# PIN_MAPPING — CM4 carrier (SYN-407)

Every used pin of the CM4 module interface is traced to Raspberry Pi Compute
Module 4 datasheet Table 6 (pages 16–22). `J_CM4A` is CM4 pins 1–100; pad `n`
on `J_CM4A` is CM4 pin `n`. `J_CM4B` is CM4 pins 101–200; pad `n` on
`J_CM4B` is CM4 pin `100 + n`.

The two connector parts are authored with pin names `p1`…`p100`; the netlist
uses those names, and this table is the authoritative mapping to the CM4
signal names. The CM4 pins that are not in the table are NC and are listed
below.

| Connector | Pad | CM4 pin | CM4 signal (Table 6) | Net | Table 6 page |
|---|---|---|---|---|---|
| J_CM4A | 1 | 1 | GND | `GND` | 16 |
| J_CM4A | 2 | 2 | GND | `GND` | 16 |
| J_CM4A | 3 | 3 | Ethernet_Pair3_P | `ETH3_P` | 16 |
| J_CM4A | 4 | 4 | Ethernet_Pair1_P | `ETH1_P` | 16 |
| J_CM4A | 5 | 5 | Ethernet_Pair3_N | `ETH3_N` | 16 |
| J_CM4A | 6 | 6 | Ethernet_Pair1_N | `ETH1_N` | 16 |
| J_CM4A | 7 | 7 | GND | `GND` | 16 |
| J_CM4A | 8 | 8 | GND | `GND` | 16 |
| J_CM4A | 9 | 9 | Ethernet_Pair2_N | `ETH2_N` | 16 |
| J_CM4A | 10 | 10 | Ethernet_Pair0_N | `ETH0_N` | 16 |
| J_CM4A | 11 | 11 | Ethernet_Pair2_P | `ETH2_P` | 16 |
| J_CM4A | 12 | 12 | Ethernet_Pair0_P | `ETH0_P` | 16 |
| J_CM4A | 13 | 13 | GND | `GND` | 16 |
| J_CM4A | 14 | 14 | GND | `GND` | 16 |
| J_CM4A | 15 | 15 | Ethernet_nLED3 | `ETH_LED_ACT` | 16 |
| J_CM4A | 17 | 17 | Ethernet_nLED2 | `ETH_LED_SPD` | 16 |
| J_CM4A | 21 | 21 | Pi_nLED_Activity | `PI_LED_ACT` | 16 |
| J_CM4A | 22 | 22 | GND | `GND` | 16 |
| J_CM4A | 23 | 23 | GND | `GND` | 16 |
| J_CM4A | 24 | 24 | GPIO26 | `GPIO26` | 16 |
| J_CM4A | 25 | 25 | GPIO21 | `GPIO21` | 16 |
| J_CM4A | 26 | 26 | GPIO19 | `GPIO19` | 16 |
| J_CM4A | 27 | 27 | GPIO20 | `GPIO20` | 16 |
| J_CM4A | 28 | 28 | GPIO13 | `GPIO13` | 17 |
| J_CM4A | 29 | 29 | GPIO16 | `GPIO16` | 17 |
| J_CM4A | 30 | 30 | GPIO6 | `GPIO6` | 17 |
| J_CM4A | 31 | 31 | GPIO12 | `GPIO12` | 17 |
| J_CM4A | 32 | 32 | GND | `GND` | 17 |
| J_CM4A | 33 | 33 | GND | `GND` | 17 |
| J_CM4A | 34 | 34 | GPIO5 | `GPIO5` | 17 |
| J_CM4A | 35 | 35 | ID_SC | `ID_SC` | 17 |
| J_CM4A | 36 | 36 | ID_SD | `ID_SD` | 17 |
| J_CM4A | 37 | 37 | GPIO7 | `GPIO7` | 17 |
| J_CM4A | 38 | 38 | GPIO11 | `GPIO11` | 17 |
| J_CM4A | 39 | 39 | GPIO8 | `GPIO8` | 17 |
| J_CM4A | 40 | 40 | GPIO9 | `GPIO9` | 17 |
| J_CM4A | 41 | 41 | GPIO25 | `GPIO25` | 17 |
| J_CM4A | 42 | 42 | GND | `GND` | 17 |
| J_CM4A | 43 | 43 | GND | `GND` | 17 |
| J_CM4A | 44 | 44 | GPIO10 | `GPIO10` | 17 |
| J_CM4A | 45 | 45 | GPIO24 | `GPIO24` | 17 |
| J_CM4A | 46 | 46 | GPIO22 | `GPIO22` | 17 |
| J_CM4A | 47 | 47 | GPIO23 | `GPIO23` | 17 |
| J_CM4A | 48 | 48 | GPIO27 | `GPIO27` | 17 |
| J_CM4A | 49 | 49 | GPIO18 | `GPIO18` | 17 |
| J_CM4A | 50 | 50 | GPIO17 | `GPIO17` | 17 |
| J_CM4A | 51 | 51 | GPIO15 | `GPIO15` | 17 |
| J_CM4A | 52 | 52 | GND | `GND` | 17 |
| J_CM4A | 53 | 53 | GND | `GND` | 17 |
| J_CM4A | 54 | 54 | GPIO4 | `GPIO4` | 18 |
| J_CM4A | 55 | 55 | GPIO14 | `GPIO14` | 18 |
| J_CM4A | 56 | 56 | GPIO3 | `GPIO3` | 18 |
| J_CM4A | 57 | 57 | SD_CLK | `SD_CLK` | 18 |
| J_CM4A | 58 | 58 | GPIO2 | `GPIO2` | 18 |
| J_CM4A | 59 | 59 | GND | `GND` | 18 |
| J_CM4A | 60 | 60 | GND | `GND` | 18 |
| J_CM4A | 61 | 61 | SD_DAT3 | `SD_DAT3` | 18 |
| J_CM4A | 62 | 62 | SD_CMD | `SD_CMD` | 18 |
| J_CM4A | 63 | 63 | SD_DAT0 | `SD_DAT0` | 18 |
| J_CM4A | 65 | 65 | GND | `GND` | 18 |
| J_CM4A | 66 | 66 | GND | `GND` | 18 |
| J_CM4A | 67 | 67 | SD_DAT1 | `SD_DAT1` | 18 |
| J_CM4A | 69 | 69 | SD_DAT2 | `SD_DAT2` | 18 |
| J_CM4A | 71 | 71 | GND | `GND` | 18 |
| J_CM4A | 74 | 74 | GND | `GND` | 18 |
| J_CM4A | 75 | 75 | SD_PWR_ON | `SD_PWR_ON` | 18 |
| J_CM4A | 77 | 77 | +5V | `+5V` | 18 |
| J_CM4A | 78 | 78 | GPIO_VREF | `GPIO_VREF` | 18 |
| J_CM4A | 79 | 79 | +5V | `+5V` | 18 |
| J_CM4A | 80 | 80 | SCL0 | `SCL0` | 18 |
| J_CM4A | 81 | 81 | +5V | `+5V` | 18 |
| J_CM4A | 82 | 82 | SDA0 | `SDA0` | 18 |
| J_CM4A | 83 | 83 | +5V | `+5V` | 18 |
| J_CM4A | 84 | 84 | CM4_3.3V | `CM4_3V3` | 18 |
| J_CM4A | 85 | 85 | +5V | `+5V` | 18 |
| J_CM4A | 86 | 86 | CM4_3.3V | `CM4_3V3` | 18 |
| J_CM4A | 87 | 87 | +5V | `+5V` | 18 |
| J_CM4A | 88 | 88 | CM4_1.8V | `CM4_1V8` | 19 |
| J_CM4A | 90 | 90 | CM4_1.8V | `CM4_1V8` | 19 |
| J_CM4A | 92 | 92 | RUN_PG | `RUN_PG` | 19 |
| J_CM4A | 93 | 93 | nRPIBOOT | `NRPIBOOT` | 19 |
| J_CM4A | 95 | 95 | PI_LED_nPWR | `PI_LED_NPWR` | 19 |
| J_CM4A | 97 | 97 | Camera_GPIO | `CAM_GPIO` | 19 |
| J_CM4A | 98 | 98 | GND | `GND` | 19 |
| J_CM4A | 99 | 99 | GLOBAL_EN | `GLOBAL_EN` | 19 |
| J_CM4A | 100 | 100 | nEXTRST | `NEXTRST` | 19 |
| J_CM4B | 1 | 101 | USB_OTG_ID | `GND` | 19 |
| J_CM4B | 3 | 103 | USB_N | `USB_UP_N` | 19 |
| J_CM4B | 5 | 105 | USB_P | `USB_UP_P` | 19 |
| J_CM4B | 7 | 107 | GND | `GND` | 19 |
| J_CM4B | 8 | 108 | GND | `GND` | 19 |
| J_CM4B | 13 | 113 | GND | `GND` | 19 |
| J_CM4B | 14 | 114 | GND | `GND` | 19 |
| J_CM4B | 19 | 119 | GND | `GND` | 20 |
| J_CM4B | 20 | 120 | GND | `GND` | 20 |
| J_CM4B | 25 | 125 | GND | `GND` | 20 |
| J_CM4B | 26 | 126 | GND | `GND` | 20 |
| J_CM4B | 28 | 128 | CAM0_D0_N | `CAM_D0_N` | 20 |
| J_CM4B | 30 | 130 | CAM0_D0_P | `CAM_D0_P` | 20 |
| J_CM4B | 31 | 131 | GND | `GND` | 20 |
| J_CM4B | 32 | 132 | GND | `GND` | 20 |
| J_CM4B | 34 | 134 | CAM0_D1_N | `CAM_D1_N` | 20 |
| J_CM4B | 36 | 136 | CAM0_D1_P | `CAM_D1_P` | 20 |
| J_CM4B | 37 | 137 | GND | `GND` | 20 |
| J_CM4B | 38 | 138 | GND | `GND` | 20 |
| J_CM4B | 40 | 140 | CAM0_C_N | `CAM_CK_N` | 20 |
| J_CM4B | 42 | 142 | CAM0_C_P | `CAM_CK_P` | 20 |
| J_CM4B | 44 | 144 | GND | `GND` | 20 |
| J_CM4B | 50 | 150 | GND | `GND` | 20 |
| J_CM4B | 51 | 151 | HDMI0_CEC | `HDMI_CEC` | 21 |
| J_CM4B | 53 | 153 | HDMI0_HOTPLUG | `HDMI_HPD` | 21 |
| J_CM4B | 55 | 155 | GND | `GND` | 21 |
| J_CM4B | 56 | 156 | GND | `GND` | 21 |
| J_CM4B | 57 | 157 | DSI0_D0_N | `DSI_D0_N` | 21 |
| J_CM4B | 59 | 159 | DSI0_D0_P | `DSI_D0_P` | 21 |
| J_CM4B | 61 | 161 | GND | `GND` | 21 |
| J_CM4B | 62 | 162 | GND | `GND` | 21 |
| J_CM4B | 63 | 163 | DSI0_D1_N | `DSI_D1_N` | 21 |
| J_CM4B | 65 | 165 | DSI0_D1_P | `DSI_D1_P` | 21 |
| J_CM4B | 67 | 167 | GND | `GND` | 21 |
| J_CM4B | 68 | 168 | GND | `GND` | 21 |
| J_CM4B | 69 | 169 | DSI0_C_N | `DSI_CK_N` | 21 |
| J_CM4B | 70 | 170 | HDMI0_TX2_P | `HDMI_D2_P` | 21 |
| J_CM4B | 71 | 171 | DSI0_C_P | `DSI_CK_P` | 21 |
| J_CM4B | 72 | 172 | HDMI0_TX2_N | `HDMI_D2_N` | 21 |
| J_CM4B | 73 | 173 | GND | `GND` | 21 |
| J_CM4B | 74 | 174 | GND | `GND` | 21 |
| J_CM4B | 76 | 176 | HDMI0_TX1_P | `HDMI_D1_P` | 21 |
| J_CM4B | 78 | 178 | HDMI0_TX1_N | `HDMI_D1_N` | 21 |
| J_CM4B | 79 | 179 | GND | `GND` | 21 |
| J_CM4B | 80 | 180 | GND | `GND` | 21 |
| J_CM4B | 82 | 182 | HDMI0_TX0_P | `HDMI_D0_P` | 21 |
| J_CM4B | 84 | 184 | HDMI0_TX0_N | `HDMI_D0_N` | 21 |
| J_CM4B | 85 | 185 | GND | `GND` | 21 |
| J_CM4B | 86 | 186 | GND | `GND` | 21 |
| J_CM4B | 88 | 188 | HDMI0_CLK_P | `HDMI_CK_P` | 21 |
| J_CM4B | 90 | 190 | HDMI0_CLK_N | `HDMI_CK_N` | 22 |
| J_CM4B | 91 | 191 | GND | `GND` | 22 |
| J_CM4B | 92 | 192 | GND | `GND` | 22 |
| J_CM4B | 97 | 197 | GND | `GND` | 22 |
| J_CM4B | 98 | 198 | GND | `GND` | 22 |
| J_CM4B | 99 | 199 | HDMI0_SDA | `HDMI_SDA_MCU` | 22 |
| J_CM4B | 100 | 200 | HDMI0_SCL | `HDMI_SCL_MCU` | 22 |

## Unused CM4 pins (NC)

These pins are not in any net. The reasons are the block notes in
`cm4_carrier.synth` and the CM4 datasheet:

| CM4 pins | Signal | Why NC |
| --- | --- | --- |
| 16, 18 | Ethernet_SYNC_IN/OUT | IEEE 1588 sync not brought out |
| 19 | Ethernet_nLED1 | third Ethernet LED not used; two board LEDs are driven by nLED2/nLED3 |
| 20 | EEPROM_nWP | internal 100k pull-up; no write-protect jumper |
| 64, 68, 70, 72 | SD_DAT4–7 | microSD socket is 4-bit; these are the eMMC-only upper lanes on CM4 Lite |
| 73 | SD_VDD_OVERRIDE | 3.3 V SD signalling; not tied to CM4_3.3V |
| 76, 104, 106 | Reserved | datasheet says do not connect |
| 89, 91 | WL_nDisable, BT_nDisable | internal pull-ups; wireless not disabled |
| 94, 96 | AnalogIP1/0 | MXL7704 CC sense; 12 V input has no USB-C CC |
| 102, 109, 110, 112, 116, 118, 122, 124 | PCIe | PCIe not used on this carrier |
| 111 | VDAC_COMP | TV-out compensation not used |
| 115, 117, 121, 123, 127, 129, 133, 135, 139, 141 | CAM1 | only CAM0 is brought to an FFC |
| 143, 145, 146, 147, 148, 149, 152, 154, 158, 160, 164, 166 | HDMI1 | only HDMI0 is brought out |
| 175, 177, 181, 183, 187, 189, 193, 194, 195, 196 | DSI1 | only DSI0 is brought to an FFC |

## Other ICs and connectors

The full netlist is in `cm4_carrier.synth` and `NETLIST.md`. The pins that
carry a datasheet decision are:

| Part | Pins | Net | Datasheet |
| --- | --- | --- | --- |
| U1 LMR33640 | 1 GND, 2 VIN, 3 EN, 4 PG, 5 FB, 6 VCC, 7 BOOT, 8 SW, 9 EP | GND, VIN_PROT, EN divider, PG_5V, FB_5V, VCC_5V, BOOT_5V, SW_5V, GND | SNVSB99C pin table; VREF 1.0 V; EN 1.231 V |
| U2/U3 LMR38010 | 1 GND, 2 EN, 3 VIN, 4 RT/SYNC, 5 FB, 6 PG, 7 BOOT, 8 SW, 9 EP | GND, PG_5V/PG_3V3, +5V_AUX, RT_3V3/RT_1V8, FB, PG_3V3/PG_1V8, BOOT, SW, GND | SNVSBM3B |
| U4 USB2514B | 1/2 port1, 3/4 port2, 6/7 port3, 8/9 port4, 30/31 upstream, 12/16/18/20 PRTPWR, 13/17/19/21 OCS_N, 22 SDA, 24 SCL, 25 HS_IND, 26 RESET_N, 27 VBUS_DET, 28 SUSP/LOCAL/NON_REM0, 32/33 crystal, 35 RBIAS, 37 VSS | USB1–4, USB_UP, USB_ON1–4, HUB_OCS1–4, HUB_CFG0/1, HUB_RESET, HUB_VBUS_DET, HUB_LOCAL, HUB_XTAL1/2, HUB_RBIAS, GND | DS00001692C Table 3-1/3-2, Table 5-1 |
| U12 PCF85063ATL | 1 OSCI, 2 OSCO, 3 CLKOE, 4 INT, 5 VSS, 6 SDA, 7 SCL, 8 NC, 9 CLKOUT, 10 VDD, 11 EP | RTC_OSC1/2, RTC_VDD, GPIO26, GND, SDA0, SCL0, NC, NC, RTC_VDD, GND | PCF85063A Rev. 7.3 Table 3 |
| J_ETH JXD6-0001NL | 1/2 pair2, 4/5 pair0, 6/7 pair1, 9/10 pair3, 3 TCT, 8 RCT, SH shield | ETH2, ETH0, ETH1, ETH3, ETH_TCT, ETH_RCT, GND | Pulse product page |
| J_SD 47219-2001 | 1 DAT2, 2 DAT3, 3 CMD, 4 VDD, 5 CLK, 6 VSS, 7 DAT0, 8 DAT1, SH shield | SD_DAT2, SD_DAT3, SD_CMD, SD_VDD, SD_CLK, GND, SD_DAT0, SD_DAT1, GND | Molex SD-47219-001 |
| J_CAM/J_DSI FH12-22S | 2/3 D0, 5/6 D1, 8/9 clock, 20 SCL, 21 SDA, 22 3V3 | CAM/DSI pairs, SCL0/SDA0, +3V3 | Raspberry Pi 22-pin FFC pinout |
| J_HDMI HDMI-A | 1/3 D2, 4/6 D1, 7/9 D0, 10/12 clock, 13 CEC, 15 SCL, 16 SDA, 18 +5V, 19 HPD | HDMI_D2/D1/D0/CK, HDMI_CEC, HDMI_SCL/SDA_CONN, HDMI_5V, HDMI_HPD | HDMI Type-A pinout; CM4 Table 6 |
