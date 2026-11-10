# E-SYNTH-SIM-005 — ngspice timed out

**Severity:** error
**Stage:** sim

## What this means

ngspice did not finish within its wall-clock budget, so Synth killed it. The
default budget is 10 seconds; a testbench can override it with
`[options] timeout` (seconds, greater than 0 and at most 3600). A timed-out
run is never a pass, even if the process printed some measurements before the
deadline.

## Minimal reproduction

A testbench whose analysis cannot finish in the configured budget. Lower the
budget to make the timeout easy to observe:

```toml
[options]
timeout = 1.0
```

A long transient analysis is the usual cause: simulation time is not wall-clock
time, and a switching converter can need a much larger budget.

## Suggested fix

Raise `[options] timeout`, shorten the analysis window, or simplify the
circuit. Heavy transient work usually needs a larger explicit timeout.
