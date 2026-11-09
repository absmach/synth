# Pin mapping: BLDC stack (SYN-408)

Every used pin of every non-passive part, with the net it lands on. The table is
generated from the lowered IR, so it is the netlist's own view, not a hand
transcription. The "Datasheet" column is the URL recorded in the part's
`[provenance]` table; the pin numbers were checked against the pin table in
that document, but the page number was not recorded for every part.

Not included: capacitors, resistors, inductors and mounting holes (their two
terminals are interchangeable in this design).

## Reading the table

- `J_B2B` is the 2 x 10 stack connector; pins 1-20 are the physical pads.
- The STM32G030K8 pin names are the datasheet names; `PA14` is also BOOT0 on
  this package, which is why the boot button and SWCLK share it.
- The IR2101 `COM` pin is declared `ground_reference` in the registry, matching
  its role as the low-side return.
- The BSC030N08NS5 has three source pins in parallel; all three are listed.

| Board | Refdes | Part | Pin | Number | Net | Datasheet |
| --- | --- | --- | --- | --- | --- | --- |
| control | J_B2B | header_2x10 | p1 | 1 | PWM_UH | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p2 | 2 | PWM_UL | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p3 | 3 | PWM_VH | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p4 | 4 | PWM_VL | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p5 | 5 | PWM_WH | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p6 | 6 | PWM_WL | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p7 | 7 | ISENSE_U | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p8 | 8 | ISENSE_V | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p9 | 9 | ISENSE_W | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p10 | 10 | VBUS_SENSE | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p11 | 11 | GATE_EN | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p12 | 12 | FAULT_N | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p13 | 13 | +3V3_B2B | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p14 | 14 | +12V_LOGIC | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p15 | 15 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p16 | 16 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p17 | 17 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p18 | 18 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p19 | 19 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | J_B2B | header_2x10 | p20 | 20 | GND | https://www.lcsc.com/product-detail/C5116480.html |
| control | U_LDO | ams1117_3v3 | gnd | 1 | GND |  |
| control | U_LDO | ams1117_3v3 | vout | 2 | +3V3 |  |
| control | U_LDO | ams1117_3v3 | vin | 3 | +12V_LOGIC |  |
| control | U1 | stm32g030k8_lqfp32 | PB9 | 1 | GATE_EN | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PC14 | 2 | FAULT_N | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PC15 | 3 | — | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | VDD | 4 | +3V3 | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | VSS | 5 | GND | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | NRST | 6 | NRST | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA0 | 7 | ISENSE_U_ADC | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA1 | 8 | ISENSE_V_ADC | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA2 | 9 | ISENSE_W_ADC | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA3 | 10 | — | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA4 | 11 | HALL_U_MCU | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA5 | 12 | HALL_V_MCU | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA6 | 13 | HALL_W_MCU | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA7 | 14 | VBUS_SENSE_ADC | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB0 | 15 | PWM_WL | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB1 | 16 | SENSE_12V | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB2 | 17 | LED_STATUS | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA8 | 18 | PWM_UH | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA9 | 19 | PWM_UL | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PC6 | 20 | — | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA10 | 21 | PWM_VH | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA11 | 22 | ENC_A | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA12 | 23 | ENC_B | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA13 | 24 | SWDIO | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA14 | 25 | BOOT | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PA15 | 26 | — | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB3 | 27 | LED_FAULT | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB4 | 28 | PWM_VL | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB5 | 29 | PWM_WH | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB6 | 30 | RS485_TX | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB7 | 31 | RS485_RX_MCU | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | U1 | stm32g030k8_lqfp32 | PB8 | 32 | RS485_DE | https://www.st.com/resource/en/datasheet/stm32g030k8.pdf |
| control | SW_BOOT | spst_tactile | p1 | 1 | +3V3 |  |
| control | SW_BOOT | spst_tactile | p2 | 2 | BOOT |  |
| control | J_HALL_U | header_1x3 | p1 | 1 | +3V3 |  |
| control | J_HALL_U | header_1x3 | p2 | 2 | HALL_U |  |
| control | J_HALL_U | header_1x3 | p3 | 3 | GND |  |
| control | J_HALL_V | header_1x3 | p1 | 1 | +3V3 |  |
| control | J_HALL_V | header_1x3 | p2 | 2 | HALL_V |  |
| control | J_HALL_V | header_1x3 | p3 | 3 | GND |  |
| control | J_HALL_W | header_1x3 | p1 | 1 | +3V3 |  |
| control | J_HALL_W | header_1x3 | p2 | 2 | HALL_W |  |
| control | J_HALL_W | header_1x3 | p3 | 3 | GND |  |
| control | J_ENC | header_1x4 | p1 | 1 | +3V3 |  |
| control | J_ENC | header_1x4 | p2 | 2 | ENC_A |  |
| control | J_ENC | header_1x4 | p3 | 3 | ENC_B |  |
| control | J_ENC | header_1x4 | p4 | 4 | GND |  |
| control | U_RS485 | thvd1450d | ro | 1 | RS485_RX | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | re_n | 2 | RS485_DE | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | de | 3 | RS485_DE | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | di | 4 | RS485_TX | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | gnd | 5 | GND | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | a | 6 | RS485_A | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | b | 7 | RS485_B | https://www.ti.com/lit/gpn/THVD1450 |
| control | U_RS485 | thvd1450d | vcc | 8 | +3V3 | https://www.ti.com/lit/gpn/THVD1450 |
| control | D_RS485_A | ptvs6v0s1ur | cathode | 1 | RS485_A | https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf |
| control | D_RS485_A | ptvs6v0s1ur | anode | 2 | GND | https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf |
| control | D_RS485_B | ptvs6v0s1ur | cathode | 1 | RS485_B | https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf |
| control | D_RS485_B | ptvs6v0s1ur | anode | 2 | GND | https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf |
| control | J_RS485 | header_1x3 | p1 | 1 | RS485_A |  |
| control | J_RS485 | header_1x3 | p2 | 2 | RS485_B |  |
| control | J_RS485 | header_1x3 | p3 | 3 | GND |  |
| control | J_SWD | header_1x4 | p1 | 1 | +3V3 |  |
| control | J_SWD | header_1x4 | p2 | 2 | SWDIO |  |
| control | J_SWD | header_1x4 | p3 | 3 | BOOT |  |
| control | J_SWD | header_1x4 | p4 | 4 | GND |  |
| control | D_LED_STATUS | ltst_c190kgkt | cathode | 1 | GND | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF |
| control | D_LED_STATUS | ltst_c190kgkt | anode | 2 | LED_STATUS | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF |
| control | D_LED_FAULT | ltst_c190krkt | cathode | 1 | GND | https://optoelectronics.liteon.com/upload/download/DS-22-99-0151/LTST-C190KRKT.PDF |
| control | D_LED_FAULT | ltst_c190krkt | anode | 2 | LED_FAULT | https://optoelectronics.liteon.com/upload/download/DS-22-99-0151/LTST-C190KRKT.PDF |
| power | J_PWR | terminal_block_mkds_2pin | p1 | 1 | +24V_IN | https://www.phoenixcontact.com/us/products/1715721/pdf |
| power | J_PWR | terminal_block_mkds_2pin | p2 | 2 | GND | https://www.phoenixcontact.com/us/products/1715721/pdf |
| power | Q_REV | irf4905 | gate | 1 | GATE_REV | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | Q_REV | irf4905 | drain | 2 | +24V_IN | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | Q_REV | irf4905 | source | 3 | VBUS | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | D_REV_Z | bzt52c15 | cathode | 1 | VBUS | https://www.diodes.com/datasheet/download/BZT52Cxx.pdf |
| power | D_REV_Z | bzt52c15 | anode | 2 | GATE_REV | https://www.diodes.com/datasheet/download/BZT52Cxx.pdf |
| power | D_TVS | smbj30a | cathode | 1 | VBUS | https://www.vishay.com/docs/88392/smbj.pdf |
| power | D_TVS | smbj30a | anode | 2 | GND | https://www.vishay.com/docs/88392/smbj.pdf |
| power | Q_UH | bsc030n08ns5 | source | 1 | PHASE_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UH | bsc030n08ns5 | source_2 | 2 | PHASE_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UH | bsc030n08ns5 | source_3 | 3 | PHASE_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UH | bsc030n08ns5 | gate | 4 | GH_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UH | bsc030n08ns5 | drain | 5 | VBUS | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UL | bsc030n08ns5 | source | 1 | SHUNT_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UL | bsc030n08ns5 | source_2 | 2 | SHUNT_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UL | bsc030n08ns5 | source_3 | 3 | SHUNT_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UL | bsc030n08ns5 | gate | 4 | GL_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_UL | bsc030n08ns5 | drain | 5 | PHASE_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VH | bsc030n08ns5 | source | 1 | PHASE_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VH | bsc030n08ns5 | source_2 | 2 | PHASE_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VH | bsc030n08ns5 | source_3 | 3 | PHASE_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VH | bsc030n08ns5 | gate | 4 | GH_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VH | bsc030n08ns5 | drain | 5 | VBUS | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VL | bsc030n08ns5 | source | 1 | SHUNT_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VL | bsc030n08ns5 | source_2 | 2 | SHUNT_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VL | bsc030n08ns5 | source_3 | 3 | SHUNT_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VL | bsc030n08ns5 | gate | 4 | GL_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_VL | bsc030n08ns5 | drain | 5 | PHASE_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WH | bsc030n08ns5 | source | 1 | PHASE_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WH | bsc030n08ns5 | source_2 | 2 | PHASE_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WH | bsc030n08ns5 | source_3 | 3 | PHASE_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WH | bsc030n08ns5 | gate | 4 | GH_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WH | bsc030n08ns5 | drain | 5 | VBUS | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WL | bsc030n08ns5 | source | 1 | SHUNT_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WL | bsc030n08ns5 | source_2 | 2 | SHUNT_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WL | bsc030n08ns5 | source_3 | 3 | SHUNT_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WL | bsc030n08ns5 | gate | 4 | GL_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | Q_WL | bsc030n08ns5 | drain | 5 | PHASE_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-bsc030n08ns5-datasheet-en.pdf |
| power | U_GD_U | ir2101 | vcc | 1 | +12V_GD | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | hin | 2 | PWM_UH | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | lin | 3 | PWM_UL | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | com | 4 | GND | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | lo | 5 | GL_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | vs | 6 | PHASE_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | ho | 7 | GH_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_U | ir2101 | vb | 8 | BOOT_U | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | vcc | 1 | +12V_GD | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | hin | 2 | PWM_VH | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | lin | 3 | PWM_VL | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | com | 4 | GND | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | lo | 5 | GL_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | vs | 6 | PHASE_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | ho | 7 | GH_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_V | ir2101 | vb | 8 | BOOT_V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | vcc | 1 | +12V_GD | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | hin | 2 | PWM_WH | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | lin | 3 | PWM_WL | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | com | 4 | GND | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | lo | 5 | GL_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | vs | 6 | PHASE_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | ho | 7 | GH_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | U_GD_W | ir2101 | vb | 8 | BOOT_W | https://www.infineon.com/assets/row/public/documents/24/49/infineon-ir2101-ds-en.pdf |
| power | D_BOOT_U | d_schottky | anode | 2 | +12V_GD | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | D_BOOT_U | d_schottky | cathode | 1 | BOOT_U | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | D_BOOT_V | d_schottky | anode | 2 | +12V_GD | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | D_BOOT_V | d_schottky | cathode | 1 | BOOT_V | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | D_BOOT_W | d_schottky | anode | 2 | +12V_GD | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | D_BOOT_W | d_schottky | cathode | 1 | BOOT_W | https://www.diodes.com/datasheet/download/1N5819HW.pdf |
| power | Q_GE | irf4905 | gate | 1 | GE_GATE | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | Q_GE | irf4905 | drain | 2 | +12V_GD | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | Q_GE | irf4905 | source | 3 | +12V | https://www.infineon.com/assets/row/public/documents/24/49/infineon-irf4905-datasheet-en.pdf |
| power | Q_GE_CTRL | npn_bjt_2n3904 | emitter | 1 | GND |  |
| power | Q_GE_CTRL | npn_bjt_2n3904 | base | 2 | GE_DRIVE |  |
| power | Q_GE_CTRL | npn_bjt_2n3904 | collector | 3 | GE_GATE |  |
| power | U_CS1 | mcp6022_soic | out_a | 1 | CS_U_OUT | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | in_a_neg | 2 | CS_U_FB | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | in_a_pos | 3 | CS_U_IN | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | vss | 4 | GND | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | in_b_pos | 5 | CS_V_IN | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | in_b_neg | 6 | CS_V_FB | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | out_b | 7 | CS_V_OUT | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS1 | mcp6022_soic | vdd | 8 | +3V3 | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | out_a | 1 | CS_W_OUT | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | in_a_neg | 2 | CS_W_FB | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | in_a_pos | 3 | CS_W_IN | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | vss | 4 | GND | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | in_b_pos | 5 | FAULT_SENSE | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | in_b_neg | 6 | VREF | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | out_b | 7 | FAULT_DRIVE | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_CS2 | mcp6022_soic | vdd | 8 | +3V3 | https://ww1.microchip.com/downloads/en/DeviceDoc/20001685E.pdf |
| power | U_BUCK | lmr38010 | gnd | 1 | GND | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | en | 2 | VBUS | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | vin | 3 | VBUS | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | rt_sync | 4 | RT_BUCK | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | fb | 5 | FB_BUCK | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | pg | 6 | — | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | boot | 7 | BOOT_BUCK | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | sw | 8 | SW_BUCK | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | U_BUCK | lmr38010 | pad | 9 | GND | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| power | Q_FAULT | npn_bjt_2n3904 | emitter | 1 | GND |  |
| power | Q_FAULT | npn_bjt_2n3904 | base | 2 | FAULT_DRIVE |  |
| power | Q_FAULT | npn_bjt_2n3904 | collector | 3 | FAULT_N |  |
| power | J_B2B | header_2x10_socket | p1 | 1 | PWM_UH | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p2 | 2 | PWM_UL | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p3 | 3 | PWM_VH | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p4 | 4 | PWM_VL | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p5 | 5 | PWM_WH | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p6 | 6 | PWM_WL | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p7 | 7 | ISENSE_U | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p8 | 8 | ISENSE_V | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p9 | 9 | ISENSE_W | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p10 | 10 | VBUS_SENSE | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p11 | 11 | GATE_EN | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p12 | 12 | FAULT_N | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p13 | 13 | +3V3 | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p14 | 14 | +12V_B2B | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p15 | 15 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p16 | 16 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p17 | 17 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p18 | 18 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p19 | 19 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_B2B | header_2x10_socket | p20 | 20 | GND | https://www.lcsc.com/product-detail/C30867.html |
| power | J_MOTOR | terminal_block_mkds_3pin | p1 | 1 | PHASE_U | https://www.phoenixcontact.com/us/products/1715734/pdf |
| power | J_MOTOR | terminal_block_mkds_3pin | p2 | 2 | PHASE_V | https://www.phoenixcontact.com/us/products/1715734/pdf |
| power | J_MOTOR | terminal_block_mkds_3pin | p3 | 3 | PHASE_W | https://www.phoenixcontact.com/us/products/1715734/pdf |
