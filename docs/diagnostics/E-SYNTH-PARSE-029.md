# E-SYNTH-PARSE-029 — unexpected keyword inside component body

**Severity:** error
**Stage:** parse

## What this means

An unexpected statement or keyword was found inside a component declaration block `{ ... }`. Component blocks currently only contain `placement_hint { ... }` definitions.

## Minimal reproduction

```synth
board "broken" {
  component U1: mcu "stm32h743" {
    unknown_keyword
  }
}
```

## Suggested fix

Remove the unrecognized keyword or place `placement_hint` statements inside the component block.
