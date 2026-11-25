# E-SYNTH-PARSE-047 — expected a quoted, non-empty outline reason

**Severity:** error
**Stage:** parse

## What this means

An `outline` statement may state why the size is fixed:
`outline <width> x <height> reason "<text>"`. After `reason` the parser expects
a quoted string with at least one non-whitespace character. It found either no
string or an empty one.

## Minimal reproduction

```synth
board "x" { outline 60mm x 30mm reason "" }
```

## Suggested fix

State a non-empty reason, for example `reason "fits the enclosure"`, or drop
the `reason` attribute. A stated reason suppresses `W-SYNTH-BOARD-004`.
