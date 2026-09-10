# E-SYNTH-DIFF-001 — differential pair without impedance

**Severity:** warning
**Stage:** erc — protocol

## What this means

A `diff_pair` was declared without an `impedance` attribute. Without a target impedance, the router cannot size the trace geometry and signal integrity is uncontrolled.

## Minimal reproduction

```synth
board "x" {
  component J1: connector "usb_c_receptacle"
  diff_pair dp dn {}
}
```

## Suggested fix

Add an `impedance` line inside the diff_pair body, typically `90ohm` for USB 2.0 or `100ohm` for Ethernet.
