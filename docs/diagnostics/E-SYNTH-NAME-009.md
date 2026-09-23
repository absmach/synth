# E-SYNTH-NAME-009 — ground pin on a non-ground net

**Severity:** error
**Stage:** erc — naming

## What this means

A pin the part declares as ground (electrical type `ground_reference`, or
a name like `gnd`/`vss`) sits on a net that is positively something else —
an inferred rail, a net carrying a supply output, or a declared
non-ground name. This is the classic mis-wire: a ground pin tied to a
supply rail. Auto-named nets that merely lack a ground name are not
flagged, so ordinary `gnd`-to-bypass-cap nets stay quiet.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  component U2: sensor "bmp280_pressure"
  connect U1.vout -> U2.gnd    // ground pin on the 3V3 rail
}
```

## Suggested fix

Move the pin to the real ground net. If the net genuinely is a ground
that was renamed away from `GND`/`VSS`, rename it back.
