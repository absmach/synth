# Standalone multi-rail compute board

An ESP32-WROOM-32 on six supply rails from one 12 V input, with its own USB
programming path, QSPI flash, I²C EEPROM, 10/100 Ethernet, a 16-channel PWM
driver, a 16-bit GPIO expander, a level-shifted header and on-board rail and
temperature monitoring. Six layers, two of them ground.

Nothing external is required to make it run: it is powered through its own
connector, flashed through its own USB-C port, and boots from its own flash.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Floorplan envelope | 140 × 100 mm |
| Layers | 6 |
| Process | `jlc-advanced` — 0.127 mm track/clearance, **0.2 mm drill**, 0.13 mm ring, filled vias |
| Components | 127 |
| Nets | 114, over 454 pads |
| Source | `multirail_compute.synth` |
| Floorplan | `multirail_compute.placement.layout.toml` |

The process is the advanced tier rather than the standard one because of the
module, not the routing: every KiCad ESP32-WROOM-32 footprint drills twelve
0.2 mm thermal vias into its ground paddle, below the 0.3 mm standard floor,
and they sit inside pad 39 so they need the fill as well as the drill. The
board declares `requires via_in_pad` for the same reason, and because a plane
pad on a board this dense can be boxed in on every side.

## Stackup

| Layer | Thickness | εr | Carries |
| --- | --- | --- | --- |
| F.Cu | 0.035 mm | — | signal, ground pour |
| prepreg | 0.2 mm | 4.4 | — |
| In1.Cu | 0.0152 mm | — | ground |
| prepreg | 0.2 mm | 4.4 | — |
| In2.Cu | 0.0152 mm | — | 5 V plane |
| core | 0.66 mm | 4.6 | — |
| In3.Cu | 0.0152 mm | — | 3V3 plane |
| prepreg | 0.2 mm | 4.4 | — |
| In4.Cu | 0.0152 mm | — | ground |
| prepreg | 0.2 mm | 4.4 | — |
| B.Cu | 0.035 mm | — | signal, ground pour |

Total 1.59 mm.

The outer dielectric is 0.2 mm rather than the 0.1 mm a four-layer board
would use, and that is an impedance decision. A 100 Ω differential pair over a
plane 0.1 mm away needs a track at the 0.127 mm process floor with no
tolerance left; at 0.2 mm the same pair is a 0.25 mm track.

## Supply

| Rail | Source | Nominal | Carried as | Feeds |
| --- | --- | --- | --- | --- |
| V12 | J1, through D1, F1 and the D2 clamp | 12 V | traces | both switchers |
| V5 | U1 MP2307, L1 10 µH, 31.6k/10k | 5 V | **In2.Cu plane** | U2, U4, U15 |
| V3V3 | U2 MP2307, L2 10 µH, 25.5k/10k | 3.3 V | **In3.Cu plane** | module, storage, expanders, bridge |
| V3V3_E | U15 AMS1117-3.3 | 3.3 V | plane class | the Ethernet island |
| V3V3_A | U3 LP5907 behind FB1 | 3.3 V | trace | the sensor, the analog header |
| V5_SW | U4 TPS22918, GPIO-enabled | 5 V | trace | the peripheral header's 5 V side |

Reverse polarity is caught by D1 before the fuse. The clamp sits behind the
fuse, so a sustained overvoltage opens F1 rather than cooking D2: 15 V
standoff clears a 12 V nominal, and the 24.4 V clamping voltage is under the
MP2307's 28 V input rating.

Both heavy rails are switchers. A linear dropping 5 V to 3.3 V at this rail's
320 mA dissipates 0.54 W, and at the 620 mA it carried before the Ethernet
island was split off, 1.05 W — 25 + 1.05 × 60 = 88 °C in a package with no
exposed pad.

## Thermal

Calculated at 25 °C ambient from each part's declared θJA and its dissipation
on this board, not from its absolute maximum.

| Part | Dissipation here | θJA | Tj |
| --- | --- | --- | --- |
| U1 MP2307, 12 V → 5 V | ~0.25 W | 50 °C/W | 38 °C |
| U2 MP2307, 5 V → 3.3 V | ~0.23 W | 50 °C/W | 37 °C |
| U15 AMS1117, 5 V → 3.3 V at 200 mA | 0.34 W | 60 °C/W | 45 °C |
| U3 LP5907 | 0.02 W | 250 °C/W | 30 °C |
| U4 TPS22918 | 0.05 W | 200 °C/W | 35 °C |
| U5 ESP32-WROOM-32 | 0.50 W | 40 °C/W | 45 °C |

U15 is the hottest linear and the only one left; it is the Ethernet island's
supply, chosen linear because a switcher beside a 25 MHz PHY buys ripple the
PHY then has to reject.

Two measurements back the estimates at runtime rather than leaving them as
arithmetic: NTC1 is the lower half of a 10k divider on the 5 V rail beside U1,
tapped to an input-only ADC pin, and U13 reports board temperature over I²C.

## Rail monitoring

| Rail | Divider | At the pin | Read on |
| --- | --- | --- | --- |
| V12 | 100k / 10k | 1.09 V | `sensor_vp`, ADC1_CH0 |
| V5 | 22k / 10k | 1.56 V | `io35`, ADC1_CH7 |
| 5 V thermistor | 10k / 10k NTC | 2.50 V at 25 °C | `sensor_vn`, ADC1_CH3 |

All three land on input-only pins, which have no output driver and no
internal pull — which is why the dividers are on the board and not on a GPIO.

## Interfaces

| Interface | Silicon | Connector | Notes |
| --- | --- | --- | --- |
| Programming and console | U6 CH340G, Y1 12 MHz | J2 USB-C | DTR/RTS through Q1/Q2 drive download mode |
| 10/100 Ethernet | U9 W5500, Y2 25 MHz | J3 SI-60062-F | 100 Ω MDI pairs, magnetics in the jack |
| QSPI flash | U7 W25Q128 | — | HSPI: io14/13/12/15 |
| EEPROM | U8 24LC256 | — | I²C 0x50, address strapped, /WP low |
| PWM, 16 channels | U11 PCA9685 | J5 p5–p20 | I²C 0x40, output-enabled from io26 |
| GPIO, 16 bits | U10 MCP23017 | J4 p9–p20 | I²C 0x20, reset and interrupt on module pins |
| 5 V logic side | U12 TXS0108E | J4 p3–p4 | B side on V5_SW |
| Status display | U14 SSD1306 | — | I²C 0x3C |

The module's pads 17–22 are its own SPI flash bus, bonded to the die inside
the shield, and pad 32 is marked NC. All seven carry `do_not_connect` and this
board connects none of them; the external flash is on HSPI instead. All four
ground pads — 1, 15, 38 and the paddle at 39 — are connected.

## Keepout

`antenna_clearance`, 12 mm radius, over the module's antenna end. Copper under
a PCB antenna detunes it; a keepout placed anywhere else would constrain
nothing and still read as satisfied.

## Assembly and DFM

Four M2.5 mounting holes in the corners, three 1 mm fiducials, and one test
point per rail. Both connectors, the RJ45 and both headers sit on board edges
and face outward; the headers run along the top and bottom edges clear of the
mounting holes.

The floorplan is authored rather than automatic, and reads left to right as
the board works: 12 V entry down the left edge, both switching stages beside
it with each inductor next to its own regulator, the module in the middle,
storage above it, the USB programming path on the bottom edge, Ethernet to the
right with the jack on the edge, and the expansion silicon under the headers.

## Verification

| Check | Result |
| --- | --- |
| `synth validate` | **0 errors** |
| Schematic export | 15 hierarchical sheets, one per group |
| Ground | one net, 116 pads |
| Dangling nets | none |
| PCB placement | **does not complete** — see below |

## Limitations

- **The board does not place, so there is no exported PCB, no routing result
  and no DRC.** The placer reports no legal position for a 0603 resistor on a
  148 × 108 mm usable area while the parts total 4,644 mm² of courtyard — a
  29% fill, with 11,340 mm² free. It is not the component, the margin or the
  seed: the failure moves between R3, R32 and R43 with source order, and every
  margin and seed the export tries fails. The placer is greedy first-fit and
  never un-commits, and the checked-in floorplan cannot override it because
  placement runs before the sidecar is read. Everything above the PCB line —
  source validation, the schematic, the netlist — is measured; nothing below
  it can be.
- **Impedance is estimated, not solved.** The 100 Ω MDI geometry comes from a
  closed-form coupled-microstrip approximation, and the tool declines to
  verify impedance on inner layers at all.
- **No part carries a recorded reviewer.** Every shipped part reports
  `W-SYNTH-PART-UNVERIFIED` and most report no distributor identity; that is a
  registry-wide governance gap, not a property of this board.
- **The refdes prefixes NTC, FB and LED are flagged unconventional.** They are
  the conventional industry spellings for a thermistor, a ferrite bead and an
  LED, and the rule expects R, L and D.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U1, U2 | MPS MP2307 3 A buck | `mp2307_buck` |
| U3 | TI LP5907 low-noise LDO | `lp5907_3v3` |
| U4 | TI TPS22918 load switch | `tps22918_loadswitch` |
| U5 | Espressif ESP32-WROOM-32 | `esp32_wroom_32` |
| U6 | WCH CH340G USB-UART | `ch340g` |
| U7 | Winbond W25Q128 | `w25q128_flash` |
| U9 | WIZnet W5500 | `w5500_ethernet` |
| U10 | Microchip MCP23017 | `mcp23017_gpio` |
| U11 | NXP PCA9685 | `pca9685_pwm` |
| U12 | TI TXS0108E | `txs0108e` |
| U13 | TI TMP117 | `tmp117_temperature` |
| U15 | AMS1117-3.3 LDO | `ams1117_3v3` |
| D2 | Littelfuse SMAJ15A TVS | `smaj15a_tvs` |
| J3 | Bel Fuse SI-60062-F magjack | `rj45_magjack_si60062` |

The generated `bom.csv` is the bill of materials; this table is the actives.
