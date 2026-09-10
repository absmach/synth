# W-SYNTH-HINT-001 — unknown placement hint attribute value

**Severity:** warning
**Stage:** lower

## What this means

A placement hint attribute (`region`, `edge`, `side`, or `priority`) contained an unrecognised value. The warning is emitted and lowering continues with default or soft priority fallback.

## Minimal reproduction

```synth
board "warning" {
  component U1: mcu "stm32h743" {
    placement_hint {
      region invalid_region_name
    }
  }
}
```

## Suggested fix

Use a valid region, edge, side, or priority identifier (for example `region top_left` or `priority hard`).
