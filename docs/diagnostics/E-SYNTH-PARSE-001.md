# E-SYNTH-PARSE-001 — expected `board` keyword

**Severity:** error
**Stage:** parse

## What this means

A SynthSpec program must begin with a `board` declaration (after any
`import` statements). The parser found something else at the position
where it expected the `board` keyword.

## Minimal reproduction

```synth
xyz
```

## Suggested fix

Insert a minimal board declaration:

```synth
board "unnamed" {}
```

Patch primitive: `insert_at` at byte 0.
