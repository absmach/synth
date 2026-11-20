# E-SYNTH-PARSE-041 — expected `x` between the outline width and height

**Severity:** error
**Stage:** parse

## What this means

An `outline` statement is written as `outline <width> x <height>`, with a bare
`x` between the two lengths. The parser found something else after the width —
usually the height on its own, or a word such as `by`.

## Minimal reproduction

```synth
board "x" { outline 60mm 30mm }
```

## Suggested fix

Put an `x` between the dimensions: `outline 60mm x 30mm`.
