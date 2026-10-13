# Dual-USB compute board

Two RP2350s on a six-layer stackup, each on its own native USB, each pair at
90 Ω with its own skew budget, a shared QSPI boot flash, and a 20-pin
expansion header.

## Board

| Parameter    | Value                      |
|--------------|----------------------------|
| Layers       | 6                          |
| Manufacturer | `jlcpcb`                   |
| Revision     | A                          |
| Components   | 28                         |
| Nets         | 38                         |
| Outline      | 83.4 × 89.9 mm, auto-sized |

## Stackup

| #  | Layer     | Material | Thickness | εr  |
|----|-----------|----------|-----------|-----|
| 1  | copper    | —        | 0.035 mm  | —   |
| 2  | insulator | FR4      | 0.1 mm    | 3.9 |
| 3  | copper    | —        | 0.0152 mm | —   |
| 4  | insulator | FR4      | 0.2 mm    | 4.4 |
| 5  | copper    | —        | 0.0152 mm | —   |
| 6  | insulator | FR4      | 0.5 mm    | 4.6 |
| 7  | copper    | —        | 0.0152 mm | —   |
| 8  | insulator | FR4      | 0.2 mm    | 4.4 |
| 9  | copper    | —        | 0.0152 mm | —   |
| 10 | insulator | FR4      | 0.1 mm    | 3.9 |
| 11 | copper    | —        | 0.035 mm  | —   |

## Netclasses

| Class | Track  | Clearance | Other                | Nets |
|-------|--------|-----------|----------------------|------|
| USB   | 0.2 mm | 0.2 mm    | `match_length 0.5mm` | 4    |
| QSPI  | 0.2 mm | 0.2 mm    | —                    | 6    |
| PWR   | 0.8 mm | 0.25 mm   | —                    | 1    |

## Differential pairs

| Pair                      | Impedance | Max skew | Couple |
|---------------------------|-----------|----------|--------|
| `U2_usb_dp` / `U2_usb_dn` | 90 Ω      | 0.15 mm  | tight  |
| `U4_usb_dp` / `U4_usb_dn` | 90 Ω      | 0.15 mm  | tight  |

Skew is the budget between a pair's own two halves; the 0.5 mm
`match_length` on the USB class is the budget across all four nets. At FR4
velocity 0.15 mm is of the order of a picosecond.

## Notable parts

| Ref | Part             | Registry id        |
|-----|------------------|--------------------|
| U1  | AMS1117-3.3      | `ams1117_3v3`      |
| U2  | RP2350A (host)   | `rp2350`           |
| U3  | W25Q128JV        | `w25q128_flash`    |
| U4  | RP2350A (device) | `rp2350`           |
| J1  | USB-C receptacle | `usb_c_receptacle` |
| J2  | Micro-USB B      | `micro_usb_b`      |
| J3  | 1×20 header      | `header_1x20`      |

Actives and connectors. 21 passives are omitted; `bom.csv` is the full bill
of materials.

## Reproducing

```
synth validate   qualification/class-c/dual-usb-compute/dual_usb_compute.synth
synth export-kicad qualification/class-c/dual-usb-compute/dual_usb_compute.synth --out <dir>
```

Clean validation and a complete project: schematic, board, symbol library, BOM
and pick-and-place.
