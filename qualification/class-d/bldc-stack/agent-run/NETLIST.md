# Netlist: BLDC stack (SYN-408)

Generated from `synth dump-ir` on the two committed sources on 2026-11-03.
Every net below is a real lowered net; the members are `refdes.pin(number)`.

## Rails and netclasses

| Board | Rail / class | Declaration | Use |
| --- | --- | --- | --- |
| control | `+12V_LOGIC` 12 V | `power "+12V_LOGIC" 12v` | from J_B2B pin 14 into the AMS1117 |
| control | `+3V3` 3.3 V | `power "+3V3" 3.3v` | AMS1117 output, MCU and logic |
| control | `+3V3_B2B` 3.3 V | `power "+3V3_B2B" 3.3v` | behind L_B3V3 ferrite, J_B2B pin 13 |
| control | `PWR` class | 0.5 mm / 0.2 mm | 12 V, 3.3 V and ground |
| control | `SENSE` class | 0.2 mm / 0.2 mm | current and voltage sense |
| power | `VBUS` 24 V | `power "VBUS" 24v` | protected 24 V bus |
| power | `+12V` 12 V | `power "+12V" 12v` | LMR38010 output |
| power | `+12V_GD` 12 V | `power "+12V_GD" 12v` | switched gate-driver supply |
| power | `+12V_B2B` 12 V | `power "+12V_B2B" 12v` | J_B2B pin 14 through L_12B2B |
| power | `+3V3` 3.3 V | `power "+3V3" 3.3v` | from J_B2B pin 13 |
| power | `PWR` class | 2 mm / 0.3 mm | 24 V, 12 V and shunt copper |
| power | `PHASE` class | 2 mm / 0.3 mm | U/V/W phase nodes |
| power | `GATE` class | 0.3 mm / 0.2 mm | gate drive |
| power | `SENSE` class | 0.25 mm / 0.2 mm | shunt and sense |

`GND` is a net, not a `power` rail, on both boards. Its name makes it the
ground domain for the ERC and power-domain inference.

## Control board

### bldc_control

| Net | Class | Members |
| --- | --- | --- |
| `ISENSE_U_ADC` |  | R_ISENSE_U.p2(2), U1.PA0(7), C_ISENSE_U.p1(1) |
| `ISENSE_V_ADC` |  | R_ISENSE_V.p2(2), U1.PA1(8), C_ISENSE_V.p1(1) |
| `ISENSE_W_ADC` |  | R_ISENSE_W.p2(2), U1.PA2(9), C_ISENSE_W.p1(1) |
| `VBUS_SENSE_ADC` |  | R_VBUS_S.p2(2), U1.PA7(14), C_VBUS_S.p1(1) |
| `SENSE_12V` |  | R_12V_T.p2(2), R_12V_B.p1(1), C_12V_S.p1(1), U1.PB1(16) |
| `HALL_U_MCU` |  | R_HALL_SU.p2(2), U1.PA4(11), C_HALL_U.p1(1) |
| `HALL_V_MCU` |  | R_HALL_SV.p2(2), U1.PA5(12), C_HALL_V.p1(1) |
| `HALL_W_MCU` |  | R_HALL_SW.p2(2), U1.PA6(13), C_HALL_W.p1(1) |
| `RS485_RX_MCU` |  | R_RS485_RX.p2(2), U1.PB7(31) |
| `NRST` |  | U1.NRST(6), R_NRST.p2(2), C_NRST.p1(1) |
| `BOOT` |  | SW_BOOT.p2(2), R_BOOT.p2(2), U1.PA14(25), J_SWD.p3(3) |
| `SWDIO` |  | U1.PA13(24), J_SWD.p2(2) |
| `LED_STATUS` |  | U1.PB2(17), R_LED_STATUS.p2(2), D_LED_STATUS.anode(2) |
| `LED_FAULT` |  | U1.PB3(27), R_LED_FAULT.p2(2), D_LED_FAULT.anode(2) |
| `+12V_LOGIC` | PWR | J_B2B.p14(14), U_LDO.vin(3), C_LDO_IN.p1(1), C_LDO_IN2.p1(1), R_12V_T.p1(1) |
| `+3V3` | PWR | U_LDO.vout(2), C_LDO_OUT.p1(1), L_B3V3.p1(1), U1.VDD(4), C_VDD1.p1(1), C_VDD2.p1(1), R_NRST.p1(1), R_HALL_U.p2(2), R_HALL_V.p2(2), R_HALL_W.p2(2), J_HALL_U.p1(1), J_HALL_V.p1(1), J_HALL_W.p1(1), J_ENC.p1(1), R_ENC_A.p2(2), R_ENC_B.p2(2), J_SWD.p1(1), SW_BOOT.p1(1), U_RS485.vcc(8), C_RS485.p1(1), R_LED_STATUS.p1(1), R_LED_FAULT.p1(1), R_BOOT.p1(1), R_FAULT.p1(1) |
| `+3V3_B2B` | PWR | L_B3V3.p2(2), C_B2B.p1(1), J_B2B.p13(13) |
| `GND` | PWR | J_B2B.p15(15), J_B2B.p16(16), J_B2B.p17(17), J_B2B.p18(18), J_B2B.p19(19), J_B2B.p20(20), U_LDO.gnd(1), C_LDO_IN.p2(2), C_LDO_IN2.p2(2), C_LDO_OUT.p2(2), C_B2B.p2(2), U1.VSS(5), C_VDD1.p2(2), C_VDD2.p2(2), C_NRST.p2(2), J_HALL_U.p3(3), J_HALL_V.p3(3), J_HALL_W.p3(3), C_HALL_U.p2(2), C_HALL_V.p2(2), C_HALL_W.p2(2), J_ENC.p4(4), C_ENC_A.p2(2), C_ENC_B.p2(2), U_RS485.gnd(5), C_RS485.p2(2), D_RS485_A.anode(2), D_RS485_B.anode(2), J_RS485.p3(3), J_SWD.p4(4), D_LED_STATUS.cathode(1), D_LED_FAULT.cathode(1), C_ISENSE_U.p2(2), C_ISENSE_V.p2(2), C_ISENSE_W.p2(2), C_VBUS_S.p2(2), R_12V_B.p2(2), C_12V_S.p2(2), R_GATE_EN.p2(2), C_FAULT.p2(2) |
| `PWM_UH` |  | U1.PA8(18), J_B2B.p1(1) |
| `PWM_UL` |  | U1.PA9(19), J_B2B.p2(2) |
| `PWM_VH` |  | U1.PA10(21), J_B2B.p3(3) |
| `PWM_VL` |  | U1.PB4(28), J_B2B.p4(4) |
| `PWM_WH` |  | U1.PB5(29), J_B2B.p5(5) |
| `PWM_WL` |  | U1.PB0(15), J_B2B.p6(6) |
| `ISENSE_U` | SENSE | R_ISENSE_U.p1(1), J_B2B.p7(7) |
| `ISENSE_V` | SENSE | R_ISENSE_V.p1(1), J_B2B.p8(8) |
| `ISENSE_W` | SENSE | R_ISENSE_W.p1(1), J_B2B.p9(9) |
| `VBUS_SENSE` | SENSE | R_VBUS_S.p1(1), J_B2B.p10(10) |
| `GATE_EN` |  | U1.PB9(1), J_B2B.p11(11), R_GATE_EN.p1(1) |
| `FAULT_N` |  | U1.PC14(2), J_B2B.p12(12), R_FAULT.p2(2), C_FAULT.p1(1) |
| `HALL_U` |  | J_HALL_U.p2(2), R_HALL_U.p1(1), R_HALL_SU.p1(1) |
| `HALL_V` |  | J_HALL_V.p2(2), R_HALL_V.p1(1), R_HALL_SV.p1(1) |
| `HALL_W` |  | J_HALL_W.p2(2), R_HALL_W.p1(1), R_HALL_SW.p1(1) |
| `ENC_A` |  | J_ENC.p2(2), R_ENC_A.p1(1), C_ENC_A.p1(1), U1.PA11(22) |
| `ENC_B` |  | J_ENC.p3(3), R_ENC_B.p1(1), C_ENC_B.p1(1), U1.PA12(23) |
| `RS485_TX` |  | U1.PB6(30), U_RS485.di(4) |
| `RS485_RX` |  | U_RS485.ro(1), R_RS485_RX.p1(1) |
| `RS485_DE` |  | U1.PB8(32), U_RS485.de(3), U_RS485.re_n(2) |
| `RS485_A` |  | U_RS485.a(6), R_TERM.p1(1), D_RS485_A.cathode(1), J_RS485.p1(1) |
| `RS485_B` |  | U_RS485.b(7), R_TERM.p2(2), D_RS485_B.cathode(1), J_RS485.p2(2) |

## Power board

### bldc_power

| Net | Class | Members |
| --- | --- | --- |
| `GH_U` |  | U_GD_U.ho(7), R_GH_U.p1(1), R_GH_U.p2(2), Q_UH.gate(4), R_GS_UH.p1(1) |
| `GL_U` |  | U_GD_U.lo(5), R_GL_U.p1(1), R_GL_U.p2(2), Q_UL.gate(4), R_GS_UL.p1(1) |
| `GH_V` |  | U_GD_V.ho(7), R_GH_V.p1(1), R_GH_V.p2(2), Q_VH.gate(4), R_GS_VH.p1(1) |
| `GL_V` |  | U_GD_V.lo(5), R_GL_V.p1(1), R_GL_V.p2(2), Q_VL.gate(4), R_GS_VL.p1(1) |
| `GH_W` |  | U_GD_W.ho(7), R_GH_W.p1(1), R_GH_W.p2(2), Q_WH.gate(4), R_GS_WH.p1(1) |
| `GL_W` |  | U_GD_W.lo(5), R_GL_W.p1(1), R_GL_W.p2(2), Q_WL.gate(4), R_GS_WL.p1(1) |
| `BOOT_U` |  | D_BOOT_U.cathode(1), U_GD_U.vb(8), C_BOOT_U.p1(1) |
| `BOOT_V` |  | D_BOOT_V.cathode(1), U_GD_V.vb(8), C_BOOT_V.p1(1) |
| `BOOT_W` |  | D_BOOT_W.cathode(1), U_GD_W.vb(8), C_BOOT_W.p1(1) |
| `CS_U_OUT` |  | U_CS1.out_a(1), R_CS_U_F.p1(1), C_CS_U_F.p1(1), R_IS_OUT_U.p1(1) |
| `CS_V_OUT` |  | U_CS1.out_b(7), R_CS_V_F.p1(1), C_CS_V_F.p1(1), R_IS_OUT_V.p1(1) |
| `CS_W_OUT` |  | U_CS2.out_a(1), R_CS_W_F.p1(1), C_CS_W_F.p1(1), R_IS_OUT_W.p1(1), R_FAULT_SENSE.p1(1) |
| `ISENSE_U` |  | R_IS_OUT_U.p2(2), J_B2B.p7(7), C_IS_OUT_U.p1(1) |
| `ISENSE_V` |  | R_IS_OUT_V.p2(2), J_B2B.p8(8), C_IS_OUT_V.p1(1) |
| `ISENSE_W` |  | R_IS_OUT_W.p2(2), J_B2B.p9(9), C_IS_OUT_W.p1(1) |
| `FAULT_SENSE` |  | U_CS2.in_b_pos(5), R_FAULT_SENSE.p2(2) |
| `FAULT_DRIVE` |  | U_CS2.out_b(7), R_FAULT_B.p1(1), R_FAULT_B.p2(2), Q_FAULT.base(2) |
| `FAULT_N` |  | Q_FAULT.collector(3), R_FAULT_PU.p2(2), J_B2B.p12(12) |
| `GE_DRIVE` |  | R_GE_B.p2(2), Q_GE_CTRL.base(2), R_GE_BD.p1(1) |
| `GE_GATE` |  | Q_GE_CTRL.collector(3), Q_GE.gate(1), R_GE_PU.p2(2) |
| `SW_BUCK` |  | U_BUCK.sw(8), L_BUCK.p1(1), C_BUCK_BOOT.p2(2) |
| `BOOT_BUCK` |  | U_BUCK.boot(7), C_BUCK_BOOT.p1(1) |
| `RT_BUCK` |  | U_BUCK.rt_sync(4), R_BUCK_RT.p1(1) |
| `FB_BUCK` |  | U_BUCK.fb(5), R_BUCK_FB_T.p2(2), R_BUCK_FB_B.p1(1), C_BUCK_FF.p2(2) |
| `+24V_IN` | PWR | J_PWR.p1(1), Q_REV.drain(2) |
| `GATE_REV` |  | Q_REV.gate(1), R_REV_G1.p2(2), R_REV_G2.p1(1), D_REV_Z.anode(2) |
| `VBUS` | PWR | Q_REV.source(3), R_REV_G1.p1(1), D_REV_Z.cathode(1), D_TVS.cathode(1), C_BULK1.p1(1), C_BULK2.p1(1), C_IN1.p1(1), C_IN2.p1(1), C_IN3.p1(1), Q_UH.drain(5), Q_VH.drain(5), Q_WH.drain(5), U_BUCK.vin(3), U_BUCK.en(2), C_BUCK_IN.p1(1), C_BUCK_IN2.p1(1), R_VBUS_S1.p1(1) |
| `GND` | PWR | J_PWR.p2(2), R_REV_G2.p2(2), D_TVS.anode(2), C_BULK1.p2(2), C_BULK2.p2(2), C_IN1.p2(2), C_IN2.p2(2), C_IN3.p2(2), R_SH_U.p2(2), R_SH_V.p2(2), R_SH_W.p2(2), U_GD_U.com(4), U_GD_V.com(4), U_GD_W.com(4), C_GD_U.p2(2), C_GD_V.p2(2), C_GD_W.p2(2), C_GD_BULK.p2(2), R_HIN_U.p2(2), R_HIN_V.p2(2), R_HIN_W.p2(2), R_LIN_U.p2(2), R_LIN_V.p2(2), R_LIN_W.p2(2), Q_GE_CTRL.emitter(1), R_GE_BD.p2(2), U_CS1.vss(4), U_CS2.vss(4), R_CS_U_G.p2(2), R_CS_V_G.p2(2), R_CS_W_G.p2(2), C_CS_U_IN.p2(2), C_CS_V_IN.p2(2), C_CS_W_IN.p2(2), C_IS_OUT_U.p2(2), C_IS_OUT_V.p2(2), C_IS_OUT_W.p2(2), R_REF_B.p2(2), C_REF.p2(2), Q_FAULT.emitter(1), U_BUCK.gnd(1), U_BUCK.pad(9), C_BUCK_IN.p2(2), C_BUCK_IN2.p2(2), C_BUCK_OUT1.p2(2), C_BUCK_OUT2.p2(2), C_BUCK_OUT3.p2(2), R_BUCK_RT.p2(2), R_BUCK_FB_B.p2(2), C_12B2B.p2(2), C_3V3.p2(2), C_3V3_2.p2(2), J_B2B.p15(15), J_B2B.p16(16), J_B2B.p17(17), J_B2B.p18(18), J_B2B.p19(19), J_B2B.p20(20) |
| `PHASE_U` | PHASE | Q_UH.source(1), Q_UH.source_2(2), Q_UH.source_3(3), Q_UL.drain(5), U_GD_U.vs(6), C_BOOT_U.p2(2), R_GS_UH.p2(2), J_MOTOR.p1(1) |
| `PHASE_V` | PHASE | Q_VH.source(1), Q_VH.source_2(2), Q_VH.source_3(3), Q_VL.drain(5), U_GD_V.vs(6), C_BOOT_V.p2(2), R_GS_VH.p2(2), J_MOTOR.p2(2) |
| `PHASE_W` | PHASE | Q_WH.source(1), Q_WH.source_2(2), Q_WH.source_3(3), Q_WL.drain(5), U_GD_W.vs(6), C_BOOT_W.p2(2), R_GS_WH.p2(2), J_MOTOR.p3(3) |
| `SHUNT_U` | PWR | Q_UL.source(1), Q_UL.source_2(2), Q_UL.source_3(3), R_GS_UL.p2(2), R_SH_U.p1(1), R_CS_U_IN.p1(1) |
| `SHUNT_V` | PWR | Q_VL.source(1), Q_VL.source_2(2), Q_VL.source_3(3), R_GS_VL.p2(2), R_SH_V.p1(1), R_CS_V_IN.p1(1) |
| `SHUNT_W` | PWR | Q_WL.source(1), Q_WL.source_2(2), Q_WL.source_3(3), R_GS_WL.p2(2), R_SH_W.p1(1), R_CS_W_IN.p1(1) |
| `PWM_UH` |  | J_B2B.p1(1), U_GD_U.hin(2), R_HIN_U.p1(1) |
| `PWM_UL` |  | J_B2B.p2(2), U_GD_U.lin(3), R_LIN_U.p1(1) |
| `PWM_VH` |  | J_B2B.p3(3), U_GD_V.hin(2), R_HIN_V.p1(1) |
| `PWM_VL` |  | J_B2B.p4(4), U_GD_V.lin(3), R_LIN_V.p1(1) |
| `PWM_WH` |  | J_B2B.p5(5), U_GD_W.hin(2), R_HIN_W.p1(1) |
| `PWM_WL` |  | J_B2B.p6(6), U_GD_W.lin(3), R_LIN_W.p1(1) |
| `CS_U_IN` |  | R_CS_U_IN.p2(2), C_CS_U_IN.p1(1), U_CS1.in_a_pos(3) |
| `CS_V_IN` |  | R_CS_V_IN.p2(2), C_CS_V_IN.p1(1), U_CS1.in_b_pos(5) |
| `CS_W_IN` |  | R_CS_W_IN.p2(2), C_CS_W_IN.p1(1), U_CS2.in_a_pos(3) |
| `CS_U_FB` |  | R_CS_U_G.p1(1), R_CS_U_F.p2(2), C_CS_U_F.p2(2), U_CS1.in_a_neg(2) |
| `CS_V_FB` |  | R_CS_V_G.p1(1), R_CS_V_F.p2(2), C_CS_V_F.p2(2), U_CS1.in_b_neg(6) |
| `CS_W_FB` |  | R_CS_W_G.p1(1), R_CS_W_F.p2(2), C_CS_W_F.p2(2), U_CS2.in_a_neg(2) |
| `VREF` |  | R_REF_T.p2(2), R_REF_B.p1(1), C_REF.p1(1), U_CS2.in_b_neg(6) |
| `GATE_EN` |  | J_B2B.p11(11), R_GE_B.p1(1) |
| `+12V` | PWR | L_BUCK.p2(2), C_BUCK_OUT1.p1(1), C_BUCK_OUT2.p1(1), C_BUCK_OUT3.p1(1), R_BUCK_FB_T.p1(1), C_BUCK_FF.p1(1), Q_GE.source(3), R_GE_PU.p1(1), L_12B2B.p1(1) |
| `+12V_GD` | PWR | Q_GE.drain(2), C_GD_BULK.p1(1), C_GD_U.p1(1), C_GD_V.p1(1), C_GD_W.p1(1), U_GD_U.vcc(1), U_GD_V.vcc(1), U_GD_W.vcc(1), D_BOOT_U.anode(2), D_BOOT_V.anode(2), D_BOOT_W.anode(2) |
| `+12V_B2B` | PWR | L_12B2B.p2(2), C_12B2B.p1(1), J_B2B.p14(14) |
| `VBUS_SENSE` |  | R_VBUS_S1.p2(2), R_VBUS_S2.p1(1), C_VBUS_S.p1(1), J_B2B.p10(10) |
| `+3V3` | PWR | J_B2B.p13(13), C_3V3.p1(1), C_3V3_2.p1(1), U_CS1.vdd(8), U_CS2.vdd(8), R_REF_T.p1(1), R_FAULT_PU.p1(1) |
