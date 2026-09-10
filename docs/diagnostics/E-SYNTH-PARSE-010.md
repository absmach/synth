# E-SYNTH-PARSE-010 — expected identifier

**Severity:** error
**Stage:** parse

## What this means

The parser expected an identifier (a name that starts with a letter or
`_` and continues with letters, digits, or `_`) but found something else.

## Minimal reproduction

```synth
board "x" {
  component "U1": mcu "rp2350"
}
```

## Suggested fix

Replace the offending token with a valid identifier. No automatic patch
is offered for this code because the correct identifier depends on
context.
