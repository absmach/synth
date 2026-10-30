# Netlist (net by net) — protected_power_entry rev A

Written by hand from `protected_power_entry.synth`; the tools did not emit a separate netlist file. The
KiCad schematic (`export/protected_power_entry.kicad_sch`) and PCB carry the same 34 named nets (+ no-net).
Net class in brackets. Pin names: J = connector pin, others as in the registry parts.

| Net [class] | Pins |
|---|---|
| GND [PGND] | J1.2 J2.2 J2.4 J2.8 J3.2 J4.2 J5.2 TP9 TP10 R1.2 C1.2 C2.2 C3.2 C4.2 C6.2 C7.2 D1.anode U1.4 **U1.9 (exposed pad)** R4.2 R6.2 C8.2 C9.2 C10.2 U3.2 C12.2 C13.2 C14.2 R7.2 D10.anode U2.1 C15.2 C16.2 C17.2 C18.2 R11.2 D11.anode C19.2 R14.2 D12.anode C20.2 R17.2 D13.anode C21.2 D2.cathode D3.cathode D4.cathode D5.cathode D6.cathode |
| VIN_12V_RAW [PWR12] | J1.1 F1.1 TP1 R19.1 R24.1 |
| VIN_12V_FUSED [PWR12] | F1.2 Q1.D(3) TP2 D7.cathode |
| PFET_GATE [SIG] | Q1.G(1) D9.anode R1.1 |
| +12V_PROT [PWR12] | Q1.S(2) D9.cathode D1.cathode C1.+ C2.1 C3.1 C4.1 U1.2 (IN) R5.1 TP3 R20.1 R10.1 |
| BUCK_SW [PWR5] | U1.3 (SW) L1.1 C5.2 TP7 |
| BUCK_BS [SIG] | U1.1 (BS) C5.1 |
| BUCK_FB [SIG] | U1.5 (FB) R3.2 R4.1 |
| BUCK_COMP [SIG] | U1.6 (COMP) R2.1 |
| BUCK_CRC [SIG] | R2.2 C7.1 |
| BUCK_SS [SIG] | U1.8 (SS) C6.1 |
| BUCK_EN [SIG] | U1.7 (EN) R5.2 R6.1 |
| +5V [PWR5] | L1.2 C8.1 C9.1 C10.+ R3.1 F2.1 U3.1 (VIN) C12.1 U2.3 (VIN) C15.1 TP4 R21.1 R13.1 |
| +5V_SW [PWR5] | U3.6 (VOUT) U3.5 (QOD) C14.1 F3.1 TP5 R22.1 |
| LS_CT [SIG] | U3.4 (CT) C13.1 |
| LOAD_EN [SIG] | U3.3 (ON) R7.1 R8.2 D10.cathode TP8 |
| EN_HDR [SIG] | R8.1 J2.3 |
| OUT_5V [PWR5] | F2.2 J3.1 |
| OUT_5V_SW [PWR5] | F3.2 J4.1 |
| +3V3 [PWR3] | U2.2 (VOUT, tab) C16.1 C17.+ C18.1 J5.1 TP6 R9.1 R23.1 R16.1 |
| 3V3_REF [SIG] | R9.2 J2.1 |
| S12_DIV [SIG] | R10.2 R11.1 R12.1 D11.cathode |
| SENSE_12V [SIG] | R12.2 C19.1 J2.5 |
| S5_DIV [SIG] | R13.2 R14.1 R15.1 D12.cathode |
| SENSE_5V [SIG] | R15.2 C20.1 J2.6 |
| S3_DIV [SIG] | R16.2 R17.1 R18.1 D13.cathode |
| SENSE_3V3 [SIG] | R18.2 C21.1 J2.7 |
| LED_IN_A [SIG] | R19.2 D8.anode |
| LED_IN_B [SIG] | D8.cathode D2.anode |
| LED12_A [SIG] | R20.2 D3.anode |
| LED5_A [SIG] | R21.2 D4.anode |
| LED5SW_A [SIG] | R22.2 D5.anode |
| LED3V3_A [SIG] | R23.2 D6.anode |
| FLT_A [SIG] | R24.2 D7.anode |

Hole-only parts H1-H4 have no electrical pins.

## Connectivity checks done on this netlist
- Compile/ERC: 0 errors (`synth_validate`, `synth_erc_report`); the KiCad export contains 34 nets and the router reports 88/88 pad pairs connected.
- Exposed pad U1.9 is on GND (authored part). U3 pin 3 (ON) has a pull-down R7 and is never floating.
- D1 (TVS) cathode is on +12V_PROT, i.e. behind Q1 and F1; it is not on VIN_12V_RAW or VIN_12V_FUSED.
- Q1: drain on VIN_12V_FUSED, source on +12V_PROT, gate on PFET_GATE with D9 gate-source and R1 gate-GND.
- D8 (Schottky) blocks the white input LED from a reversed supply.
- Not checked by a tool: pin-number correctness of the authored parts against datasheets (see `VERIFICATION_REPORT.md`).
- Names in the brief that do not exist as nets: none missing. Extra nets: OUT_5V, OUT_5V_SW (after the output fuses), 3V3_REF, S*_DIV, LED*_A, BUCK_BS, BUCK_CRC, BUCK_SS, BUCK_EN, LS_CT, EN_HDR, FLT_A.
