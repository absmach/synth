# E-SYNTH-USB-002 — USB-C CC pin missing pull-down resistor

**Severity:** warning
**Stage:** erc — protocol

## What this means

A USB-C Configuration Channel (CC1 or CC2) pin is present on a connector but no pull-down resistor or active CC controller is connected to its net. USB-C Sink ports require 5.1 kΩ pull-down resistors on CC1 and CC2 for Upstream Facing Port (UFP) detection.

## Minimal reproduction

```synth
board "usb_cc_test" {
  layers 2
  component J1: connector "usb_c_receptacle"
}
```

## Suggested fix

Add 5.1kΩ pull-down resistors to GND on both CC1 and CC2 nets.
