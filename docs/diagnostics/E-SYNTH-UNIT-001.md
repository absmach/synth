# E-SYNTH-UNIT-001 — unit conversion failure

**Severity:** error
**Stage:** ir-lower

## What this means

A `ValueWithUnit` from the parsed AST could not be lowered into the
IR's typed quantity. Subcategories:

1. **Wrong unit family.** An attribute that expects a length got a
   resistance unit (e.g. `radius 90ohm`).
2. **Invalid literal.** The numeric portion is not a parseable
   decimal.
3. **Overflow.** The value exceeds the integer base range. With i64
   bases this is unreachable for any physically reasonable PCB but
   is checked defensively.

## Minimal reproduction

```synth
board "x" {
  keepout antenna {
    radius 90ohm
  }
}
```

## Suggested fix

Replace with a unit appropriate to the attribute. The diagnostic's
`expected` field names the quantity that was wanted ("length",
"voltage", "resistance", ...).
