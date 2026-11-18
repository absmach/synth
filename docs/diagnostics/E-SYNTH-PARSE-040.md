# E-SYNTH-PARSE-040 — unexpected attribute inside escape

**Severity:** error
**Stage:** parse

## What this means

An `escape { … }` block on a component takes three things:

```synth
escape { method underpad  layers "In1.Cu", "In2.Cu" }
escape { off }
```

`method` takes a bare identifier, `layers` a comma-separated list of quoted
copper layer names, and `off` nothing at all.

Whether the named method is one this build has is a separate question,
answered by `E-SYNTH-PROCESS-004`.

## Minimal reproduction

```synth
board "b" {
  layers 4
  component U1: fpga "ice40hx4k_bg121" {
    escape { strategy underpad }
  }
}
```

## Suggested fix

Use `method`, `layers` or `off`. A quoted method (`method "underpad"`) is also
rejected: it is an identifier, not a string.
