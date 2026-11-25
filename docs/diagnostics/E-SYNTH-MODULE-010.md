# E-SYNTH-MODULE-010 — deviation names a parameter the instance does not set

**Severity:** error
**Stage:** lower

## What this means

A `deviation` is attached to a parameter, but the `use` sets no value for
that parameter, so the deviation explains nothing. This usually means the
override was removed and its reason left behind, or the parameter name is
misspelt.

## Suggested fix

Remove the stale deviation, or restore the override it explains. For an
exception that is not one parameter, write a bare `deviation "reason"`.
