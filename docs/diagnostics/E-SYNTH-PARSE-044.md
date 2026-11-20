# E-SYNTH-PARSE-044 — `no_connect` names no pin

**Severity:** error
**Stage:** parse

## What this means

A `no_connect` statement was written without any `REFDES.PIN` endpoint.

## Minimal reproduction

```synth
board "x" {
  no_connect
}
```

## Suggested fix

Name the pins that are intentionally unused, separated by commas:

```synth
no_connect U1.cts, U1.dsr
```
