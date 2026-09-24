# E-SYNTH-ESD-001 — external connector without protection

**Severity:** warning
**Stage:** erc — protocol

## What this means

A signal or rail reachable through an external connector has no
protection device. Two cases:

- **ESD** — a connector pin carrying an interface capability
  (`usb_dp`, `usb_dn`, `usb_cc`, `uart_tx`, `uart_rx`) with no TVS/ESD
  array on its net. A user's touch can inject kilovolts.
- **Reverse polarity** — a power connector (USB / DC jack / battery)
  feeding a rail with no series diode, ideal-diode, or load switch. A
  reversed supply destroys every part on the rail.

A plain internal header is not treated as a DC input, and ground nets
are skipped, so debug headers do not attract findings.

## Minimal reproduction

```synth
board "x" {
  component J1: connector "usb_c_receptacle"
  component R1: resistor "r_generic_0603" value "1k"
  connect J1.dp -> R1.p1           // no ESD device on the D+ net
  connect R1.p2 -> J1.gnd
}
```

## Suggested fix

Add an ESD/TVS array between the connector pin and ground, and a series
diode or load switch between a power connector and its rail. Disable the
check with `require_connector_protection = false` for a board with no
external interface.
