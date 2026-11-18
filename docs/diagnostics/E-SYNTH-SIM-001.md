# E-SYNTH-SIM-001 — ngspice not found

**Severity:** error
**Stage:** sim

## What this means

`synth sim` could not start the ngspice binary. No simulation ran, so no
measurement can pass. The default lookup is `NGSPICE`, then `ngspice` on
`PATH`.

## Minimal reproduction

Run `synth sim` on a design whose sidecar is valid with ngspice absent:

```sh
synth sim examples/divider/divider.synth
```

## Suggested fix

Install ngspice (`sudo apt install ngspice`, `brew install ngspice`, or the
Windows installer), or set `NGSPICE` to the binary path.
