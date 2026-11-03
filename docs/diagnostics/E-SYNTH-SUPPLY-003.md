# E-SYNTH-SUPPLY-003 — malformed LCSC part number

**Severity:** error
**Stage:** erc — board/supply

## What this means

A component's own `lcsc_pn "..."` attribute is not an LCSC part number. LCSC numbers are `C` followed by digits (for example `C110776`). Registry-supplied values are not checked by this rule.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603" value "1k" lcsc_pn "110776"
}
```

## Suggested fix

Use the number exactly as LCSC prints it, including the leading `C`.

## See also

`W-SYNTH-SUPPLY-002` (part has no distributor identity).
