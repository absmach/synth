# W-SYNTH-SUPPLY-002 — part has no distributor identity

**Severity:** warning
**Stage:** erc — board/supply

## What this means

A placed part carries neither `mpn` nor `lcsc_pn`. The exporter stamps hidden `MPN`/`LCSC` fields from exactly these registry fields, and the JLCPCB/DigiKey tooling reads those spellings — without them the BOM line cannot be quoted, stock-checked, or substituted, and `W-SYNTH-SUPPLY-001` has no part number to look up.

## Minimal reproduction

```synth
board "x" {
  component U3: sensor "bme680_env"   // registry entry has no mpn/lcsc_pn
}
```

## Suggested fix

Add `mpn` and/or `lcsc_pn` to the part's registry entry (`registry/parts/**/<id>.synth.toml`, or a Tier-2 project overlay for proprietary parts). Generic passives (`r_generic_*`, `c_generic_*`) are exempt — their value plus footprint is orderable as-is.

## See also

`W-SYNTH-SUPPLY-001` (cached stock/lifecycle for known part numbers).
