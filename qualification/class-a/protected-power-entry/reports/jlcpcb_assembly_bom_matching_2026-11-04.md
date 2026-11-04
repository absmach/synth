# JLCPCB BOM Tool, BOM matching, 2026-11-04

Matched from `fab/protected_power_entry_bom_orderable.csv` (Gerber content hash `76a311a42f`) in the JLCPCB assembly flow, PCB assembled quantity 5, estimated total price $63.9945, no order placed. The original file is [`jlcpcb_assembly_bom_matching_2026-11-04.xlsx`](jlcpcb_assembly_bom_matching_2026-11-04.xlsx), downloaded 2026-11-04 20:35:50 (JLCPCB clock).

| Result | Lines | Designators |
|---|---|---|
| Matched, in JLCPCB stock | 65 | every SMD line except R3 |
| Matched to an SMD part with 0 in stock | 3 | C1, C10, C17 (`100uF25V`, JLCPCB Assembly `C2941301`, a 6.3 mm SMD can; the board footprint is the radial through-hole `CP_Radial_D6.3mm_P2.50mm`) |
| Unconfirmed | 1 | R3, 44.2k, `0603WAF4422T5E`, JLCPCB `C23056`, 0 in stock, comment differs from the BOM value (the BOM matched here lists this part; the package BOM now lists KOA `RK73H1JTTD4422F`, `C317170`) |
| No match | 1 | J2, 1x8 pin header, 2.54 mm, vertical (through-hole) |

The matched lines are Basic or Extended parts with the JLCPCB part numbers of the file. D9 and D10 match Micro Commercial Components BZT52C5V1-TP (`C78728`, 15 in stock).
