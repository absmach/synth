# E-SYNTH-PROCESS-004 — unknown escape method

**Severity:** error
**Stage:** lowering

## What this means

A component's `escape { method … }` names an engine this build does not have.

| Method | What it does |
| --- | --- |
| `channel` | Route out between the pads, in the channels they leave. |
| `underpad` | Drop a via inside each pad and route away underneath. |

An error rather than a fallback to the default: a board asking for an engine
that does not exist is not a board that asked for the other one. At fine pitch
the two are not interchangeable — `channel` needs a gap between pads wide
enough for a track and its clearance, which a fully populated array does not
leave.

## Minimal reproduction

```synth
board "b" {
  layers 4
  component U1: fpga "ice40hx4k_bg121" {
    escape { method teleport }
  }
}
```

## Suggested fix

Name `channel` or `underpad`, or drop the `method` line and let the default
apply. `escape { off }` leaves the package to the general router entirely.
