# E-SYNTH-PARSE-015 — expected manufacturer name (quoted string)

**Severity:** error
**Stage:** parse

## What this means

A `manufacturer` statement is written as `manufacturer "<name>"`. The
parser found something other than a quoted string after the keyword.

## Minimal reproduction

```synth
board "x" { manufacturer jlcpcb }
```

## Suggested fix

Wrap the manufacturer name in double quotes.
