# JLCPCB assembly BOM matching, 2026-11-04

The orderable BOM of `fab/` (Gerber content hash `daa736de85`) through JLCPCB's BOM tool, 5 assembled boards, no order placed. The tool's own file is [`jlcpcb_assembly_bom_matching_2026-11-04.xlsx`](jlcpcb_assembly_bom_matching_2026-11-04.xlsx) (downloaded 2026-11-04 17:10 as `i2c_sensor_node_bom_orderable-JLCPCB Assembly Order (1).xls`; it is an `.xlsx` file). Estimated total price in the file: $105.31.

| Result | Lines |
|---|---|
| Matched by the system to the BOM's manufacturer part number | 54 of 55 |
| No match | 1 (`J4`) |

Parts whose match the tool reports in detail:

| Ref | Part | The tool's result |
|---|---|---|
| `U2` | STMicroelectronics STM32G030F6P6TR, LCSC `C529330`, Extended | Matched; the Source column reads `5 JLCPCB` against a quantity of 5; unit price $7.2735 |
| `SW1` | C&K PTS645SM43SMTR92LFS, LCSC `C221880`, Extended, SMD-4P 6 x 6 mm | Matched; the Source column reads `5 JLCPCB` against a quantity of 5; unit price $1.8865 |
| `J5` | Harwin M20-9991246 12-pin header, LCSC `C5439732`, Extended, category Global Sourcing Parts | Matched; the Source column reads `0 JLCPCB` against a quantity of 5; unit price $2.526 |
| `J4` | SWD header, 1x5, 2.54 mm | No match: "No Part Selected". The orderable BOM names Harwin M20-9990546 from Farnell 1022253 (also RS 681-2985, DigiKey 3728232), hand-fitted |

The quote ([`jlcpcb_quote_2026-11-04.md`](jlcpcb_quote_2026-11-04.md)) carries a hand-soldering labor fee of $3.61 and a manual assembly fee of $0.59.
