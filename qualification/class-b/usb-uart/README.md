# USB-C dual UART + RS-485 adapter (SYN-66)

Status: design and compact placement. Routing is incomplete. Not fabricated, not tested on hardware.
Do not call it proven or clean. The list of what is unchecked is at the bottom.

## What it is

- One USB-C port, one WCH CH342F (two UARTs).
- UART1 goes to pin headers as TTL (J2: GND, TXD1, RXD1; J3: DTR1, RTS1, CTS1).
- UART0 drives a TI THVD1450D RS-485 transceiver. Header J4: A, B, GND.
- DTR0/TNOW0 switches the transceiver direction. R_TNOW (4.7k to GND) puts UART0 in the CH342 half-duplex mode.
- Protection: PRTR5V0U2X on D+/D-, PTVS6V0S1UR on VBUS, CC1 and CC2. A 3.3 V LDO (LP5907) feeds the +3V3 rail.
- Source: `usb_uart.synth`. Sidecars: `usb_uart.placement.layout.toml` (board mm), `usb_uart.schematic.layout.toml` (sheet mm).

## VIO jumper (J_VIO)

VIO is the I/O rail of both UARTs, the reset pin and the RS-485 chip. It is always 3.3 V or 5 V.
The THVD1450 needs 3 V or more, so 1.8 V and target-powered VIO were dropped.

| Jumper | VIO | Notes |
| --- | --- | --- |
| pins 1-2 | 3.3 V (LP5907) | default choice |
| pins 2-3 | 5 V (VBUS) | |
| none | off | CH342F stays in reset (VIO under 0.8 to 1.15 V), no enumeration. VIO LED is off. |

- VIO is not on J2 or J3, so a target cannot back-feed it. Do not wire anything to J_VIO pin 2.
- Forgotten jumper: the board only looks dead, and the VIO LED shows it.
- Not harmless with a powered target: a target driving RXD1 or CTS1 with VIO off goes past the CH342F limit of VIO+0.5 V.
  R5 and R6 (1k) limit the current to about 2.6 mA (3.3 V target) or 4.3 mA (5 V). The datasheet gives no clamp rating. Add the jumper first.

## Unused CH342F pins

- RI0, RI1, DCD0, DCD1, DSR0, DSR1: R8 (10k) pulls them to VIO (inactive, active low).
- CTS0: R9 (10k) holds it low, so UART0 flow control cannot stall.
- RST: left open (internal pull-up). The datasheet says unused lines may be left open; they are tied anyway.
- CTS1 floats if J3 is unplugged. Datasheet gives no pull-up for it.
- Datasheet 5.3: a pull-down on DTR1 at power-on switches UART1 to half-duplex. A target that holds DTR1 low at power-up changes the mode.

## BOM

| Ref | Part | MPN | Datasheet (revision seen) |
| --- | --- | --- | --- |
| U1 | WCH USB dual UART | CH342F | https://www.wch-ic.com/downloads/CH342DS1_PDF.html, V1E (file id 295, dated 2022-01-27) |
| U3 | TI RS-485 transceiver | THVD1450D | https://www.ti.com/lit/ds/symlink/thvd1450.pdf, SLLSEY3E (May 2018, rev May 2019) |
| U2 | TI 3.3 V LDO | LP5907MFX-3.3 | https://www.ti.com/lit/ds/symlink/lp5907.pdf, SNVS798Q (rev Jul 2025) |
| D1 | Nexperia USB ESD | PRTR5V0U2X | https://assets.nexperia.com/documents/data-sheet/PRTR5V0U2X.pdf, dated 1 Apr 2023 |
| D2, D5, D6 | Nexperia TVS 6 V | PTVS6V0S1UR | https://assets.nexperia.com/documents/data-sheet/PTVSXS1UR_SER.pdf, dated 1 Dec 2025 |
| D3 | Lite-On green LED | LTST-C190KGKT | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF, revision not checked (site not reachable) |
| D4, D7 | Lite-On red LED | LTST-C190KRKT | https://optoelectronics.liteon.com/upload/download/DS-22-99-0151/LTST-C190KRKT.PDF, revision not checked (site not reachable) |
| J1 | USB-C receptacle | TYPE-C-31-M-12 | none in the registry, not checked |
| J2, J3, J4, J_VIO | 2.54 mm headers | generic 1x3 | none |
| C2, C3 | 2.2 uF X7R 0603 | generic | none |
| C4, C5, C6, C7 | 100 nF X7R 0603 | generic | none |
| R1, R2 | 5.1k | generic | none |
| R3, R4, R7 | 470 | generic | none |
| R5, R6 | 1k | generic | none |
| R8, R9 | 10k | generic | none |
| R_TNOW | 4.7k 1% | generic | none |
| R_TERM | 120 ohm 1% | generic | none |

Checked by me against the manufacturer file in this work: CH342F, THVD1450D, LP5907 (revision line only), the two Nexperia parts (revision line only).
The Nexperia and LP5907 electrical numbers were not re-read.

## Placement and routing (this commit)

Status: compact placement done. Routing is not complete. Do not call it routed.

- Outline: 45.1 x 35.8 mm (was 92.6 x 66.8 mm).
  Old cause: the sidecar only fixed the caps and protection parts. The rest was auto-placed far apart, and the placer adds 4 mm around every part.
  Now every part has a position in `usb_uart.placement.layout.toml`. J1 sits flush on the top edge. J_VIO, J2, J3 and J4 sit flush on the bottom edge.
- No courtyard overlaps. J1 courtyard to top edge: 0.01 mm (flush).
- Pad-edge gap to the target pad on the same net:

| Part | Target | Gap mm |
| --- | --- | --- |
| C4 (VDD5) | U1 pin 7 | 0.97 |
| C5 (V3) | U1 pin 6 | 1.14 |
| C6 (VIO) | U1 pin 5 | 0.99 |
| C7 (VCC) | U3 pin 8 | 0.99 |
| C2 (LDO in) | U2 VBUS pins | 0.99 |
| C3 (LDO out) | U2 pin 5 | 0.99 |
| D1 D+ / D- | J1 D+ / D- pads | 1.76 / 1.69 |
| D2 (VBUS TVS) | J1 VBUS pad | 2.34 |
| D5 (CC1 TVS) | J1 CC1 pad | 1.95 |
| D6 (CC2 TVS) | J1 CC2 pad | 1.60 |

- D2 is over 2 mm: four protection parts do not fit side by side under the 7 mm of J1 pads any closer.
- R1, R2, the LEDs and R3 to R9 are placed by hand but not checked against any rule.

Routing (FreeRouting 2.4.1, default settings, 40 passes; best of several placement tries):
- Result: not complete. FreeRouting leaves nets open; Synth's own check says 9 nets are open (CC1, CC2, CTS1, RS485_B, RTS1, RXD1, TXD1, VBUS, VIO). Synth marks the board draft, not fabricable.
- kicad-cli DRC on the routed attempt (`--severity-all --refill-zones`): 0 errors, 5 unconnected items, 49 warnings.
  The 5 unconnected items are GND pour fragments on F.Cu with no stitching via. No signal net is listed. KiCad and Synth's check disagree about the 9 nets; this is not resolved.
  The warnings are 32 footprint library links, 12 silk over copper, 5 silk overlap.
- 263 track segments, 23 vias.
- USB pair: width 0.242 mm. D+ is 17.03 mm (F.Cu only). D- is 18.43 mm (11.21 F.Cu, 7.23 B.Cu, 2 vias). Skew 1.40 mm. Closest gap 0.133 mm; 12.6 mm of D+ is within 0.5 mm of D-.
  FreeRouting does not couple the pair, so the 90 ohm geometry holds only where the tracks happen to run side by side.
- KiCadRoutingTools was also tried (`--best-of`): 17 open nets, worse.
- Other widths FreeRouting used: CC and UART nets 0.127 mm, VBUS 0.5 mm. The CC net width is not a design choice.
- The silkscreen reference text is large and sits away from its part. Labels overlap each other, and some sit outside the board.

Commands (use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts):
```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out /tmp/usb_uart --allow-incomplete
kicad-cli pcb drc --severity-all --refill-zones /tmp/usb_uart/usb_uart.freerouting.kicad_pcb
```
The routed attempt is `usb_uart.freerouting.kicad_pcb`; the unrouted placement is `usb_uart.synth.kicad_pcb`.
Add `--best-of --kicad-routing-tools-repo <dir> --kicad-routing-tools-python <python>` to try the second engine.

## Known limits and gaps

- Schematic: an earlier exporter bug drew the CH342F wires 1.27 mm short of its pins. With commit 9592ad5 (PR #104) applied, the netlist shows U1 connected (only RST and RTS0 left open). Without that fix, KiCad sees every U1 pin as unconnected.
- Schematic look: R_TERM text overlaps, notes sit on top of J1 pin text, U1 top pins crowd their title, and the sheet is A2 with most of it empty. Not tidied.
- Validate still warns: E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio, a count heuristic), W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001 (inner layers not verified), W-SYNTH-SUPPLY-002 x4, W-SYNTH-PART-UNVERIFIED x9.
- No VBUS bulk capacitor, no fuse, no reverse or inrush protection.
- R_TERM (120 ohm across A/B) is always connected. The registry has no 2-pin header (`header_1x2`) for a jumper.
- RS-485: no bias resistors (the THVD1450 has idle, open and short bus failsafe built in, per SLLSEY3E). No surge or TVS on A/B: only the on-chip +/-18 kV IEC contact ESD. TI shows an external circuit for 1 kV surge. No common-mode choke.
- No silkscreen text support in Synth: header labels (GND/TXD1/RXD1 and so on) exist only in the schematic.
- Routing is incomplete (see above), so the board is not fabricable.
- 4-layer stackup and 90 ohm USB pair are declared but not verified.

## Not verified

- Lite-On LED numbers are second-hand (from PR #90); the Lite-On site was not reachable.
- CH342F datasheet: V1E is the newest I found on wch-ic.com. WCH's CH340 sheet was out of date once before, so a newer CH342 sheet may exist.
- Exposed pad: KiCad footprint 2.6 mm, supplier drawing 2.4 mm. Not reconciled.
- Parts availability, JLCPCB assembly, and the registry LCSC numbers were not checked.
- Nothing was powered or tested.
