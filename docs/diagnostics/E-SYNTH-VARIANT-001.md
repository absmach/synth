# E-SYNTH-VARIANT-001 — duplicate variant name

**Severity:** error
**Stage:** lower

## What this means

Two `variant "NAME" { … }` blocks share a name. A variant is identified
by its name (it becomes a KiCad design variant), so a duplicate is
ambiguous and the second one is dropped.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  variant "lite" { dnp U1 }
  variant "lite" { dnp U1 }
}
```

## Suggested fix

Rename one variant, or merge the two blocks.
