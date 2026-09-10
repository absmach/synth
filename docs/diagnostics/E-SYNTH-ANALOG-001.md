# E-SYNTH-ANALOG-001 — analog pin mixed with digital output

**Severity:** warning
**Stage:** erc — analog

## What this means

An `analog` electrical-type pin shares a net with a digital `output` pin. The fast edges of the digital driver couple noise into the analog signal.

## Minimal reproduction

(intentionally omitted — relies on a part with `electrical_type = "analog"`, none present in the V1 seed registry)

## Suggested fix

Buffer the analog signal through an op-amp follower, or keep the analog and digital nets physically separate.
