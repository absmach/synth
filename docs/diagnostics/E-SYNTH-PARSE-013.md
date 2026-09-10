# E-SYNTH-PARSE-013 — expected component kind identifier

**Severity:** error
**Stage:** parse

## What this means

In `component U1: <kind> "<part>"`, the `<kind>` must be an identifier
naming a component class (e.g., `mcu`, `secure_element`, `modem`,
`charger`, `capacitor`, `resistor`).

## Minimal reproduction

```synth
board "x" {
  component U1: "rp2350"
}
```

## Suggested fix

Insert a kind identifier between `:` and the part string. No automatic
patch because the correct kind depends on the part.
