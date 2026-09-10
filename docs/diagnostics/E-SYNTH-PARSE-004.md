# E-SYNTH-PARSE-004 — expected `}` to close board body

**Severity:** error
**Stage:** parse

## What this means

The board body must be closed with a matching `}`. The Phase 0 stub
parser does not yet accept statements inside the body — any non-whitespace
content between `{` and `}` will produce this diagnostic. Phase 1 will
replace the stub with a full statement parser, at which point this
diagnostic will fire only for genuinely unterminated boards.

## Minimal reproduction

```synth
board "a" {
```

## Suggested fix

Insert `}` at the appropriate position.

Patch primitive: `insert_at`.
