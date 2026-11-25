# USB-C dual UART and RS-485 adapter (Class B, SYN-66)

A USB-C adapter built on a WCH CH342F dual UART bridge, on two copper layers. UART1 comes out as TTL on pin headers. UART0 drives a TI THVD1450D RS-485 transceiver. The board is 40.99 x 32.23 mm with four M2.5 mounting holes.

It is a bench or lab adapter for short cables, point-to-point links or the last node on an RS-485 bus. It is not for industrial installs, long cable runs or a node in the middle of a multidrop bus. A and B have no surge protection beyond the transceiver's built-in ESD, and the 120 ohm termination is always connected.

The board has not been fabricated, powered or tested. No part has been reviewed. Nothing here claims signal integrity, impedance control or hardware function.

nyagamunene is the author. The board has no independent review.

## The board

| Group | Parts |
|---|---|
| USB-C input | `J1` Korean Hroparts TYPE-C-31-M-12 on the top edge; `R1` and `R2`, 5.1 k Rd on CC1 and CC2 so a host sources 5 V; D+ and D- go straight to `U1` with no series resistor |
| ESD | `D1` Nexperia PRTR5V0U2X on D+ and D- (VCC pin on VBUS); `D2`, `D5`, `D6` Nexperia PTVS6V0S1UR on VBUS, CC1 and CC2 |
| 3.3 V | `U2` TI LP5907MFX-3.3 (250 mA, SOT-23-5), EN tied to VIN; `C2` and `C3`, 2.2 uF in and out (datasheet minimum 1 uF) |
| Bridge | `U1` WCH CH342F, QFN-24 4 x 4 mm, exposed pad to GND; `C4` on VDD5, `C5` on V3, `C6` on VIO (100 nF each); `R8` and `R9` hold the unused inputs |
| VIO select | `J_VIO` 1x3 jumper header: pin 1 +3V3, pin 2 VIO, pin 3 VBUS |
| UART1 TTL | `J2` (GND, TXD1, RXD1) and `J3` (DTR1, RTS1, CTS1); `R5` and `R6`, 1 k in series on RXD1 and CTS1 |
| RS-485 | `U3` TI THVD1450D, VCC from VIO with `C7`; `J4` (A, B, GND); `R_TERM` 120 ohm 1% across A and B; `R_TNOW` 4.7 k 1% to GND on TNOW0 |
| LEDs | `D3` green (VIO present) through `R3` 470 ohm; `D4` and `D7` red TX LEDs from VIO, sunk into TXD0 and TXD1, through `R4` and `R7`; no ACT# LED, since the QFN-24 has no ACT# pin |
| Mechanical | `H1` to `H4` M2.5 holes at x 5.5 and 39.6 mm, y 3.0 and 21.0 mm; the bottom corners hold the header row |

VIO is the I/O rail of both UARTs, the reset pin and the RS-485 chip. The THVD1450 needs 3 V or more, so VIO is always 3.3 V or 5 V.

| Jumper | VIO | Notes |
|---|---|---|
| pins 1-2 | 3.3 V (LP5907) | default |
| pins 2-3 | 5 V (VBUS) | |
| none | off | CH342F stays in reset, no enumeration, VIO LED off |

- VIO is not on `J2` or `J3`, so a target cannot back-feed it. Do not wire anything to `J_VIO` pin 2.
- With VIO off, a powered target driving RXD1 or CTS1 exceeds the CH342F limit of VIO + 0.5 V. `R5` and `R6` limit the current to about 2.6 mA (3.3 V) or 4.3 mA (5 V). The datasheet gives no clamp rating, so fit the jumper first.
- `R8` (10 k) pulls RI0, RI1, DCD0, DCD1, DSR0 and DSR1 to VIO (inactive). `R9` (10 k) holds CTS0 low, so UART0 flow control cannot stall. RST is left open (internal pull-up).
- CTS1 floats when `J3` is unplugged. A target that holds DTR1 low at power-up switches UART1 to half-duplex (CH342F datasheet 5.3).
- DTR0/TNOW0 drives DE and RE of `U3`. `R_TNOW` puts UART0 in the CH342F half-duplex mode.
- The USB pair has no impedance target. On two 1.6 mm layers, 90 ohm differential needs about 1.18 mm traces, so only the `USB` netclass width (0.242 mm) applies.

The source is [`usb_uart.synth`](usb_uart.synth). Every part has a position in [`usb_uart.placement.layout.toml`](usb_uart.placement.layout.toml) (board mm). The schematic has no sidecar: the compiler lays it out, one region per group. The 3D model of `J1` is in [`../models/`](../models/README.md).

The committed PCB in [`kicad/`](kicad/usb_uart.kicad_pcb) has F.Cu and B.Cu with a GND pour on both, 273 track segments, 43 vias and 515.4 mm of track. Its only via-in-pad is on the `U1` exposed pad (pin 25); the fab must fill and cap it.

## Not verified by Synth

- **Release evidence:** the DRC, ERC, release, routing and JLCPCB DFM records for this board are no longer in the tree. No DRC, ERC, release-gate or DFM result is claimed here.
- **Source and package differ:** the source sets `R4` and `R7` to 1 k. The committed KiCad files, `fab/bom.csv` and `fab/cpl.csv` still show 470 ohm, and the package was not re-exported after that change.
- **TX LED current:** with 1 k and a 1.9 V forward drop, about 1.4 mA at 3.3 V and 3.1 mA at 5 V per LED (estimate). The CH342F output drive at 5 V was not checked.
- **Orderable parts:** `fab/bom.csv` has value, footprint, `lcsc_pn` and `mpn` columns only, with no manufacturer or order columns. Passives and headers have no part number, and only `J1`, `U1`, `U2` and `U3` have an LCSC number. Availability and JLCPCB assembly were not checked.
- **Return paths:** not checked in this folder; the earlier signal-integrity record is no longer in the tree. The USB pair is not impedance-controlled.
- **Protection:** no VBUS bulk capacitor, fuse, reverse or inrush protection. No surge part, TVS or common-mode choke on A and B. No bias resistors (the THVD1450 has built-in failsafe).
- **Termination:** `R_TERM` is always connected. The registry has no 2-pin header for a jumper.
- **3D model:** the STEP uses the GrabCAD `J1` model, but the committed `kicad_pcb` still names KiCad's stock path. The mounting holes have no model.
- **Exposed pad:** the KiCad footprint pad is 2.6 mm, the supplier drawing 2.4 mm. Not reconciled.
- **Datasheets:** the Lite-On LED numbers are second-hand; the Lite-On site was not reachable. A newer CH342F datasheet than V1E may exist.
- **Mechanical:** `J1` sits on the top edge. Enclosure cutout fit, screw size and standoff height are not chosen or checked.
- **Silkscreen:** Synth has no silkscreen text, so header pin labels exist only in the schematic.
- **Review:** no part or layout review, and nothing was powered or tested.
