# Assembly dispositions, Gerber content hash `1c84c56d98`

For the package of `fab/` (Gerbers, BOM and CPL), against the JLCPCB BOM matching of 2026-11-06 ([`jlcpcb_assembly_bom_matching_2026-11-06.md`](jlcpcb_assembly_bom_matching_2026-11-06.md)): 102 of 106 lines match a JLCPCB part, 3 of those have stock below the quantity for 5 boards, and 4 have no valid match. Each line below has its disposition. The BOM is unchanged by this file.

## The four lines with no valid match

| Ref | Part | Why there is no match | Disposition |
|---|---|---|---|
| `C_BULK` | NIC Components NRSZ101M25V6.3X11F, 100 uF 25 V radial electrolytic, 6.3 x 11 mm, 2.5 mm pitch | the tool's suggestion `C2941301` is an SMD can; the board uses the radial through-hole `CP_Radial_D6.3mm_P2.50mm` | through-hole, hand-fitted; supplied from RS 7374159 |
| `J_SWD` | Harwin M20-9990546, 1x5 pin header, 2.54 mm, through-hole | through-hole connector | hand-fitted; supplied from Farnell 1022253 (also RS 681-2985, DigiKey 3728232) |
| `U4` | Broadcom APDS-9301-020, 6-pin ChipLED | no LCSC or JLCPCB listing of this part was found (the nearest, the APDS-9007-020, is a different sensor) | hand-fitted; supplied from Farnell 2497389 |
| `U5` | Microchip 24LC256-I/SN, SOIC-8, 3.9 mm | the BOM names RS 8895102 as the order number, and the tool did not match it; the same MPN is listed at LCSC as `C411625` ([page](https://www.lcsc.com/product-detail/C411625.html)), while JLCPCB's `C5458` is the 24LC256T-I/SN, a different order code | supplied from RS 8895102 in this package; the LCSC listing of the same MPN is the alternative if the BOM is changed to it |

## The lines with stock below the quantity for 5 boards

| Ref | Part | Stock at JLCPCB (2026-11-06) | Disposition |
|---|---|---|---|
| `R_COMP` | ROHM MCR03EZPFX9101, 9.1 k 0603 (`C162000`) | 0 against 10 needed | the order needs JLCPCB to source the part or the builder to supply it; the BOM line is unchanged |
| `L1` | Taiyo Yuden CBC3225T100MR, 10 uH 1210 (`C90315`) | 1 against 7 | as above |
| `Y1` | JYJE 3TJ480000FYLBC (`C2924620`) | 3 against 5 | as above |

The stock figures are those of the match run (JLCPCB's download time 2026-11-06 12:00); the tool states that stock and prices change.
