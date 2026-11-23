# E-SYNTH-PLACE-005 — placement spacing outside the supported range

The `placement { spacing }` setting is the least courtyard-to-courtyard gap
between any two parts. The placer is tuned for **1.0 mm to 5.0 mm**; a value
outside that range is a blocking geometry diagnostic.

## Example

```synth
board "spaced" {
  layers 2
  placement { spacing = 0.5mm }
}
```

## Fix

Set `spacing` between `1.0mm` and `5.0mm`, or remove the block to keep the
placer's own clearances. `export-kicad --spacing <mm>` accepts the same range
and rejects other values before placement.

- **Code:** `E-SYNTH-PLACE-005`
- **Severity:** error
