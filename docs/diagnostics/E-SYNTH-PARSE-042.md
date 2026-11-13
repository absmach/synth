# E-SYNTH-PARSE-042 — outline dimension must be positive

**Severity:** error
**Stage:** parse

## What this means

Both dimensions of an `outline` statement must be greater than zero. A zero or
negative length would state a board with no area, so it is rejected where it was
written instead of reaching placement.

The unit is checked separately during lowering (`E-SYNTH-UNIT-001`).

## Minimal reproduction

```synth
board "x" { outline 0mm x 30mm }
```

## Suggested fix

Give both dimensions a positive length: `outline 60mm x 30mm`.
