# E-SYNTH-PARSE-022 — invalid escape sequence in string

**Severity:** error
**Stage:** parse (lexer)

## What this means

Inside a string literal, `\` introduces an escape. Phase 1 recognizes
`\\`, `\"`, `\n`, `\t`, `\r`. Any other character after `\` is an
error.

## Minimal reproduction

```synth
board "weird \q name" {}
```

## Suggested fix

Replace the escape with a valid one, or remove the backslash.
