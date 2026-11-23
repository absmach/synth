# E-SYNTH-SIM-003 — measurement failed

**Severity:** error
**Stage:** sim

## What this means

The generated deck promised a measurement but ngspice did not produce a
usable value. The causes are a missing `SYNTH_MEAS` sentinel, a sentinel with
no value, `nan` or `inf`, an unparseable number, a duplicated measurement, a
missing `SYNTH_DONE` terminator, an `Error: measure ...` line on stderr, or an
`Error: &m0: no such variable` from the generated `let`. A relative-dB
crossing that cannot be found is a failed `.meas` (`Error: measure ...`) and
is this code too. ngspice commonly exits 0 for all of these. A value is never
defaulted to 0.

A transient convergence failure aborts ngspice's control block, so
`SYNTH_DONE` is never printed; this code then carries ngspice's reason
(`doAnalyses: Timestep too small …`) in the message. The same is true of a
transient measure that cannot be evaluated over its window: a waveform that
never enters its settling band, never crosses its rise thresholds, or ends at
0 V where a percentage overshoot would be meaningless.

## Minimal reproduction

A measure whose vector cannot be evaluated:

```
echo SYNTH_MEAS vmid $&ghost
echo SYNTH_DONE
```

ngspice prints `SYNTH_MEAS vmid` with no value and still exits 0; Synth
reports this code. A failed `.meas` prints `Error: measure ...` and is the
same code.

## Suggested fix

Fix the testbench node name or the circuit so the measured vector exists, then
rerun. Never suppress the missing measurement.
