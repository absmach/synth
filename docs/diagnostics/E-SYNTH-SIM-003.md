# E-SYNTH-SIM-003 — measurement failed

**Severity:** error
**Stage:** sim

## What this means

The generated deck promised a measurement but ngspice did not produce a
usable value. The causes are a missing `SYNTH_MEAS` sentinel, a sentinel with
no value, `nan` or `inf`, an unparseable number, a duplicated measurement, a
missing `SYNTH_DONE` terminator, an `Error: measure ...` line on stderr, or an
`Error: &m0: no such variable` from the generated `let`. ngspice commonly
exits 0 for all of these. A value is never defaulted to 0.

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
