# E-SYNTH-PARSE-011 — expected statement keyword

**Severity:** error
**Stage:** parse

## What this means

Inside a board body, the parser expected one of the recognized statement
keywords: `layers`, `manufacturer`, `revision`, `component`, `connect`,
`diff_pair`, or `keepout`.

## Minimal reproduction

```synth
board "x" {
  banana
}
```

## Suggested fix

Replace the offending token with a valid statement, or delete the line.
No automatic patch is offered because the correct fix depends on intent.
