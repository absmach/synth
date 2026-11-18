# E-SYNTH-USB-002 — USB-C CC pin missing pull-down resistor

**Severity:** error when a CC pin is floating; warning when it is wired to a net with no resistor
**Stage:** erc — protocol

## What this means

A USB-C Configuration Channel (CC1 or CC2) pin is present on a connector but no pull-down resistor or active CC controller is connected to its net. USB-C Sink ports require 5.1 kΩ pull-down resistors on CC1 and CC2 for Upstream Facing Port (UFP) detection.

Two cases are reported:

- **Floating** (error): the CC pin is on no net at all. Without Rd a compliant source never detects the sink and never turns VBUS on, so the board cannot be powered from it.
- **Wired, no resistor** (warning): the CC pin's net carries no resistor. This stays a warning because an active CC controller on the net is a valid design.

## Minimal reproduction

```synth
board "usb_cc_test" {
  layers 2
  component J1: connector "usb_c_receptacle"
}
```

## Suggested fix

Add 5.1kΩ pull-down resistors to GND on both CC1 and CC2 nets.
