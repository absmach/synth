# E-SYNTH-COMP-002 — undefined pin

**Severity:** error
**Stage:** resolve

## What this means

An endpoint in a `connect` statement references a pin name that does
not exist on the resolved part definition.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  component U2: secure_element "atecc608"
  connect U1.does_not_exist -> U2.sda
}
```

## Suggested fix

The resolver suggests the three closest matching pin names by
Levenshtein distance (≤3), each shipped as a `replace_range` patch.
