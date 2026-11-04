# Pin mapping: buck rev A

Net names are the names in `../buck.synth` (the same names are in `../kicad/buck.kicad_pcb`). Pin numbers are registry pad numbers. Datasheet reference is the `datasheet_url` recorded in the part file; "reviewed" is the `reviewed_by`/`reviewed_at` in that file. Nothing here re-reads a datasheet.

## U1: TI LMR38010SDDAR, HSOP-8-1EP (`registry/parts/regulators/lmr38010.synth.toml`)
Datasheet: https://www.ti.com/lit/ds/symlink/lmr38010.pdf (SNVSBM3B). Reviewed Steve Munene 2026-10-29. `QUALIFICATION.md` records that the datasheet pin table agrees with these pins.

| Pad | Pin | Net | Connected to | Note |
|---|---|---|---|---|
| 1 | gnd | GND | | |
| 2 | en | EN | R_UV_T.p2, R_UV_B.p1 | UVLO divider; EN abs max VIN + 0.3 V |
| 3 | vin | VIN | D1.cathode, D2.cathode, C1/C2/C3 p1, R_UV_T.p1 | |
| 4 | rt_sync | RT | R_RT.p1 | R_RT 33k to GND sets 800 kHz; must not float (datasheet 8.3.4) |
| 5 | fb | FB | R_FB_T.p2, R_FB_B.p1, C7.p1 | feedback; abs max 5.5 V |
| 6 | pg | PG | R_PG.p1, J2.p3, D4.cathode | open drain; tolerates 20 V |
| 7 | boot | BOOT | C4.p1 | |
| 8 | sw | SW | L1.p1, C4.p2 | switch node (85 V abs max) |
| 9 | pad | GND | exposed pad 2.41 x 3.1 mm, six GND vias | |

## J1: JST B2B-XH-A, 2 pin (`jst_b2b_xh_a_2pin`)
Datasheet: https://www.jst-mfg.com/product/pdf/eng/eXH.pdf. Reviewed Steve Munene 2026-10-28. Pin-1 polarity is not stated in the catalogue (QUALIFICATION B8).

| Pin | Net | Note |
|---|---|---|
| 1 | VIN_RAW | +24 V |
| 2 | GND | |

## J2: JST B4B-XH-A, 4 pin (`jst_b4b_xh_a_4pin`)
Same datasheet and review as J1. Rotated 270 degrees in the sidecar so PG (pin 3) and +3V3 (pin 1) do not cross.

| Pin | Net | Note |
|---|---|---|
| 1 | +3V3 | 3.3 V out |
| 2 | GND | |
| 3 | PG | 3.3 V logic through the 100k pull-up; valid only after the 4 ms start-up delay |
| 4 | GND | |

## Two-terminal and discrete parts

| Ref | Part (datasheet URL in part file) | Pin 1 -> net | Pin 2 -> net | Notes |
|---|---|---|---|---|
| F1 | Bourns MF-MSMF110/33X, https://www.bourns.com/docs/Product-Datasheets/mfmsmf.pdf | p1 VIN_RAW | p2 VIN_FUSED | no polarity |
| D1 | Vishay SS36, https://www.vishay.com/docs/88751/ss32.pdf | cathode VIN | anode VIN_FUSED | pad 1 = cathode, pad 2 = anode per registry; footprint `Diode_SMD:D_SMC`, symbol `Device:D_Schottky`; pad-1-is-cathode against the footprint is "not verified" here |
| D2 | Vishay SMBJ30A, https://www.vishay.com/docs/88392/smbj.pdf | cathode VIN | anode GND | pad 1 = cathode per registry; symbol `Device:D_Zener` (no KiCad SMBJ30A symbol) |
| D3 | Lite-On LTST-C190KGKT, https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF | cathode GND | anode LED_PWR | datasheet and registry agree on polarity (QUALIFICATION review record) |
| D4 | same as D3 | cathode PG | anode LED_PG | lit while PG is low |
| L1 | Bourns SRN6045TA-6R8M, https://www.bourns.com/docs/Product-Datasheets/SRN6045TA.pdf | p1 SW | p2 +3V3 | |
| C1 | Honor Elec RVT1H470M0810, https://wmsc.lcsc.com/wmsc/upload/file/pdf/v2/lcsc/2109290630_Honor-Elec-RVT1H470M0810_C2895739.pdf | p1 (+) VIN | p2 GND | + on pin 1 follows the KiCad `CP_Elec_8x10` convention; datasheet does not state the marking: not verified |
| C2, C3 | generic 1206 / 0603 (no datasheet) | p1 VIN | p2 GND | |
| C4 | generic 0603 | p1 BOOT | p2 SW | |
| C5, C6, C9 | generic 1206 | p1 +3V3 | p2 GND | |
| C7 | generic 0603 | p1 FB | p2 +3V3 | across R_FB_T |
| R_UV_T | generic 0603 | p1 VIN | p2 EN | |
| R_UV_B | generic 0603 | p1 EN | p2 GND | |
| R_RT | generic 0603 | p1 RT | p2 GND | |
| R_FB_T | generic 0603 | p1 +3V3 | p2 FB | |
| R_FB_B | generic 0603 | p1 FB | p2 GND | |
| R_PG | generic 0603 | p1 PG | p2 +3V3 | |
| R_PGLED | generic 0603 | p1 +3V3 | p2 LED_PG | |
| R_LED | generic 0603 | p1 +3V3 | p2 LED_PWR | |
| H1, H2 | MountingHole_2.5mm (NPTH) | | | no net |

Resistor and capacitor pin 1/2 assignment follows the `connect`/`net` statements in the source; the generic passives are non-polar except as noted.
