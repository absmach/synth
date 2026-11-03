# NETLIST — CM4 carrier (SYN-407)

The full netlist is the `net` blocks in `cm4_carrier.synth`. This table is the
same netlist from `synth dump-ir`, with the netclass and the declared rail
voltage. The CM4 connector pins are named `pN`; `PIN_MAPPING.md` maps them to
the datasheet signal names.

## Rails and netclasses

| Netclass | Trace width | Clearance | Used by |
|---|---|---|---|
| PWR | 0.5 mm | 0.2 mm | VIN, +5V, +5V_AUX, +3V3, +1V8, RTC_VDD, SD_VDD, USB_VBUS, HDMI_5V, GND |
| USB | 0.2 mm | 0.15 mm | USB upstream and four downstream pairs |
| ETH | 0.2 mm | 0.15 mm | four Ethernet pairs |
| HDMI | 0.2 mm | 0.15 mm | four HDMI TMDS pairs |
| SD | 0.15 mm | 0.15 mm | SD_CLK, SD_CMD, SD_DAT0–3 |
| CAM | 0.15 mm | 0.15 mm | CAM0 pairs |
| DSI | 0.15 mm | 0.15 mm | DSI0 pairs |
| I2C | 0.15 mm | 0.15 mm | SCL0, SDA0 |

## Nets

| Net | Class | Voltage | Members |
|---|---|---|---|
| `VIN_12V` | PWR | 12.00 V | J1.p1, F1.p1 |
| `VIN_FUSED` | PWR |  | F1.p2, Q1.s |
| `VIN_PROT` | PWR | 12.00 V | Q1.d, R_G1.p1, D1.cathode, C1.p1, C2.p1, C3.p1, U1.vin, R_UV_T.p1 |
| `VIN_GATE` |  |  | Q1.g, R_G1.p2 |
| `VCC_5V` |  |  | U1.vcc, C_VCC.p1 |
| `SW_5V` | PWR |  | U1.sw, L1.p1, C_BOOT.p2 |
| `BOOT_5V` |  |  | U1.boot, C_BOOT.p1 |
| `FB_5V` |  |  | U1.fb, R_FB_T.p2, R_FB_B.p1, C_FF.p1 |
| `PG_5V` |  |  | U1.pg, R_PG.p2, U2.en, TP_PG5V.p1 |
| `RT_3V3` |  |  | U2.rt_sync, R_RT2.p1 |
| `SW_3V3` | PWR |  | U2.sw, L2.p1, C_BOOT2.p2 |
| `BOOT_3V3` |  |  | U2.boot, C_BOOT2.p1 |
| `FB_3V3` |  |  | U2.fb, R_FB2_T.p2, R_FB2_B.p1, C_FF2.p1 |
| `PG_3V3` |  |  | U2.pg, R_PG2.p2, U3.en, TP_PG3V3.p1 |
| `RT_1V8` |  |  | U3.rt_sync, R_RT3.p1 |
| `SW_1V8` | PWR |  | U3.sw, L3.p1, C_BOOT3.p2 |
| `BOOT_1V8` |  |  | U3.boot, C_BOOT3.p1 |
| `FB_1V8` |  |  | U3.fb, R_FB3_T.p2, R_FB3_B.p1, C_FF3.p1 |
| `PG_1V8` |  |  | U3.pg, R_PG3.p2, TP_PG1V8.p1 |
| `+5V` | PWR | 5.00 V | L1.p2, C_OUT1.p1, C_OUT2.p1, C_OUT3.p1, R_FB_T.p1, R_PG.p1, R_PWR.p1, J_CM4A.p77, J_CM4A.p79, J_CM4A.p81, J_CM4A.p83, J_CM4A.p85, J_CM4A.p87, U5.vin, U6.vin, U7.vin, U8.vin, U10.vin, J_FAN.p1, D_FAN.cathode, J_GPIO2.p1, J_GPIO2.p2, L_FB1.p1, TP_5V.p1, U9.vccb, C_HDMI_VCCB.p1, D_HDMI5.vcc |
| `+5V_AUX` | PWR | 5.00 V | L_FB1.p2, C_AUX.p1, U2.vin, C_IN2_1.p1, C_IN2_2.p1, U3.vin, C_IN3_1.p1, C_IN3_2.p1 |
| `+3V3` | PWR | 3.30 V | L2.p2, C_OUT4.p1, C_OUT5.p1, C_OUT6.p1, R_FB2_T.p1, R_PG2.p1, R_3V3.p1, U4.vdd33, U4.vdd33_2, U4.vdda33, U4.vdda33_2, U4.vdda33_3, U4.vdda33_4, C_HUB1.p1, C_HUB2.p1, C_HUB3.p1, C_HUB4.p1, R_OCS1.p1, R_OCS2.p1, R_OCS3.p1, R_OCS4.p1, R_RESET.p1, R_VBUS.p1, J_VREF.p1, U10.on, U11.vin, C_SD_IN.p1, J_CAM.p22, C_CAM.p1, J_DSI.p22, C_DSI.p1, J_DSI.p17, U9.vcca, R_OE.p1, C_HDMI_VCCA.p1, D_USB_UP.vcc, D_ETH_ESD1.vcc, D_ETH_ESD2.vcc, D_ETH_ESD3.vcc, D_ETH_ESD4.vcc, D_HDMI1.vcc, D_HDMI2.vcc, D_HDMI3.vcc, D_HDMI4.vcc, D_RTC1.anode, R_ACT.p1, R_ETH.p1, R_ETH2.p1, R_LED_PWR.p1, J_GPIO1.p1, J_GPIO1.p9, TP_3V3.p1, R_GEN.p1, R_HDMI_SDA_MCU.p1, R_HDMI_SCL_MCU.p1, R_SD_PWR.p1, J_CAM.p18 |
| `+1V8` | PWR | 1.80 V | L3.p2, C_OUT7.p1, C_OUT8.p1, C_OUT9.p1, R_FB3_T.p1, R_PG3.p1, J_VREF.p3, TP_1V8.p1 |
| `CM4_3V3` |  | 3.30 V | J_CM4A.p84, J_CM4A.p86, TP_CM4_3V3.p1 |
| `CM4_1V8` |  | 1.80 V | J_CM4A.p88, J_CM4A.p90, TP_CM4_1V8.p1 |
| `GPIO_VREF` |  |  | J_CM4A.p78, J_VREF.p2 |
| `RUN_PG` |  |  | J_CM4A.p92, TP_RUN_PG.p1 |
| `NEXTRST` |  |  | J_CM4A.p100, TP_NEXTRST.p1 |
| `GLOBAL_EN` |  |  | J_CM4A.p99, SW1.p1, C_GEN.p1, TP_GLOBAL_EN.p1, R_GEN.p2 |
| `NRPIBOOT` |  |  | J_CM4A.p93, J_BOOT.p1 |
| `PI_LED_NPWR` |  |  | J_CM4A.p95, Q_LED.gate |
| `LED_PWR_CM4` |  |  | Q_LED.drain, D_PWR_CM4.cathode |
| `LED_PWR_CM4_A` |  |  | R_LED_PWR.p2, D_PWR_CM4.anode |
| `PI_LED_ACT` |  |  | J_CM4A.p21, D_ACT.cathode |
| `LED_ACT_A` |  |  | R_ACT.p2, D_ACT.anode |
| `ETH_LED_ACT` |  |  | J_CM4A.p15, D_ETH.cathode |
| `LED_ETH_A` |  |  | R_ETH.p2, D_ETH.anode |
| `ETH_LED_SPD` |  |  | J_CM4A.p17, D_ETH2.cathode |
| `LED_ETH2_A` |  |  | R_ETH2.p2, D_ETH2.anode |
| `ETH0_P` | ETH |  | J_CM4A.p12, J_ETH.td_p, D_ETH_ESD1.io1 |
| `ETH0_N` | ETH |  | J_CM4A.p10, J_ETH.td_n, D_ETH_ESD1.io2 |
| `ETH1_P` | ETH |  | J_CM4A.p4, J_ETH.rd_p, D_ETH_ESD2.io1 |
| `ETH1_N` | ETH |  | J_CM4A.p6, J_ETH.rd_n, D_ETH_ESD2.io2 |
| `ETH2_P` | ETH |  | J_CM4A.p11, J_ETH.pair2_p, D_ETH_ESD3.io1 |
| `ETH2_N` | ETH |  | J_CM4A.p9, J_ETH.pair2_n, D_ETH_ESD3.io2 |
| `ETH3_P` | ETH |  | J_CM4A.p3, J_ETH.pair3_p, D_ETH_ESD4.io1 |
| `ETH3_N` | ETH |  | J_CM4A.p5, J_ETH.pair3_n, D_ETH_ESD4.io2 |
| `ETH_TCT` |  |  | J_ETH.tct, C_TCT.p1 |
| `ETH_RCT` |  |  | J_ETH.rct, C_RCT.p1 |
| `USB_UP_P` | USB |  | J_CM4B.p5, U4.usbdp_up, D_USB_UP.io1 |
| `USB_UP_N` | USB |  | J_CM4B.p3, U4.usbdm_up, D_USB_UP.io2 |
| `USB1_P` | USB |  | U4.usbdp_dn1, J_USB1.d1_plus, D_USB1.io1 |
| `USB1_N` | USB |  | U4.usbdm_dn1, J_USB1.d1_minus, D_USB1.io2 |
| `USB2_P` | USB |  | U4.usbdp_dn2, J_USB1.d2_plus, D_USB2.io1 |
| `USB2_N` | USB |  | U4.usbdm_dn2, J_USB1.d2_minus, D_USB2.io2 |
| `USB3_P` | USB |  | U4.usbdp_dn3, J_USB2.d1_plus, D_USB3.io1 |
| `USB3_N` | USB |  | U4.usbdm_dn3, J_USB2.d1_minus, D_USB3.io2 |
| `USB4_P` | USB |  | U4.usbdp_dn4, J_USB2.d2_plus, D_USB4.io1 |
| `USB4_N` | USB |  | U4.usbdm_dn4, J_USB2.d2_minus, D_USB4.io2 |
| `USB_VBUS1` | PWR |  | U5.vout, U5.qod, J_USB1.vbus1, C_USB1.p1, C_USB1B.p1, D_USB1.vcc |
| `USB_VBUS2` | PWR |  | U6.vout, U6.qod, J_USB1.vbus2, C_USB2.p1, C_USB2B.p1, D_USB2.vcc |
| `USB_VBUS3` | PWR |  | U7.vout, U7.qod, J_USB2.vbus1, C_USB3.p1, C_USB3B.p1, D_USB3.vcc |
| `USB_VBUS4` | PWR |  | U8.vout, U8.qod, J_USB2.vbus2, C_USB4.p1, C_USB4B.p1, D_USB4.vcc |
| `USB_ON1` |  |  | U4.prtpwr1, U5.on |
| `USB_ON2` |  |  | U4.prtpwr2, U6.on |
| `USB_ON3` |  |  | U4.prtpwr3, U7.on |
| `USB_ON4` |  |  | U4.prtpwr4, U8.on |
| `USB_CT1` |  |  | U5.ct, C_CT1.p1 |
| `USB_CT2` |  |  | U6.ct, C_CT2.p1 |
| `USB_CT3` |  |  | U7.ct, C_CT3.p1 |
| `USB_CT4` |  |  | U8.ct, C_CT4.p1 |
| `HUB_XTAL1` |  |  | U4.xtalin_clkin, Y1.p1, C_X1.p1 |
| `HUB_XTAL2` |  |  | U4.xtalout, Y1.p2, C_X2.p1 |
| `HUB_RBIAS` |  |  | U4.rbias, R_RBIAS.p1 |
| `HUB_RESET` |  |  | U4.reset_n, R_RESET.p2, C_RESET.p1 |
| `HUB_CFG0` |  |  | U4.scl_smbclk_cfg_sel0, R_CFG0.p1 |
| `HUB_CFG1` |  |  | U4.hs_ind_cfg_sel1, R_CFG1.p1 |
| `HUB_LOCAL` |  |  | U4.susp_ind_local_pwr_non_rem0, R_LOCAL.p1 |
| `HUB_VBUS_DET` |  |  | U4.vbus_det, R_VBUS.p2 |
| `HUB_OCS1` |  |  | U4.ocs_n1, R_OCS1.p2 |
| `HUB_OCS2` |  |  | U4.ocs_n2, R_OCS2.p2 |
| `HUB_OCS3` |  |  | U4.ocs_n3, R_OCS3.p2 |
| `HUB_OCS4` |  |  | U4.ocs_n4, R_OCS4.p2 |
| `HUB_CRFILT` |  |  | U4.crfilt, C_CRFILT.p1 |
| `HUB_PLLFILT` |  |  | U4.pllfilt, C_PLLFILT.p1 |
| `HDMI_D2_P` | HDMI |  | J_CM4B.p70, J_HDMI.d2_p, D_HDMI1.io1 |
| `HDMI_D2_N` | HDMI |  | J_CM4B.p72, J_HDMI.d2_n, D_HDMI1.io2 |
| `HDMI_D1_P` | HDMI |  | J_CM4B.p76, J_HDMI.d1_p, D_HDMI2.io1 |
| `HDMI_D1_N` | HDMI |  | J_CM4B.p78, J_HDMI.d1_n, D_HDMI2.io2 |
| `HDMI_D0_P` | HDMI |  | J_CM4B.p82, J_HDMI.d0_p, D_HDMI3.io1 |
| `HDMI_D0_N` | HDMI |  | J_CM4B.p84, J_HDMI.d0_n, D_HDMI3.io2 |
| `HDMI_CK_P` | HDMI |  | J_CM4B.p88, J_HDMI.ck_p, D_HDMI4.io1 |
| `HDMI_CK_N` | HDMI |  | J_CM4B.p90, J_HDMI.ck_n, D_HDMI4.io2 |
| `HDMI_CEC` |  |  | J_CM4B.p51, J_HDMI.cec |
| `HDMI_HPD` |  |  | J_CM4B.p53, J_HDMI.hpd |
| `HDMI_SDA_MCU` |  |  | J_CM4B.p99, U9.a1, R_HDMI_SDA_MCU.p2 |
| `HDMI_SCL_MCU` |  |  | J_CM4B.p100, U9.a2, R_HDMI_SCL_MCU.p2 |
| `HDMI_SDA_CONN` |  |  | J_HDMI.sda, U9.b1, D_HDMI5.io1, R_HDMI_SDA.p1 |
| `HDMI_SCL_CONN` |  |  | J_HDMI.scl, U9.b2, D_HDMI5.io2, R_HDMI_SCL.p1 |
| `HDMI_5V` | PWR |  | U10.vout, U10.qod, J_HDMI.vcc_5v, C_HDMI1.p1, C_HDMI2.p1 |
| `HDMI_CT` |  |  | U10.ct, C_HDMI_CT.p1 |
| `HDMI_OE` |  |  | U9.oe, R_OE.p2 |
| `SD_CLK` | SD |  | J_CM4A.p57, J_SD.clk, D_SD1.io1 |
| `SD_CMD` | SD |  | J_CM4A.p62, J_SD.cmd, D_SD1.io2, R_SD_CMD.p2 |
| `SD_DAT0` | SD |  | J_CM4A.p63, J_SD.dat0, D_SD2.io1, R_SD_DAT0.p2 |
| `SD_DAT1` | SD |  | J_CM4A.p67, J_SD.dat1, D_SD2.io2, R_SD_DAT1.p2 |
| `SD_DAT2` | SD |  | J_CM4A.p69, J_SD.dat2, D_SD3.io1, R_SD_DAT2.p2 |
| `SD_DAT3` | SD |  | J_CM4A.p61, J_SD.dat3_cd, D_SD3.io2, R_SD_DAT3.p2 |
| `SD_VDD` | PWR |  | U11.vout, U11.qod, J_SD.vdd, C_SD1.p1, C_SD2.p1, R_SD_CMD.p1, R_SD_DAT0.p1, R_SD_DAT1.p1, R_SD_DAT2.p1, R_SD_DAT3.p1, D_SD1.vcc, D_SD2.vcc, D_SD3.vcc |
| `SD_PWR_ON` |  |  | J_CM4A.p75, U11.on, R_SD_PWR.p2 |
| `SD_CT` |  |  | U11.ct, C_SD_CT.p1 |
| `CAM_D0_P` | CAM |  | J_CM4B.p30, J_CAM.p3 |
| `CAM_D0_N` | CAM |  | J_CM4B.p28, J_CAM.p2 |
| `CAM_D1_P` | CAM |  | J_CM4B.p36, J_CAM.p6 |
| `CAM_D1_N` | CAM |  | J_CM4B.p34, J_CAM.p5 |
| `CAM_CK_P` | CAM |  | J_CM4B.p42, J_CAM.p9 |
| `CAM_CK_N` | CAM |  | J_CM4B.p40, J_CAM.p8 |
| `DSI_D0_P` | DSI |  | J_CM4B.p59, J_DSI.p3 |
| `DSI_D0_N` | DSI |  | J_CM4B.p57, J_DSI.p2 |
| `DSI_D1_P` | DSI |  | J_CM4B.p65, J_DSI.p6 |
| `DSI_D1_N` | DSI |  | J_CM4B.p63, J_DSI.p5 |
| `DSI_CK_P` | DSI |  | J_CM4B.p71, J_DSI.p9 |
| `DSI_CK_N` | DSI |  | J_CM4B.p69, J_DSI.p8 |
| `SCL0` | I2C |  | J_CM4A.p80, J_CAM.p20, J_DSI.p20, U12.scl, R_RTC_SCL.p2 |
| `SDA0` | I2C |  | J_CM4A.p82, J_CAM.p21, J_DSI.p21, U12.sda, R_RTC_SDA.p2 |
| `CAM_GPIO` |  |  | J_CM4A.p97, J_CAM.p17 |
| `RTC_VDD` | PWR | 3.00 V | U12.vdd, U12.clkoe, C_RTC.p1, D_RTC1.cathode, D_RTC2.cathode, R_RTC_SDA.p1, R_RTC_SCL.p1, R_RTC_INT.p1 |
| `RTC_BAT` |  |  | D_RTC2.anode, J_BT1.plus |
| `RTC_OSC1` |  |  | U12.osci, Y2.p1 |
| `RTC_OSC2` |  |  | U12.osco, Y2.p2 |
| `GPIO2` |  |  | J_CM4A.p58, J_GPIO1.p2 |
| `GPIO3` |  |  | J_CM4A.p56, J_GPIO1.p3 |
| `GPIO4` |  |  | J_CM4A.p54, J_GPIO1.p4 |
| `GPIO14` |  |  | J_CM4A.p55, J_GPIO2.p4 |
| `GPIO15` |  |  | J_CM4A.p51, J_GPIO2.p5 |
| `GPIO17` |  |  | J_CM4A.p50, J_GPIO1.p6 |
| `GPIO18` |  |  | J_CM4A.p49, J_GPIO2.p6 |
| `GPIO27` |  |  | J_CM4A.p48, J_GPIO1.p7 |
| `GPIO22` |  |  | J_CM4A.p46, J_GPIO1.p8 |
| `GPIO23` |  |  | J_CM4A.p47, J_GPIO2.p8 |
| `GPIO24` |  |  | J_CM4A.p45, J_GPIO2.p9 |
| `GPIO10` |  |  | J_CM4A.p44, J_GPIO1.p10 |
| `GPIO9` |  |  | J_CM4A.p40, J_GPIO1.p11 |
| `GPIO25` |  |  | J_CM4A.p41, J_GPIO2.p11 |
| `GPIO11` |  |  | J_CM4A.p38, J_GPIO1.p12 |
| `GPIO8` |  |  | J_CM4A.p39, J_GPIO2.p12 |
| `GPIO7` |  |  | J_CM4A.p37, J_GPIO2.p13 |
| `ID_SD` |  |  | J_CM4A.p36, J_GPIO1.p14 |
| `ID_SC` |  |  | J_CM4A.p35, J_GPIO2.p14 |
| `GPIO5` |  |  | J_CM4A.p34, J_GPIO1.p15 |
| `GPIO6` |  |  | J_CM4A.p30, J_GPIO1.p16 |
| `GPIO12` |  |  | J_CM4A.p31, J_GPIO2.p16 |
| `GPIO13` |  |  | J_CM4A.p28, J_GPIO1.p17 |
| `GPIO19` |  |  | J_CM4A.p26, J_GPIO1.p18 |
| `GPIO16` |  |  | J_CM4A.p29, J_GPIO2.p18 |
| `GPIO26` |  |  | J_CM4A.p24, J_GPIO1.p19, U12.int, R_RTC_INT.p2 |
| `GPIO20` |  |  | J_CM4A.p27, J_GPIO2.p19 |
| `GPIO21` |  |  | J_CM4A.p25, J_GPIO2.p20 |
| `GND` | PWR |  | J1.p2, D1.anode, C1.p2, C2.p2, C3.p2, U1.gnd, U1.pad, R_UV_B.p2, C_VCC.p2, C_OUT1.p2, C_OUT2.p2, C_OUT3.p2, R_FB_B.p2, D_PWR.cathode, R_PWR.p2, U2.gnd, U2.pad, R_RT2.p2, C_OUT4.p2, C_OUT5.p2, C_OUT6.p2, R_FB2_B.p2, D_3V3.cathode, R_3V3.p2, C_IN2_1.p2, C_IN2_2.p2, U3.gnd, U3.pad, R_RT3.p2, C_OUT7.p2, C_OUT8.p2, C_OUT9.p2, R_FB3_B.p2, C_IN3_1.p2, C_IN3_2.p2, C_AUX.p2, U4.vss, C_HUB1.p2, C_HUB2.p2, C_HUB3.p2, C_HUB4.p2, C_CRFILT.p2, C_PLLFILT.p2, R_RBIAS.p2, R_CFG0.p2, R_CFG1.p2, R_LOCAL.p2, C_RESET.p2, C_X1.p2, C_X2.p2, Y1.gnd1, Y1.gnd2, U5.gnd, U6.gnd, U7.gnd, U8.gnd, C_USB1.p2, C_USB2.p2, C_USB3.p2, C_USB4.p2, C_USB1B.p2, C_USB2B.p2, C_USB3B.p2, C_USB4B.p2, C_CT1.p2, C_CT2.p2, C_CT3.p2, C_CT4.p2, D_USB1.gnd, D_USB2.gnd, D_USB3.gnd, D_USB4.gnd, D_USB_UP.gnd, J_USB1.gnd1, J_USB1.gnd2, J_USB1.shield, J_USB2.gnd1, J_USB2.gnd2, J_USB2.shield, J_ETH.shield, C_TCT.p2, C_RCT.p2, D_ETH_ESD1.gnd, D_ETH_ESD2.gnd, D_ETH_ESD3.gnd, D_ETH_ESD4.gnd, J_HDMI.gnd, J_HDMI.shield, J_HDMI.d2_shield, J_HDMI.d1_shield, J_HDMI.d0_shield, J_HDMI.ck_shield, D_HDMI1.gnd, D_HDMI2.gnd, D_HDMI3.gnd, D_HDMI4.gnd, D_HDMI5.gnd, U9.gnd, C_HDMI_VCCA.p2, C_HDMI_VCCB.p2, U10.gnd, C_HDMI1.p2, C_HDMI2.p2, C_HDMI_CT.p2, J_SD.vss, J_SD.shield, U11.gnd, C_SD1.p2, C_SD2.p2, C_SD_IN.p2, C_SD_CT.p2, D_SD1.gnd, D_SD2.gnd, D_SD3.gnd, J_CAM.p1, J_CAM.p4, J_CAM.p7, J_CAM.p10, J_CAM.p13, J_CAM.p16, J_CAM.p19, C_CAM.p2, J_DSI.p1, J_DSI.p4, J_DSI.p7, J_DSI.p10, J_DSI.p13, J_DSI.p16, J_DSI.p19, C_DSI.p2, U12.vss, U12.ep, C_RTC.p2, J_BT1.minus, SW1.p2, C_GEN.p2, J_BOOT.p2, Q_LED.source, J_CM4B.p1, J_FAN.p2, D_FAN.anode, J_GPIO1.p5, J_GPIO1.p13, J_GPIO1.p20, J_GPIO2.p3, J_GPIO2.p7, J_GPIO2.p10, J_GPIO2.p15, J_GPIO2.p17, J_CM4A.p1, J_CM4A.p2, J_CM4A.p7, J_CM4A.p8, J_CM4A.p13, J_CM4A.p14, J_CM4A.p22, J_CM4A.p23, J_CM4A.p32, J_CM4A.p33, J_CM4A.p42, J_CM4A.p43, J_CM4A.p52, J_CM4A.p53, J_CM4A.p59, J_CM4A.p60, J_CM4A.p65, J_CM4A.p66, J_CM4A.p71, J_CM4A.p74, J_CM4A.p98, J_CM4B.p7, J_CM4B.p8, J_CM4B.p13, J_CM4B.p14, J_CM4B.p19, J_CM4B.p20, J_CM4B.p25, J_CM4B.p26, J_CM4B.p31, J_CM4B.p32, J_CM4B.p37, J_CM4B.p38, J_CM4B.p44, J_CM4B.p50, J_CM4B.p55, J_CM4B.p56, J_CM4B.p61, J_CM4B.p62, J_CM4B.p67, J_CM4B.p68, J_CM4B.p73, J_CM4B.p74, J_CM4B.p79, J_CM4B.p80, J_CM4B.p85, J_CM4B.p86, J_CM4B.p91, J_CM4B.p92, J_CM4B.p97, J_CM4B.p98 |

## Diff pairs

| Pair | Impedance | Members |
|---|---|---|
| ETH0_P / ETH0_N | 100 ohm | see nets |
| ETH1_P / ETH1_N | 100 ohm | see nets |
| ETH2_P / ETH2_N | 100 ohm | see nets |
| ETH3_P / ETH3_N | 100 ohm | see nets |
| USB_UP_P / USB_UP_N | 90 ohm | see nets |
| USB1_P / USB1_N | 90 ohm | see nets |
| USB2_P / USB2_N | 90 ohm | see nets |
| USB3_P / USB3_N | 90 ohm | see nets |
| USB4_P / USB4_N | 90 ohm | see nets |
| HDMI_D2_P / HDMI_D2_N | 100 ohm | see nets |
| HDMI_D1_P / HDMI_D1_N | 100 ohm | see nets |
| HDMI_D0_P / HDMI_D0_N | 100 ohm | see nets |
| HDMI_CK_P / HDMI_CK_N | 100 ohm | see nets |
| CAM_D0_P / CAM_D0_N | 100 ohm | see nets |
| CAM_D1_P / CAM_D1_N | 100 ohm | see nets |
| CAM_CK_P / CAM_CK_N | 100 ohm | see nets |
| DSI_D0_P / DSI_D0_N | 100 ohm | see nets |
| DSI_D1_P / DSI_D1_N | 100 ohm | see nets |
| DSI_CK_P / DSI_CK_N | 100 ohm | see nets |
