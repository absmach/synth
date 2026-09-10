# E-SYNTH-SPI-002 — SPI MOSI/MISO pin direction mismatch

**Severity:** error
**Stage:** erc — protocol

## What this means

An SPI MOSI or MISO net has an endpoint direction collision (for example, connecting two MOSI output drivers directly to each other instead of MOSI to MOSI/Input or MISO to MISO).

## Minimal reproduction

```synth
board "spi_dir_test" {
  layers 2
  component U1: mcu "rp2350"
  component U2: ic "w5500_ethernet"

  connect U1.gp0 -> U2.miso
}
```

## Suggested fix

Connect MOSI output from controller to MOSI input of peripheral, and MISO output from peripheral to MISO input of controller.
