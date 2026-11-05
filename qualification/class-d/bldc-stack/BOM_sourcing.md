# BOM and sourcing: BLDC stack (SYN-408)

The machine-readable BOM for each board is `kicad/control/bom.csv` and
`kicad/power/bom.csv`. This file is the sourcing summary a buyer needs; it is
not a substitute for the BOM.

## Control board (61 components)

| Refdes | Part | MPN | LCSC | Note |
| --- | --- | --- | --- | --- |
| U1 | STM32G030K8T6, LQFP-32 | STM32G030K8T6 | — | reviewed registry part; no LCSC number asserted |
| U_LDO | AMS1117-3.3, SOT-223 | AMS1117-3.3 | C6186 | seed part |
| U_RS485 | THVD1450D, SOIC-8 | THVD1450D | C2671361 | seed part |
| J_B2B | 2 x 10, 2.54 mm plug | 2.54-2*10 | C5116480 | authored; 2.5 A/contact |
| J_HALL_U/V/W, J_ENC, J_SWD, J_RS485 | 1 x 3 / 1 x 4 headers | — | — | generic seed parts, no MPN |
| D_LED_STATUS | LTST-C190KGKT green LED | LTST-C190KGKT | — | reviewed registry part |
| D_LED_FAULT | LTST-C190KRKT red LED | LTST-C190KRKT | — | seed part |
| D_RS485_A/B | PTVS6V0S1UR, SOD-123W | PTVS6V0S1UR | — | seed part |
| SW_BOOT | PTS645 tactile switch | PTS645SM43SMTR92 LFS | C221880 | seed part |
| L_B3V3 | 0603 ferrite bead | — | — | generic seed part |
| R_*, C_* | 0603/1206 resistors and MLCCs | — | — | generic seed parts; values are in the source, no MPN |
| H1-H4 | M2.5 non-plated hole | — | — | mechanical |

## Power board (111 components)

| Refdes | Part | MPN | LCSC | Note |
| --- | --- | --- | --- | --- |
| U_BUCK | LMR38010SDDAR, HSOP-8 | LMR38010SDDAR | — | reviewed registry part |
| U_GD_U/V/W | IR2101STRPBF, SOIC-8 | IR2101STRPBF | C2956 | authored |
| Q_UH..Q_WL | BSC030N08NS5, TDSON-8 | BSC030N08NS5ATMA1 | C501507 | authored; 80 V 100 A |
| Q_REV, Q_GE | IRF4905PBF, TO-220 | IRF4905PBF | C2564 | authored |
| U_CS1/2 | MCP6022-I/SN, SOIC-8 | MCP6022-I/SN | C34047 | authored |
| Q_FAULT, Q_GE_CTRL | 2N3904, TO-92 | 2N3904 | C20505 | seed part; datasheet read 2026-11-03, 40 V/200 mA |
| D_TVS | SMBJ30A, SMB | SMBJ30A | — | seed part |
| D_REV_Z | BZT52C15, SOD-123 | BZT52C15 | C2104 | authored |
| D_BOOT_U/V/W | 1N5819HW-7-F, SOD-123 | 1N5819HW-7-F | — | seed part |
| C_BULK1/2 | 470 uF 50 V radial, 12.5 x 20 mm | EEU-FR1H471 | C1589760 | authored |
| J_PWR | MKDS 1,5/2-5,08, 2-pin | 1715721 | C480516 | authored; datasheet read 2026-11-03 with Firecrawl; DigiKey 277-1263-ND |
| J_MOTOR | MKDS 1,5/3-5,08, 3-pin | 1715734 | C480520 | authored; datasheet read 2026-11-03 with Firecrawl; DigiKey 277-1264-ND |
| J_B2B | 2 x 10, 2.54 mm socket | 2.54-2*10P | C30867 | authored; 2.5 A/contact |
| R_SH_U/V/W | 2512 5 mOhm 1 % | — | — | generic seed part; 1 W assumed, not in a datasheet |
| R_*, C_*, L_* | 0603/1206 passives and 6.8 uH inductor | — | — | generic seed parts; L_BUCK is Bourns SRN6045TA-6R8M |

## Sourcing gaps

- The Phoenix MKDS terminal blocks are LCSC C480516 (2-pin) and C480520
  (3-pin), matched by MPN in the JLCPCB catalogue. JLC's listing says 250 V
  while Phoenix says 400 V (III/2); the 24 V rail is below both. DigiKey
  277-1263-ND / 277-1264-ND remain the distributor cross-reference.
- The generic passives have no MPN or LCSC number. This is the same gap the
  Class-B boards carry; a buyer has to choose equivalents by value, dielectric
  and voltage.
- The STM32G030K8T6, THVD1450D, MCP6022 and the LEDs have no LCSC number in the
  registry even where the part is common. They were not invented.
- The 470 uF 50 V bulk capacitor is 12.5 mm diameter; two of them and the
  2 x 10 connector are the parts that set the power board's width.
- No part was purchased, no availability or lead time was checked, and no
  second source was qualified.

## Assembly notes

- Both boards are 4-layer with 0.07 mm (2 oz) outer copper. The power board
  needs the 2 oz for the phase and ground copper; the control board uses the
  same stackup so the stack has one fabrication recipe.
- The BSC030N08NS5 is a PG-TDSON-8 (SuperSO8 5 x 6) with the drain on the large
  pad. Reflow, not hand soldering, is assumed.
- The 470 uF capacitors are polarised; pin 1 is positive. The AMS1117 and the
  LEDs are also polarised. Check the fab's rotation convention before ordering.
- J_B2B is through-hole. The control board carries the plug and the power board
  the socket; both are vertical and are placed at the same X/Y.
- No via-in-pad or filled-via process is declared. The thermal vias under the
  MOSFETs are ordinary through vias in the drain copper, not in a paste pad.
- The 24 V input terminal is rated 17.5 A but the board is stated for 10 A; no
  fuse is fitted, so the 10 A limit must come from the external supply.
