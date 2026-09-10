# E-SYNTH-PARSE-028 — expected placement hint identifier

**Severity:** error
**Stage:** parse

## What this means

A placement hint attribute keyword (`region`, `edge`, `near`, `side`, `priority`) was not followed by a valid identifier string.

## Minimal reproduction

```synth
board "broken" {
  component U1: mcu "stm32h743" {
    placement_hint {
      region
    }
  }
}
```

## Suggested fix

Provide a valid identifier after the attribute keyword, for example `region top_left` or `region: top_left`.
