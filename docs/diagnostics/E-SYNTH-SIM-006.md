# E-SYNTH-SIM-006 — ngspice error

**Severity:** error
**Stage:** sim

## What this means

ngspice exited unsuccessfully, or Synth could not prepare or run the process.
The diagnostic message carries the exit status and an excerpt of ngspice's
stderr. A run that exits non-zero is never treated as a pass.

## Minimal reproduction

A deck that references an unknown subcircuit makes ngspice exit 1:

```
X1 n_mid 0 nosuchsub
```

## Suggested fix

Read the stderr excerpt. Common causes are an unknown subcircuit, a missing
model file, or a malformed generated line. Fix the design, testbench or model
and rerun.
