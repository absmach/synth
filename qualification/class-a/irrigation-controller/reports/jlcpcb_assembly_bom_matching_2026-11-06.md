# JLCPCB BOM Tool, BOM matching, 2026-11-06

Matched from `fab/irrigation_controller_bom_orderable.csv` (Gerber content hash `1c84c56d98`) in the JLCPCB assembly flow, PCB assembled quantity 5, estimated total price $90.9978, no order placed. The original file is [`jlcpcb_assembly_bom_matching_2026-11-06.xlsx`](jlcpcb_assembly_bom_matching_2026-11-06.xlsx).

| Result | Lines | Designators |
|---|---|---|
| Matched, stock covers 5 boards | 99 | all with a JLCPCB part number and product page |
| Matched, stock below the quantity for 5 boards | 3 | `R_COMP` (ROHM `MCR03EZPFX9101`, `C162000`, 0 in stock, 10 needed), `L1` (1 in stock, 7 needed), `Y1` (JYJE `3TJ480000FYLBC`, `C2924620`, Extended, 3 in stock, 5 needed) |
| No valid match | 4 | `C_BULK` (`100uF25V`: the tool's suggestion `C2941301` is an SMD can, and the board uses the radial through-hole `CP_Radial_D6.3mm_P2.50mm`, so the match is not valid and `C_BULK` is a hand-fitted through-hole part), `J_SWD` (1x5 pin header, through-hole), `U4` (APDS-9301), `U5` (SOIC-8) |

`Y1` matches JLCPCB's own listing of the JYJE 3TJ480000FYLBC (8 MHz, 8 pF, SMD3225-4P). The stock figures are those of the match run at 2026-11-06 12:00 (JLCPCB's download time); the tool states that stock and prices change.
