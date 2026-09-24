# E-SYNTH-MODULE-003 — unknown module port

**Severity:** error
**Stage:** lower

## What this means

An instantiation binds a port the module does not declare, or
addresses a member of an interface port that the interface does not
have. Port names are the identifiers in the module's port list, e.g.
`module "SensorChannel" (vdd: power, i2c: I2C) { … }`; for an
interface port, the member must exist on the interface
(`interface "I2C" (sda: i2c_sda)`).

## Minimal reproduction

```synth
board "x" {
  module "M" (a: input) {
    component R1: resistor "r_generic_0603"
    connect a -> R1.p1
  }
  use "M" as X {
    a -> "IN"
    b -> "OUT"
  }
}
```

## Suggested fix

Bind a declared port name. For an interface port, bind the whole
bundle with one clause (`i2c -> "BUS0"`) or address a declared member
(`i2c.sda -> "BUS0.sda"`).
