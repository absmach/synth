# E-SYNTH-PROCESS-001 — unknown required fabrication process

**Severity:** error
**Stage:** lowering

## What this means

A `requires` statement names a fabrication process this build does not know
about. The error lists the ones it does.

An error rather than a warning, because a requirement nothing can check is not
a weaker requirement — it is an unchecked one. A misspelled `requires` that
lowered quietly would reach the router as nothing at all, and the board would
read exactly like one that never asked.

Known requirements:

| Requirement | What the process must do |
| --- | --- |
| `via_in_pad` | Fill and cap a via drilled inside an SMD pad. |

## Minimal reproduction

```synth
board "b" {
  layers 4
  requires blind_vias
}
```

## Suggested fix

Correct the spelling, or drop the statement if the design does not in fact
need the process. A requirement is matched against the manufacturer profile's
capability: declaring one the declared tier does not offer is reported
separately, so `requires` alone never grants permission.
