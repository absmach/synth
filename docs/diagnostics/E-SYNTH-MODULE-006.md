# E-SYNTH-MODULE-006 — unknown bus or interface in bind

**Severity:** error
**Stage:** lower

## What this means

A `bind` names a bus or interface that is not declared, addresses a
member the bus does not have, or binds an interface port to something
other than a declared bus. A `bind` ties a declared bus to an
interface so that each bus member connects to the interface member of
the same name.

## Minimal reproduction

```synth
board "x" {
  interface "I2C" (sda: i2c_sda)
  component J1: connector "jst_ph_2pin"
  bind "NOPE" : I2C {
    sda -> J1.p1
  }
}
```

## Suggested fix

Declare the bus (`bus "NOPE" (sda, scl)`), declare the interface, and
make sure the bus declares every member the interface names.
