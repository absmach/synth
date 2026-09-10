# E-SYNTH-PARSE-006 — unexpected character

**Severity:** error
**Stage:** parse (lexer)

## What this means

The lexer encountered a character that is not part of any valid token
in the current position.

## Minimal reproduction

```synth
board @ "x" {}
```

## Suggested fix

Remove the offending character.

Patch primitive: `delete_range` covering the single character.
