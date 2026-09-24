# E-SYNTH-SCHEM-015 — group has no notes

**Severity:** info
**Stage:** schematic aesthetic ERC

## What this means

A declared `group` carries no `notes` block. Every region on the reference sheet explains its intent in prose — voltage range, always-on tie-off, I²C address — text the netlist cannot carry.

Advisory only: a group without notes is under-documented, not wrong.

## Minimal reproduction

```synth
board "x" {
  group "3.3V LDO" {
    component U1: regulator "ams1117_3v3"
  }
}
```

## Suggested fix

Add a `notes` block inside the group:

```synth
group "3.3V LDO" {
  component U1: regulator "ams1117_3v3"
  notes "LDO notes" {
    "VIN = 3.3 - 5.5 V, EN tied to VIN (always on)"
  }
}
```
