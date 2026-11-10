# E-SYNTH-SIM-002 — part has no SPICE model

**Severity:** error
**Stage:** sim

## What this means

A placed part cannot be represented in the generated SPICE deck. Synth names
the reference and part id, so the uncovered part is visible in the report.

Built-in models exist for resistors, capacitors, inductors, and diodes/LEDs
that declare a forward voltage and current. A diode or LED must also name its
pins `anode` and `cathode`; a two-pin part with any other pin names is reported
here instead of guessing which way round it goes.

A part whose `kind` is `diode` but which is not an LED (any diode-kind part,
zener/TVS included) should carry a registry `[spice]` model. Without one it uses
the plain built-in diode model when it declares the forward fields, which does
not represent zener or TVS behaviour.

The value is part of the model: an absent, unparseable, zero or negative
resistor/capacitor/inductor value is reported here too. `4k7` is not accepted;
write `4.7k`. Inductance values are in henries (`10uH`, `4.7nH`).

A part the testbench lists under `ignore` is left out of the deck and this
report. That is for a known gap, not for hiding an accident: the part also
stops conducting in the topology check, so removing a part that was the only
DC path reports `E-SYNTH-SIM-008`.

## Registry `[spice]` models

```toml
[spice]
file = "spice/ams1117_3v3.lib"
subckt = "ams1117_3v3"
pin_order = ["vin", "gnd", "vout"]
dialect = "ps"          # optional: ps, lt or ltps

[spice.params]
rout = "100m"
```

A `[spice]` table wins over every built-in model, so a part of any kind uses
its subcircuit. `pin_order` lists the part's pin names in subcircuit terminal
order; it must name every pin exactly once, or the deck is rejected here.
`file` is resolved relative to a registry directory (an absolute path is
accepted only inside one), and the model file must define the named `.subckt`.
A missing file or subcircuit is reported only when a simulation runs, so
`validate` and `export` do not fail on it.

When a deck runs, the model file is checked: `.control`, `.endc`, `shell` and
`system` lines are rejected, as are Verilog-A models and encrypted vendor
files. v1 also rejects nested `.include`, `.inc` and `.lib` directives, so a
model must be self-contained. All parts in one run must use the same `dialect`,
because ngspice has a single `ngbehavior` setting per run; a conflict is
reported here naming both parts.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
}
```

## Suggested fix

Use a part kind with a built-in model and give it a value, add
`forward_voltage_v` and `forward_current_ma` to the part's
`[operating_conditions]` (and name its pins `anode` and `cathode`), add a
registry `[spice]` model, or mark the part `ignore` in the testbench when the
gap is deliberate.
