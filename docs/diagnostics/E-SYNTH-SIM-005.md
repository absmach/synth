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

A dense AC sweep on a large circuit, or a slow subcircuit model, is the usual
cause. A switching converter in transient would need a much larger budget,
but transient analysis itself is not implemented yet.

## Suggested fix

Raise `[options] timeout`, shorten the analysis window, or simplify the
circuit. A large circuit or a slow model may need a larger explicit timeout.
