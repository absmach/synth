# E-SYNTH-PLACE-HINT-001 — hard placement hint conflict

**Severity:** error
**Stage:** place

## What this means

A hard placement hint (`priority: hard`) specified a region or edge constraint for a component that could not be satisfied without violating courtyard collision rules or board boundaries. The placer relaxed the constraint to prevent placement failure.

## Minimal reproduction

```synth
board "conflict" {
  component U1: mcu "stm32h743" {
    placement_hint { region: top_left priority: hard }
  }
}
```

## Suggested fix

Relax the placement hint priority from `hard` to `soft`, expand board dimensions, or move competing components to another region.
