# E-SYNTH-BOOT-001 — boot_mode pin floating

**Severity:** warning
**Stage:** erc — boot

## What this means

A `boot_mode` capability pin (e.g., ESP32 IO0) is unconnected. The MCU's boot ROM samples this on reset; leaving it floating makes startup behaviour non-deterministic.

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "esp32_wroom_32"
  // no strap on io0
}
```

## Suggested fix

Pull the pin to its inactive state with a resistor, and expose a button or jumper if entry to the bootloader is needed.
