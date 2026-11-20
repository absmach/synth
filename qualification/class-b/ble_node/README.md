# BLE Environment Node Board

4-layer BLE environment node for Class B qualification.

## Features
- nRF52840 MCU (aQFN-73, BLE 5/Thread/Zigbee)
- USB-C power input with 5.1k CC pull-downs and ESD protection
- LP5907-3.3 low-noise LDO regulator
- 32 MHz crystal (12 pF load caps)
- 2.4 GHz BLE RF: balun + matching network (3.9 nH / 1.0 pF) + chip antenna
- BME280 environmental sensor (temp, humidity, pressure) on I2C
- SWD 5-pin debug header
- 50 Ω antenna feed: 0.145 mm trace width (computed from 4-layer stackup)

## Stackup
4-layer: Cu 0.035 / FR4 0.1 (er 4.3) / Cu 0.035 / FR4 1.0 (er 4.4) / Cu 0.035 / FR4 0.1 (er 4.3) / Cu 0.035 mm

## Files
- `ble_node.synth` — SynthSpec source
- `kicad/` — KiCad project files (PCB, schematic, BOM, PnP)
- `images/` — schematic, PCB, and 3D model screenshots
- `reports/` — DRC and ERC reports
- `qualification.json` — qualification gate status
- `SHA256SUMS` — file checksums
