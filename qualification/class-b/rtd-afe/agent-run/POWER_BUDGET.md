# POWER_BUDGET: rtd-afe

Only values recorded in `rtd_afe.synth`, `registry/parts/**`, README.md and QUALIFICATION.md are used. No current was measured or datasheet-derived by this reconstruction. README: "No VBUS current budget was measured." Anything not recorded is "not verified".

## Rail table
| Rail | Source | Loads | Regulator limit (registry) | Margin |
| --- | --- | --- | --- | --- |
| VBUS 5 V (`power "VBUS" 5v`) | J1 (USB host) | U5 LP5907 VIN and EN, C_L1 | host current limit not recorded | not verified |
| +3V3 3.3 V | U5 LP5907MFX-3.3, 250 mA max, 200 mW max power, 250 C/W | U1 DVDD/IOVDD, U2 IOVDD x6 / QSPI_IOVDD / USB_OTP_VDD / VREG_VIN, U4 VCC, pull-ups R_RST, R_RUN, L2 (feeds +3V3A), R_AV (feeds +3V3_RP_A) | 250 mA | load sum not verified |
| +3V3A | +3V3 through L2 600R@100MHz ferrite | U1 AVDD (330 nF + 10 uF), U3 VIN | L2 current rating and DC resistance not recorded ("not computed here" in `.synth`) | not verified |
| +3V3_RP_A | +3V3 through R_AV 33R + C_AVF 4.7 uF | U2 VREG_AVDD, ADC_AVDD | R_AV power rating not recorded | not verified |
| DVDD_1V1 | U2 on-chip regulator, VREG_LX through L1 3.3 uH | U2 DVDD x3, C_DV1, C_DV2, C_DVB | regulator output current limit and L1 saturation current not recorded | not verified |
| VREF 2.5 V | U3 REF5025 (registry description: 10 mA) | U1 REFP0, C_RV | 10 mA (from the registry description, not re-read) | U1 REFP0 load not recorded: not verified |

## Operating limits recorded in the registry
| Part | Supply range | Other |
| --- | --- | --- |
| U1 ADS124S08 | 2.7 to 5.25 V | rails used: AVDD, DVDD, IOVDD all 3.3 V (inside range) |
| U3 REF5025 | 2.7 to 18 V | VIN 3.3 V from +3V3A; dropout against the 2.5 V output not recorded: not verified |
| U2 RP2350 | 1.8 to 3.6 V, max_current 150 mA, max_power 300 mW | registry values; origin not verified |
| U4 W25Q128 | 1.8 to 3.6 V, max_current 50 mA | registry value |
| U5 LP5907 | 2.2 to 5.5 V, 250 mA, 200 mW, 250 C/W | VBUS 5 V in |

## Derived arithmetic (this document, not a source value)
- Registry maxima for U2 and U4 sum to 200 mA, leaving 50 mA of the 250 mA LDO limit before U1, U3 and the 1 mA IDAC are counted. Those loads are not recorded: not verified.
- LDO dissipation at 5.0 V in and 3.3 V out is (5.0 - 3.3) V x I = 1.7 V x I, ignoring ground current. The registry `max_power_mw` of 200 is reached at about 118 mA (200 mW / 1.7 V). At the 250 mA limit the figure is 425 mW, above the 200 mW registry value. Whether the registry 200 mW is the datasheet value, and the actual thermal behaviour on this board, are not verified. The note in the `.synth` says the board draws "a few tens of mA" (stated, not measured).
- IDAC path: 1 mA through R_BIAS 1k gives 1 V across R_BIAS (README: common mode about 1.1 V at 100 ohm RTD). Firmware must set the IDAC; not verified.

## Decoupling fitted vs registry requirement
Per QUALIFICATION.md: ADS124S08 AVDD 330 nF (C_AV1) and DVDD 100 nF (C_DV); REF5025 VIN/VOUT 1 uF (C_RI, C_RV); LP5907 VIN/VOUT 2.2 uF fitted vs 1 uF required (C_L1, C_L2); RP2350 IOVDD 4 x 100 nF (C_IO1..4) and DVDD 2 x 100 nF + 4.7 uF (C_DV1, C_DV2, C_DVB); W25Q128 100 nF (C_FL). Capacitor DC-bias derating at 10 V ratings on 3.3 V rails: not verified.

## Gaps
No VBUS fuse, TVS or reverse-polarity protection. No in-rush or USB-compliance check. No measured current.
