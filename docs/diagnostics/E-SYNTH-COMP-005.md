# E-SYNTH-COMP-005 — obsolete registry part

**Severity:** error
**Stage:** resolve

## What this means

The design selects a registry entry whose `lifecycle` is `obsolete`.
An obsolete entry may carry a stale pin map or sourcing data, so lowering
refuses it instead of silently laying out a different device.

## Minimal reproduction

```synth
board "x" {
  component U1: memory "24lc256_eeprom"
}
```

## Suggested fix

Replace the part with the live substitute named in the diagnostic
(`24lc256_soic8` for the EEPROM above) and re-check every pin name and
number against the new part.
