# E-SYNTH-PLACE-006 — duplicate placement block

A board declares at most one `placement` block. A second block is reported
and ignored; the first one stands. The same applies to a duplicate `spacing`
setting inside one block (reported by the parser).

## Example

```synth
board "twice" {
  layers 2
  placement { spacing = 2.0mm }
  placement { spacing = 1.0mm }
}
```

## Fix

Keep one `placement { spacing = ... }` block.

- **Code:** `E-SYNTH-PLACE-006`
- **Severity:** error
