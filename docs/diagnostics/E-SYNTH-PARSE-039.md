# E-SYNTH-PARSE-039 — expected the name of a required fabrication process

**Severity:** error
**Stage:** parse

## What this means

A `requires` statement is written as `requires <process>`, where the process is
a bare identifier. The parser found something else after the keyword — most
often a quoted string.

Whether the named process is one Synth knows about is a separate question,
answered later by `E-SYNTH-PROCESS-001`.

## Minimal reproduction

```synth
board "x" { requires "via_in_pad" }
```

## Suggested fix

Drop the quotes: `requires via_in_pad`.
