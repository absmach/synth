# E-SYNTH-PARSE-021 — unterminated string literal

**Severity:** error
**Stage:** parse (lexer)

## What this means

A string literal began with `"` but no matching closing `"` was found
before the end of the line or the end of the file. SynthSpec strings
do not span lines.

## Minimal reproduction

```synth
board "x
```

## Suggested fix

Add a closing `"`. Patch primitive: `insert_at` at the end of the string.
