# E-SYNTH-CONNECT-004 — two outputs driving the same net

**Severity:** error
**Stage:** erc — connectivity

## What this means

Two pins with electrical type `output` were placed on the same net. When both drive opposing levels, one of them will source destructive current.

## Minimal reproduction

```synth
board "x" {
  component U1: ic "sn74hc595_shift"
  component U2: ic "sn74hc595_shift"
  connect U1.qa -> U2.qa  // two QA outputs colliding
}
```

## Suggested fix

Insert a multiplexer, use a bus arbiter, or change one of the pins to `open_drain` with an explicit pullup.
