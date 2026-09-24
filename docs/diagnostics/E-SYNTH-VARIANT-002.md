# E-SYNTH-VARIANT-002 — variant names an unknown component

**Severity:** error
**Stage:** lower

## What this means

A `variant` block marks a refdes do-not-populate that no `component`
statement declares. Either the refdes is misspelled or the component is
missing; the override is dropped so it cannot silently reference a
non-existent part.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  variant "lite" { dnp U9 }
}
```

## Suggested fix

Correct the refdes, or add the missing `component` declaration.
