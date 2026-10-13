# Dual-USB compute board

Two RP2350s on a six-layer stackup, each on its own native USB, each pair at
90 Ω with its own skew budget, a shared QSPI boot flash, and a 20-pin
expansion header. Written for #40 case 3, "PCIe, USB 3.x, or HDMI-class
differential interface": two independent pairs on one board, each staying its
own pair with its own budgets.

**Status: not fabricated, not powered, not routed here.** Validates clean and
exports a complete KiCad project. Routed copper needs the independent routing
validator, which is not part of this build.

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

A `netclass` block assigns no nets. Each of these is joined by `class "NAME"`
on a `connect`, which is the spelling the language reference omitted until
#129. Without it a class is inert and reads as satisfied.

## Differential pairs

| Pair                      | Impedance | Max skew | Couple |
|---------------------------|-----------|----------|--------|
| `U2_usb_dp` / `U2_usb_dn` | 90 Ω      | 0.15 mm  | tight  |
| `U4_usb_dp` / `U4_usb_dn` | 90 Ω      | 0.15 mm  | tight  |

`max_skew` bounds one pair's own two halves. `match_length` on the USB class
bounds all four nets against each other. They answer different questions and
this board states both: USB 2.0 high speed tolerates a few tens of
picoseconds between halves, which is a few tenths of a millimetre of FR4.

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

The actives and connectors; 21 passives are omitted. The generated `bom.csv`
is the bill of materials.

## What #40 case 3 asks, and what this board answers

| Case 3 asks                       | Answer                                                |
|-----------------------------------|-------------------------------------------------------|
| a differential interface declared | **Yes.** Two pairs, 90 Ω, `max_skew`, `couple tight`. |
| an inter-pair length budget       | **Yes.** `match_length 0.5mm` over four nets.         |
| constraints preserved in KiCad    | **No.** Neither budget survives export (#147).        |
| routed evidence                   | **Not here.** No router in this build.                |

## Limitations

| Limitation                                                                                                                                                                                                       | Issue |
|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------|
| `match_length` and `max_skew` are dropped at the export boundary, and no `.kicad_dru` is written, so KiCad enforces neither                                                                                      | #147  |
| Nothing in this build measures either budget; the checks live in the routing validator                                                                                                                           | —     |
| The second endpoint is an RP2350, not the ESP32-C3 this design was written around: that part names a symbol and footprint absent from KiCad 10 and declares USB on two pins the module does not have             | #146  |
| No keepouts. A declared keepout reaches no exported board, and even when it does a KiCad keepout is unconditional and would forbid the pair it protects; isolating a pair needs a rule area with a net condition | #142  |
| The outline is roughly twice the area the parts occupy — components span 84% × 53% of it                                                                                                                         | —     |

What reaches the exported project file is `clearance`, `trace_width`,
`diff_pair_width` and `diff_pair_gap`, which is what makes #147 easy to miss:
the net class looks like it carries the design's constraints.

## Reproducing

```
synth validate   qualification/class-c/dual-usb-compute/dual_usb_compute.synth
synth export-kicad qualification/class-c/dual-usb-compute/dual_usb_compute.synth --out <dir>
```

Clean validation and a complete project: schematic, board, symbol library, BOM
and pick-and-place.
