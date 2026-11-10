# ngspice fixtures

Every `.out`/`.err` pair is the raw stdout/stderr of a real
`/usr/bin/ngspice` 42 (`ngspice-42`) batch run with a `.spiceinit` containing
`set strict_errorhandling`. The tests compile the stub source in
`tests/support/stub_ngspice.rs` with `rustc` and replay these logs; do not
edit them by hand.

| stem | deck behaviour | exit |
| --- | --- | --- |
| `divider-op` | 12 V across two 10 kΩ resistors, `op`, sane sentinels | 0 |
| `led-op` | 5 V through 150 Ω into a 2.0 V / 20 mA LED, `op` | 0 |
| `diode-op` | 5 V through 430 Ω into a 0.7 V / 10 mA diode, `op` | 0 |
| `ldo-op` | 5 V into the AMS1117-3.3 `[spice]` model with a 1 kΩ load, `op` | 0 |
| `divider-meas-failed` | `.meas tran vmax max v(ghost)` fails with `Error: measure ...` | 0 |
| `divider-missing-value` | `let m0 = v(ghost)`, so `SYNTH_MEAS vmid` has no value | 0 |
| `divider-missing-done` | control block ends without `echo SYNTH_DONE` | 0 |
| `divider-nan` | `let a = 1e308 * 1e308; let m0 = a - a`, which prints `-NAN` | 0 |
| `divider-error` | `X1 in 0 nosuchsub` — unknown subcircuit | 1 |
| `divider-singular` | two series capacitors; singular matrix warnings but exit 0 and a value | 0 |
| `divider-singular-no-done` | the same singular run without `echo SYNTH_DONE` | 0 |
| `divider-singular-missing-value` | singular matrix plus a `let` on a missing vector, so stderr has both warnings | 0 |

`hang.cir` is a real ngspice deck whose control loop never ends. The stub
sleeps instead of replaying a log, so the runner's timeout and kill can be
tested.
