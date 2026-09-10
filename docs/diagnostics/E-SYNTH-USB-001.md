# E-SYNTH-USB-001 — USB differential capability mismatch

**Severity:** error
**Stage:** erc — protocol

## What this means

A `connect` statement attaches a pin carrying `usb_dp` (or `usb_dn`)
capability to a pin that does not carry the matching capability on
the other side. USB differential lines must connect to pins that
the part explicitly advertises as USB-capable; routing USB through a
plain GPIO will not work even if it might compile in legacy EDA
tools.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  connect U1.usb_dp -> U2.gp0
}
```

## Suggested fix

Connect the USB pin to a counterpart that advertises the `usb_dp` (or
`usb_dn`) capability. The rule does not emit automatic patches
because the correct mate is a topology decision.
