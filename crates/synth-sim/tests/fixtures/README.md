# ngspice fixtures

Every `.out`/`.err` pair is the raw stdout/stderr of a real
`/usr/bin/ngspice` 42 (`ngspice-42`) batch run with a `.spiceinit` containing
`set strict_errorhandling`. They are replayed by the Rust stub in
`tests/support/stub_ngspice.rs`; do not edit them by hand.

| stem | deck behaviour | exit |
| --- | --- | --- |
| `divider-op` | 12 V across two 10 kΩ resistors, `op`, sane sentinels | 0 |
| `divider-meas-failed` | `.meas tran vmax max v(ghost)` fails with `Error: measure ...` | 0 |
| `divider-missing-value` | `let m0 = v(ghost)`, so `SYNTH_MEAS vmid` has no value | 0 |
| `divider-missing-done` | control block ends without `echo SYNTH_DONE` | 0 |
| `divider-nan` | `let a = 1e308 * 1e308; let m0 = a - a`, which prints `-NAN` | 0 |
| `divider-error` | `X1 in 0 nosuchsub` — unknown subcircuit | 1 |
| `divider-singular` | two series capacitors; singular matrix warnings but exit 0 and a value | 0 |

`hang.cir` is a real ngspice deck whose control loop never ends. The stub
sleeps instead of replaying a log, so the runner's timeout and kill can be
tested.
