# JLCPCB BOM Tool, BOM matching, 2026-11-04

Matched from `fab/irrigation_controller_bom_orderable.csv` (Gerber content hash `1c84c56d98`) in the JLCPCB assembly flow, PCB assembled quantity 5, estimated total price $92.8686, no order placed. The original file is [`jlcpcb_assembly_bom_matching_2026-11-04.xlsx`](jlcpcb_assembly_bom_matching_2026-11-04.xlsx).

| Result | Lines | Designators |
|---|---|---|
| Matched, in JLCPCB stock | 100 | including `C_IN1`, `C_IN2`, `C_N5` and `C_N7` (10 uF 25 V 0805, Murata `GRM21BR61E106KA73L`, `C84416`, Extended, 28 in stock) |
| Matched with 0 in stock | 2 | `R_COMP` (ROHM `MCR03EZPFX9101`, `C162000`, Extended), `Y1` (NDK `NX3225GD-8MHZ-STD-CRA-3`, `C528999`, Extended) |
| No valid match | 4 | `C_BULK` (`100uF25V`: the tool's suggestion `C2941301` is an SMD can, and the board uses the radial through-hole `CP_Radial_D6.3mm_P2.50mm`, so the match is not valid and `C_BULK` is a hand-fitted through-hole part), `J_SWD` (1x5 pin header, through-hole), `U4` (APDS-9301), `U5` (SOIC-8) |
