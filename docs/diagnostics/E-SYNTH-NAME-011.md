# E-SYNTH-NAME-011 — invalid colour

**Severity:** warning
**Stage:** lower

## What this means

A `color` attribute on a `netclass` or `group` block is not a six-digit hex colour. The class/region keeps its deterministic default palette hue, so the schematic still exports — only the requested hue is dropped.

## Minimal reproduction

```synth
board "x" {
  netclass "PWR" { color "not-a-colour" }
  group "Power" color "red" {
    component U1: regulator "ams1117_3v3"
  }
}
```

## Suggested fix

Write a `#rrggbb` value (the leading `#` is optional), e.g. `color "#c2410c"`.
