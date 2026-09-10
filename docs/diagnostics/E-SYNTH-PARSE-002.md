# E-SYNTH-PARSE-002 — expected board name (quoted string)

**Severity:** error
**Stage:** parse

## What this means

The `board` keyword must be followed by a quoted string literal naming
the board. The parser found something else (or end-of-file) at the
position where the name was expected.

## Minimal reproduction

```synth
board {}
```

## Suggested fix

Insert a placeholder name:

```synth
board "unnamed" {}
```

Patch primitive: `insert_at` at the position immediately after `board`.
