# E-SYNTH-SCHEM-014 — auto-named net rendered on the sheet

**Severity:** warning
**Stage:** schematic aesthetic ERC

## What this means

A net that was never given a name (`net_3`, `NET_3`) reaches the sheet as a rendered label. A placeholder name carries no intent — the reference sheet names every net it draws (`SCL`, `SDA`, `VIN`).

Nets drawn only as wires are not rendered by name and never fire; power rails get derived labels (`GND`, `VCC`), not placeholders.

## Minimal reproduction

An unnamed `connect` whose net is truncated to a label (multi-drop or long span) renders `NET_<n>`.

## Suggested fix

Name the net at its source:

```synth
net "SDA" { U1.pb7, U2.sda }
// or
connect U1.pb7 -> U2.sda as "SDA"
```

The diagnostic's message suggests a name taken from the net's first endpoint pin.
