# E-SYNTH-PARSE-029 — unexpected keyword inside component body

**Severity:** error
**Stage:** parse

## What this means

An unexpected statement or keyword was found inside a component declaration block `{ ... }`. Component blocks accept `placement_hint { ... }`, the structured-value fields (`tolerance`, `voltage`, `power_rating`, `dielectric`), and `dnp`.

## Minimal reproduction

```synth
board "broken" {
  component U1: mcu "stm32h743" {
    unknown_keyword
  }
}
```

## Suggested fix

Remove the unrecognized keyword, or use one of `placement_hint`, `tolerance`, `voltage`, `power_rating`, `dielectric`, `dnp`.
