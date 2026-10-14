# Dual-USB compute board

A USB 3.0 Type-A host port whose two SuperSpeed lanes run at 5 Gb/s, two
RP2350s on their own USB 2.0 pairs, a shared QSPI boot flash, and a 20-pin
expansion header, on a six-layer stackup.

## Board

| Parameter    | Value                      |
|--------------|----------------------------|
| Layers       | 6                          |
| Manufacturer | `jlcpcb`                   |
| Revision     | A                          |
| Components   | 28                         |
| Nets         | 38                         |
| Outline      | 64.3 × 64.3 mm, auto-sized |

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

| Class | Track   | Clearance | Group skew | Nets |
|-------|---------|-----------|------------|------|
| SS    | 0.15 mm | 0.2 mm    | 0.2 mm     | 4    |
| USB   | 0.2 mm  | 0.2 mm    | 0.5 mm     | 4    |
| QSPI  | 0.2 mm  | 0.2 mm    | —          | 6    |
| PWR   | 0.8 mm  | 0.25 mm   | —          | 1    |

## Differential pairs

| Pair                      | Signal         | Rate    | Impedance | Max skew | Couple |
|---------------------------|----------------|---------|-----------|----------|--------|
| `J2_sstx_p` / `J2_sstx_n` | SuperSpeed TX  | 5 Gb/s  | 90 Ω      | 0.1 mm   | tight  |
| `J2_ssrx_p` / `J2_ssrx_n` | SuperSpeed RX  | 5 Gb/s  | 90 Ω      | 0.1 mm   | tight  |
| `U2_usb_dp` / `U2_usb_dn` | USB 2.0 host   | 12 Mb/s | 90 Ω      | 0.15 mm  | tight  |
| `U4_usb_dp` / `U4_usb_dn` | USB 2.0 device | 12 Mb/s | 90 Ω      | 0.15 mm  | tight  |

A pair's skew is the budget between its own two halves, checked by
E-SYNTH-DRC-014; a class's group skew is the spread across its nets, longest
minus shortest, checked by E-SYNTH-DRC-019. Both are measured on routed copper.
At FR4 velocity 0.1 mm is about 0.7 ps.

## Notable parts

| Ref | Part                      | Registry id         |
|-----|---------------------------|---------------------|
| U1  | AMS1117-3.3               | `ams1117_3v3`       |
| U2  | RP2350A (host)            | `rp2350`            |
| U3  | W25Q128JV                 | `w25q128_flash`     |
| U4  | RP2350A (device)          | `rp2350`            |
| J1  | USB-C receptacle          | `usb_c_receptacle`  |
| J2  | USB 3.0 Type-A receptacle | `usb3_a_receptacle` |
| J3  | 1×20 header               | `header_1x20`       |

Actives and connectors. 21 passives are omitted; `bom.csv` is the full bill
of materials.

## Reproducing

```
synth validate   qualification/class-c/dual-usb-compute/dual_usb_compute.synth
synth export-kicad qualification/class-c/dual-usb-compute/dual_usb_compute.synth --out <dir>
```

Clean validation and a complete project: schematic, board, symbol library, BOM
and pick-and-place.
