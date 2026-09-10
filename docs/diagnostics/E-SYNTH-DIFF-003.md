# E-SYNTH-DIFF-003 — Differential pair leg not fully connected

**Severity:** error
**Stage:** erc — protocol

## What this means

A differential pair was declared, but one or both of the leg nets (positive or negative) does not have at least two endpoints connected. Differential signals require both positive and negative lines to be routed between endpoints.

## Minimal reproduction

```synth
board "diff_legs_test" {
  layers 2
  diff_pair DP_P DP_N {
    impedance 90ohm
  }
}
```

## Suggested fix

Ensure both the positive (`DP_P`) and negative (`DP_N`) legs of the differential pair have active net connections between endpoints.
