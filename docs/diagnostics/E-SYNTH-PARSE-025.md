# E-SYNTH-PARSE-025 — unterminated block comment

**Severity:** error
**Stage:** parse (lexer)

## What this means

A `/* ... */` block comment was opened but the file ended before the
closing `*/`. Block comments do not nest in Phase 1.

## Minimal reproduction

```synth
/* missing closer
board "x" {}
```

## Suggested fix

Add the closing `*/` at the appropriate position.
