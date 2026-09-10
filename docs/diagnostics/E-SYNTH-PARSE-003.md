# E-SYNTH-PARSE-003 — expected `{` to open board body

**Severity:** error
**Stage:** parse

## What this means

After the board name, the parser expected an opening brace `{` to begin
the board body.

## Minimal reproduction

```synth
board "a"
```

## Suggested fix

Insert `{` after the board name.

Patch primitive: `insert_at`.
