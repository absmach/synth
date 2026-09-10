# E-SYNTH-PARSE-005 — unexpected trailing input after board

**Severity:** error
**Stage:** parse

## What this means

A SynthSpec program contains exactly one board declaration. Content
after the closing `}` of the board is not valid syntax.

## Minimal reproduction

```synth
board "a" {} extra
```

## Suggested fix

Remove the trailing content.

Patch primitive: `delete_range`.
