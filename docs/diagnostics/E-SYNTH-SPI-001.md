# E-SYNTH-SPI-001 — SPI endpoint connected to non-SPI pin

**Severity:** error
**Stage:** erc — protocol

## What this means

A dedicated SPI peripheral pin (a pin whose only protocol capability is one of `spi_mosi`, `spi_miso`, `spi_sck`, `spi_cs`) was connected to a pin that lacks any SPI capability.

## Minimal reproduction

```synth
board "x" {
  component U1: memory "w25q128_flash"
  component U2: ic "lm555_timer"
  connect U1.mosi -> U2.trig  // 555 input is not SPI
}
```

## Suggested fix

Route the SPI signal to the matching pin on a SPI master/slave (usually an MCU's SPI peripheral).
