# E-SYNTH-PARSE-046 — duplicate placement setting

A `placement` block sets `spacing` once. A second `spacing` setting in the
same block is reported; the first value stands.

## Example

```synth
board "twice" {
  layers 2
  placement {
    spacing = 2.0mm
    spacing = 1.0mm
  }
}
```

## Fix

Keep one `spacing` setting per `placement` block.

- **Code:** `E-SYNTH-PARSE-046`
- **Severity:** error
