# E-SYNTH-PARSE-043 — `anchor` needs a component name

**Severity:** error
**Stage:** parse

## What this means

A `keepout` used `anchor` without the quoted reference designator that names
the component the rule area is centred on.

## Minimal reproduction

```synth
board "x" {
  keepout hot_buck { radius 5mm anchor }
}
```

## Suggested fix

Give the anchor a quoted refdes, or use `at` with a board coordinate:

```synth
keepout hot_buck { radius 5mm anchor "U3" }
```
