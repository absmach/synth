# E-SYNTH-NAME-011 — invalid netclass colour

**Severity:** warning
**Stage:** lower

## What this means

A `netclass "…"` block's `color` attribute is not a six-digit hex colour. The class keeps its deterministic default palette hue, so the schematic still exports — only the requested hue is dropped.

## Minimal reproduction

```synth
board "x" {
  netclass "PWR" {
    color "not-a-colour"
  }
}
```

## Suggested fix

Write a `#rrggbb` value (the leading `#` is optional), e.g. `color "#c2410c"`.
