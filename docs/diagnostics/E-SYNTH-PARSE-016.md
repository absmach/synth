# E-SYNTH-PARSE-016 — expected part identifier (quoted string)

**Severity:** error
**Stage:** parse

## What this means

A concrete component declaration includes a quoted part identifier:
`component U1: mcu "rp2350"`. The parser found something other than a
quoted string in that position.

## Minimal reproduction

```synth
board "x" { component U1: mcu rp2350 }
```

## Suggested fix

Wrap the part identifier in double quotes.
