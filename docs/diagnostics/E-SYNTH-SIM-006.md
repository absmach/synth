# E-SYNTH-SIM-006 — ngspice error

**Severity:** error
**Stage:** sim

## What this means

ngspice exited unsuccessfully, or it printed an error or a convergence warning
even though it exited 0. The diagnostic message carries the exit status and an
excerpt of ngspice's stderr. A run that exits non-zero, or that reports a
singular matrix, a failed gmin step or a failed source step, is never treated
as a pass: ngspice can recover just enough to print a plausible number.

## Minimal reproduction

A deck that references an unknown subcircuit makes ngspice exit 1:

```
X1 n_mid 0 nosuchsub
```

Two capacitors in series make the operating point singular. ngspice prints
`Warning: singular matrix`, falls back to a transient op, prints a value and
exits 0. A series-capacitor design is caught by `E-SYNTH-SIM-008` before the
run; this code still catches a singular matrix from any other cause.

## Suggested fix

Read the stderr excerpt. Common causes are an unknown subcircuit, a missing
model file, a floating node, or a malformed generated line. Fix the design,
testbench or model and rerun. Do not add `rshunt` to hide a singular matrix.
