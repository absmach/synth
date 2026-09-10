# E-SYNTH-COMP-003 — duplicate component refdes

**Severity:** error
**Stage:** resolve

## What this means

Two `component` declarations share the same refdes. Refdeses must be
unique within a board so that `connect` endpoints resolve
unambiguously.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  component U1: secure_element "atecc608"
}
```

## Suggested fix

Rename one of the duplicate refdeses. No automatic patch is offered
because the correct rename depends on intent.
