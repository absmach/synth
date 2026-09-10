# E-SYNTH-PARSE-030 — unexpected attribute inside placement_hint

**Severity:** error
**Stage:** parse

## What this means

An unknown attribute key was found inside a `placement_hint { ... }` block. Allowed attribute keys are `region`, `edge`, `near`, `side`, and `priority`.

## Minimal reproduction

```synth
board "broken" {
  component U1: mcu "stm32h743" {
    placement_hint {
      foo bar
    }
  }
}
```

## Suggested fix

Use only valid `placement_hint` attribute keywords (`region`, `edge`, `near`, `side`, `priority`).
