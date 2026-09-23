# E-SYNTH-PARSE-031 — placement_hint names an unknown component

**Severity:** error
**Stage:** parse — placement

## What this means

A board-level `placement_hint` names a component reference designator
that was never declared with a `component` statement. The hint cannot
be attached to anything, so the parser reports it instead of silently
discarding the constraint.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  placement_hint { component: "U9" edge: right }
}
```

## Suggested fix

Declare the named component, or correct the `component:` value to an
existing refdes such as `U1`.
