# Disposition of the JLCPCB BOM-matching findings, Gerber content hash `ceeb7444dd`

The BOM matching of 2026-11-06 ([`jlcpcb_assembly_bom_matching_2026-11-06_99da25a419.md`](jlcpcb_assembly_bom_matching_2026-11-06_99da25a419.md), original [`.xlsx`](jlcpcb_assembly_bom_matching_2026-11-06_99da25a419.xlsx)) was made on `fab/ice40_bga_node_bom_orderable.csv`. That BOM and `fab/ice40_bga_node_cpl.csv` are byte-identical to the files of the package with Gerber content hash `ceeb7444dd` (SHA-256 `cb76c951ddd3e35b` and `313a1423707ae869`), so the findings below apply to it unchanged.

| Line | Finding | Disposition |
|---|---|---|
| `U1`, Lattice ICE40HX4K-BG121, LCSC `C1552775` | matched, 0 in stock for 5 boards (5 needed) | The part is identified and orderable: the report lists DigiKey `220-2147-N` for it, with the stock not shown on that page. The quantity is bought at order time; the package's identity data does not change with stock. |
| `Y1`, JYJE `3TJ480000FYLBC`, LCSC `C2924620` | matched, 3 in stock for 5 boards (5 needed) | The part is identified and its specification is in [`jyje_3tj480000fylbc_specification_2021-11-18.pdf`](jyje_3tj480000fylbc_specification_2021-11-18.pdf). The shortfall of 2 is bought at order time; no other distributor was searched. |
| `J4`, 1x5 SWD header, Harwin `M20-9990546` | no match | A through-hole header fitted by hand; its distributor line is Farnell `1022253` in the orderable BOM. |

No order was placed. The tool states that stock and prices change.
