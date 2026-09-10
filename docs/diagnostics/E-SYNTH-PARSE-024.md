# E-SYNTH-PARSE-024 — invalid number literal

**Severity:** error
**Stage:** parse (lexer)

## What this means

The lexer began parsing what looked like a number but produced something
it could not interpret as either an integer or a decimal.

## Minimal reproduction

A number followed by an unparseable form (e.g., overflow):

```synth
board "x" { layers 99999999999999999999 }
```

## Suggested fix

Use a valid integer or decimal literal.
