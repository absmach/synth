# E-SYNTH-POWER-012 — part operated outside its declared operating envelope

**Severity:** error
**Stage:** erc — power

## What this means

A connector, diode or transistor sits on a declared power rail that runs
outside the envelope its registry entry declares (`operating_conditions`):
the rail's nominal voltage is above the part's `max_voltage_v`, or the
rail's declared current budget (the `current` its netclass declares) is
above the part's `max_current_ma`. `E-SYNTH-POWER-008` looks at pull-ups,
`009` at regulator inputs and `010` at regulator output budgets; this is
the same arithmetic for the parts those rules do not judge — the ones a
Class-A board review carries current and voltage ratings for.

A part without `[operating_conditions]` has nothing declared to violate,
so it is not checked.

## Minimal reproduction

```synth
board "x" {
  netclass "PWR12" { current 1.5A }
  component J1: connector "header_1x4"   // declared 250 V, 3 A — fine
  component F1: diode "d_zener_5v1"      // declared 4.8–5.4 V, 98 mA
  power "VIN12" 12v
  connect J1.p1 -> F1.cathode as "VIN12"
}
```

The zener is clamping a 12 V rail with a 5.1 V device: `F1.cathode` sits
on `VIN12`, whose 12 V is above the 5.4 V the part declares.

## Suggested fix

Move the part to a rail inside its envelope, or pick a part whose
datasheet covers the rail — a 12 V clamp needs `d_zener_12v`, not
`d_zener_5v1`. The envelope comes from the registry, so correcting an
understated entry there fixes every design that uses it.
