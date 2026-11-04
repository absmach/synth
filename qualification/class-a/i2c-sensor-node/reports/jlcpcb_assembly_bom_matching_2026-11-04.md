# JLCPCB assembly BOM matching, 2026-11-04

The orderable BOM of `fab/` (Gerber content hash `daa736de85`) through JLCPCB's BOM tool, 5 assembled boards, no order placed. The tool's own file is [`jlcpcb_assembly_bom_matching_2026-11-04.xlsx`](jlcpcb_assembly_bom_matching_2026-11-04.xlsx) (downloaded 2026-11-04 17:04 as `i2c_sensor_node_bom_orderable-JLCPCB Assembly Order.xls`; it is an `.xlsx` file). Estimated total price in the file: $124.80.

| Result | Lines |
|---|---|
| Matched by the system to the BOM's manufacturer part number | 53 of 55 |
| Unconfirmed | 1 (`SW1`) |
| No match | 1 (`J4`) |

The 53 matched lines carry the manufacturer part numbers and LCSC numbers of the orderable BOM (for example `C15850` for `C1`, `C2` and `C4`, `C545347` for the BZT52C3V3 zeners, `C125972` for the BME680 `U3`, `C411625` for the 24LC256 `U5`, `C6186` for the AMS1117 `U1`).

Exceptions, as the tool reports them:

| Ref | Part | The tool's result | The orderable BOM |
|---|---|---|---|
| `U2` | STMicroelectronics STM32G030F6P6, LCSC `C724040`, Extended | Matched; the Source column reads `0 JLCPCB` against a quantity of 5; unit price $7.4215 | LCSC `C724040` |
| `J5` | Harwin M20-9991246 12-pin header, LCSC `C5439732`, Extended, category Global Sourcing Parts | Matched; the Source column reads `0 JLCPCB` against a quantity of 5; unit price $2.526 | JLCPCB parts `C5439732` |
| `SW1` | the tool offers E-Switch TL1220S1BBSG, LCSC `C5798268`, a through-hole tactile switch, Extended, $21.23 | Unconfirmed: "the comment (TL1220S1BBSG-RESET) of this part does not match the one (RESET) provided in your BOM"; the Source column reads `0 JLCPCB` | C&K PTS645SM43SMTR92 LFS, a surface-mount switch, Mouser 611-PTS645SM43SMTR92 |
| `J4` | SWD header, 1x5, 2.54 mm | No match: "No Part Selected" | Harwin M20-9990546, Farnell 1022253 (also RS 681-2985, DigiKey 3728232), hand-fitted |

The quote ([`jlcpcb_quote_2026-11-04.md`](jlcpcb_quote_2026-11-04.md)) carries a hand-soldering labor fee of $3.61 and a manual assembly fee of $0.59.
