# E-SYNTH-PARSE-033 — invalid `legends` value

**Severity:** error
**Stage:** parse

## What this means

A `legends` board statement names something other than `on` or `off`.

## Minimal reproduction

```synth
board "x" {
  legends maybe
}
```

## Suggested fix

Write `legends on` to emit compact connector pin legends, or `legends off` (the default) to leave them out. The diagnostic carries a patch replacing the word with `off`.
