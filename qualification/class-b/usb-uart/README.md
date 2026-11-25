# USB-C dual UART and RS-485 adapter (Class B, SYN-66)

A USB-C adapter built on a WCH CH342F dual UART bridge, on two copper layers. UART1 comes out as TTL on pin headers. UART0 drives a TI THVD1450D RS-485 transceiver. The board is 41.0 x 32.2 mm with four M2.5 mounting holes.

It is a bench or lab adapter for short cables, point-to-point links or the last node on an RS-485 bus. It is not for industrial installs, long cable runs or a node in the middle of a multidrop bus. The board has not been fabricated, powered or tested, and nothing here claims signal integrity, impedance control or hardware function.

The design is summarised in [`PROMPT.md`](PROMPT.md), written after the board was finished. No agent prompt was kept; the nearest record is the cleaned brief in `absmach/synth-website` at `public/showcase/usb-uart/prompt.md` (`be423c81`).

## The board

### Parts

| Group                 | Parts                                                                                                                                                                        |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| USB-C input           | `J1` Korean Hroparts TYPE-C-31-M-12 on the top edge; `R1` and `R2`, 5.1 k Rd on CC1 and CC2 so a host sources 5 V; D+ and D- go straight to `U1` with no series resistor     |
| ESD                   | `D1` Nexperia PRTR5V0U2X on D+ and D- (VCC pin on VBUS); `D2`, `D5`, `D6` Nexperia PTVS6V0S1UR on VBUS, CC1 and CC2                                                          |
| 3.3 V                 | `U2` TI LP5907MFX-3.3 (250 mA, SOT-23-5), EN tied to VIN; `C2` and `C3`, 2.2 uF in and out                                                                                   |
| Bridge                | `U1` WCH CH342F, QFN-24 4 x 4 mm, exposed pad to GND; `C4` on VDD5, `C5` on V3, `C6` on VIO (100 nF each); `R8` and `R9` hold the unused inputs                              |
| VIO select            | `J_VIO` 1x3 jumper header: pin 1 +3V3, pin 2 VIO, pin 3 VBUS                                                                                                                 |
| UART1 TTL             | `J2` (GND, TXD1, RXD1) and `J3` (DTR1, RTS1, CTS1); `R5` and `R6`, 1 k in series on RXD1 and CTS1                                                                            |
| RS-485                | `U3` TI THVD1450D, VCC from VIO with `C7`; `J4` (A, B, GND); `R_TERM` 120 ohm 1% across A and B; `R_TNOW` 4.7 k 1% to GND on TNOW0                                           |
| LEDs                  | `D3` green (VIO present) through `R3` 470 ohm; `D4` and `D7` red TX LEDs from VIO, sunk into TXD0 and TXD1, through `R4` and `R7`                                            |
| No ACT# LED           | The QFN-24 CH342F has no ACT# pin, so the board has no activity LED                                                                                                          |
| Mechanical            | `H1` to `H4` M2.5 non-plated holes at x 5.5 and 39.6 mm, y 3.0 and 21.0 mm; the bottom corners hold the header row                                                           |
| VIO rail              | VIO is the I/O rail of both UARTs, the CH342F reset pin and the RS-485 chip                                                                                                  |
| VIO jumper pins 1-2   | VIO is 3.3 V from the LP5907. This is the default                                                                                                                            |
| VIO jumper pins 2-3   | VIO is 5 V from VBUS                                                                                                                                                         |
| VIO jumper not fitted | VIO is off. The CH342F stays in reset, does not enumerate, and the VIO LED is off                                                                                            |
| VIO isolation         | VIO is not on `J2` or `J3`, so a target cannot back-feed it. Do not wire anything to `J_VIO` pin 2                                                                           |
| Unused bridge inputs  | `R8` (10 k) pulls RI0, RI1, DCD0, DCD1, DSR0 and DSR1 to VIO (inactive). `R9` (10 k) holds CTS0 low, so UART0 flow control cannot stall. RST is left open (internal pull-up) |
| RS-485 direction      | DTR0/TNOW0 drives DE and RE of `U3`. `R_TNOW` puts UART0 in the CH342F half-duplex mode                                                                                      |
| Source                | [`usb_uart.synth`](usb_uart.synth)                                                                                                                                           |
| Placement             | Every part has a position in [`usb_uart.placement.layout.toml`](usb_uart.placement.layout.toml) (board mm)                                                                   |
| Schematic             | No sidecar: the compiler lays it out, one region per group                                                                                                                   |
| 3D model of `J1`      | In [`../models/`](../models/README.md)                                                                                                                                       |

### Layout

| Item                    | Value                                                                                 |
| ----------------------- | ------------------------------------------------------------------------------------- |
| Outline                 | 41.0 x 32.2 mm                                                                        |
| Copper layers           | 2 (F.Cu and B.Cu)                                                                     |
| Stackup                 | 1.6 mm board                                                                          |
| Parts in the BOM        | 36                                                                                    |
| Parts placed in the CPL | 28 (the four mounting holes and four through-hole headers are not in the CPL)         |
| Nets                    | 26 named nets, counted in [`kicad/usb_uart.kicad_pcb`](kicad/usb_uart.kicad_pcb)      |
| Track segments          | 273                                                                                   |
| Vias                    | 43                                                                                    |
| Track length            | 515.4 mm                                                                              |
| Zones and planes        | GND pour on F.Cu and on B.Cu; one keepout per mounting hole (`H1` to `H4` screw head) |
| Via-in-pad              | Only on the `U1` exposed pad (pin 25). The fab must fill and cap it                   |
| Fab package             | `fab/gerbers.zip`, `fab/bom.csv`, `fab/cpl.csv`; STEP in `models/usb_uart.step`       |

### Checks

| Check                                   | Result                               | Notes                                             |
| --------------------------------------- | ------------------------------------ | ------------------------------------------------- |
| KiCad DRC (10.0.8, --refill-zones)      | 0 errors, 0 warnings, 0 unconnected  | Measured on the committed board file              |
| KiCad ERC (10.0.8)                      | 0 errors, 11 warnings, 0 unconnected | 10 lib_symbol_issues, 1 label_multiple_wires      |
| Release, routing and JLCPCB DFM records | Not in the tree                      | No release-gate, routing or DFM result is claimed |

### Design figures

| Area             | Item                                                 | Value                                                                                                                                         | Basis                                                          |
| ---------------- | ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Power            | LDO capacitors `C2`, `C3`                            | 2.2 uF each; minimum 1 uF                                                                                                                     | TI LP5907 datasheet (revision not stated in the source)        |
| Power            | RS-485 supply                                        | THVD1450 needs 3 V or more, so VIO is always 3.3 V or 5 V                                                                                     | TI THVD1450 datasheet (revision not stated in the source)      |
| Protection       | Target driving RXD1 or CTS1 with VIO off             | Exceeds the CH342F input limit of VIO + 0.5 V                                                                                                 | WCH CH342F datasheet V1E                                       |
| Protection       | Current through `R5` and `R6` with VIO off           | About 2.6 mA (3.3 V target) or 4.3 mA (5 V target)                                                                                            | Calculated                                                     |
| Protection       | CH342F input clamp rating                            | Not given, so fit the VIO jumper before connecting a target                                                                                   | WCH CH342F datasheet V1E                                       |
| Signal integrity | UART1 half-duplex at power-up                        | CTS1 floats when `J3` is unplugged. A target that holds DTR1 low at power-up switches UART1 to half-duplex                                    | WCH CH342F datasheet V1E, section 5.3                          |
| Signal integrity | USB pair impedance                                   | No impedance target. 90 ohm differential on two 1.6 mm layers needs about 1.18 mm traces, so only the `USB` netclass width (0.242 mm) applies | Calculated; width from `kicad/usb_uart.kicad_pro`              |
| Power            | TX LED current with `R4`, `R7` at 1 k (source value) | About 1.4 mA at 3.3 V and 3.1 mA at 5 V per LED, for a 1.9 V forward drop                                                                     | Calculated (estimate); Lite-On LED forward drop is second-hand |

### Sourcing

| Item                             | Status         | Notes                                                                                                       |
| -------------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------- |
| BOM columns                      | Partial        | `fab/bom.csv` has value, footprint, `lcsc_pn` and `mpn` columns only, with no manufacturer or order columns |
| LCSC numbers                     | 4 parts        | Only `J1`, `U1`, `U2` and `U3` have an LCSC number                                                          |
| Passives and headers             | No part number | Generic 0603 parts and 2.54 mm headers                                                                      |
| Availability and JLCPCB assembly | Not checked    |                                                                                                             |

## Not verified by Synth

| Area                      | What is not verified                                                                                                                                                   |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source and package differ | The source sets `R4` and `R7` to 1 k. The committed KiCad files, `fab/bom.csv` and `fab/cpl.csv` still show 470 ohm. The package was not re-exported after that change |
| TX LED drive              | The CH342F output drive at 5 V was not checked                                                                                                                         |
| Return paths              | Not checked. The USB pair is not impedance-controlled                                                                                                                  |
| VBUS protection           | No VBUS bulk capacitor, fuse, reverse or inrush protection                                                                                                             |
| RS-485 protection         | A and B have no surge protection beyond the transceiver's built-in ESD. No TVS or common-mode choke on A and B                                                         |
| RS-485 bias               | No bias resistors. The THVD1450 has built-in failsafe                                                                                                                  |
| Termination               | `R_TERM` is always connected. The registry has no 2-pin header for a jumper                                                                                            |
| 3D model                  | The STEP uses the GrabCAD `J1` model, but the committed `kicad_pcb` still names KiCad's stock path. The mounting holes have no model                                   |
| Exposed pad               | The KiCad footprint pad is 2.6 mm, the supplier drawing 2.4 mm. Not reconciled                                                                                         |
| Datasheets                | The Lite-On LED numbers are second-hand; the Lite-On site was not reachable. A newer CH342F datasheet than V1E may exist                                               |
| Mechanical                | Enclosure cutout fit for `J1` on the top edge, screw size and standoff height are not chosen or checked                                                                |
| Silkscreen                | Synth has no silkscreen text, so header pin labels exist only in the schematic                                                                                         |
| Parts                     | No part has been reviewed                                                                                                                                              |
| Hardware                  | The board has not been fabricated, powered or tested                                                                                                                   |
| Independent sign-off      | The independent PCB review required by #38 is still open.                                                                                                              |
