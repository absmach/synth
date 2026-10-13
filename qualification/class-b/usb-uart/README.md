# USB-C dual UART + RS-485 adapter (SYN-66)

Status: **requires expert review. Not a completed qualification result.** Not fabricated, not tested on hardware.
Revision B (this change) routes with FreeRouting 2.4.1 with 0 open nets. KiCad reports 2 unconnected ground-pour items, so Synth's release gate says `validation_failed` and marks the package UNTRUSTED. DRC reports 0 errors on the raw router file, but 67 clearance errors after a KiCad re-save (59 on revision A); see Routing. See `QUALIFICATION.md` for what Synth checked and what needs an SI/PI/RF expert, and `SHA256SUMS` for the artifact hashes.
Do not call it proven or clean. The list of what is unchecked is at the bottom.

## What it is

- One USB-C port, one WCH CH342F (two UARTs).
- UART1 goes to pin headers as TTL (J2: GND, TXD1, RXD1; J3: DTR1, RTS1, CTS1).
- UART0 drives a TI THVD1450D RS-485 transceiver. Header J4: A, B, GND.
- DTR0/TNOW0 switches the transceiver direction. R_TNOW (4.7k to GND) puts UART0 in the CH342 half-duplex mode.
- Protection: PRTR5V0U2X on D+/D-, PTVS6V0S1UR on VBUS, CC1 and CC2. A 3.3 V LDO (LP5907) feeds the +3V3 rail.
- RS-485 bus side (revision B): R_A and R_B (10 ohm) in series with A and B, then D8 (Bourns CDSOT23-SM712 TVS array) on the bus side, then J4. This follows TI's 1 kV surge circuit (SLLSEY3E fig. 37 and table 7). It is not surge-tested; see the RS-485 section.
- RS-485 termination (revision B): R_TERM (120 ohm) connects across the bus side through the 2-pin jumper J_TERM. Fit the jumper only when this board is at an end of the bus.
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
| D8 | Bourns TVS array for RS-485 | CDSOT23-SM712 | https://www.bourns.com/docs/Product-Datasheets/CDSOT23-SM712.pdf, REV. 06/24 (file sha256 75c1a765dbea8b5d7e57611b8787ce6b2856e57eef387b334bc0a894d91a19d2). LCSC C404012 comes from a search result, not checked. |
| D3 | Lite-On green LED | LTST-C190KGKT | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF, revision not checked (site not reachable) |
| D4, D7 | Lite-On red LED | LTST-C190KRKT | https://optoelectronics.liteon.com/upload/download/DS-22-99-0151/LTST-C190KRKT.PDF, revision not checked (site not reachable) |
| J1 | USB-C receptacle | TYPE-C-31-M-12 | none in the registry, not checked |
| J2, J3, J4, J_VIO | 2.54 mm headers | generic 1x3 | none |
| J_TERM | 2.54 mm header | generic 1x2 | none |
| R_A, R_B | 10 ohm | generic 0603 | none. TI's surge circuit lists a pulse-proof thick-film part (Vishay CRCW0603010RJNEAHP, table 7); this board does not use one. |
| C2, C3 | 2.2 uF X7R 0603 | generic | none |
| C4, C5, C6, C7 | 100 nF X7R 0603 | generic | none |
| R1, R2 | 5.1k | generic | none |
| R3, R4, R7 | 470 | generic | none |
| R5, R6 | 1k | generic | none |
| R8, R9 | 10k | generic | none |
| R_TNOW | 4.7k 1% | generic | none |
| R_TERM | 120 ohm 1% | generic | none |

Checked by me against the manufacturer file in this work: CH342F, THVD1450D (including absolute maximum ratings and the surge circuit), CDSOT23-SM712 (electrical table, from bourns.com), LP5907 (revision line only), the two Nexperia parts (revision line only).
The Nexperia and LP5907 electrical numbers were not re-read.

## Placement and routing

- Outline, revision B: 49.4 x 39.8 mm (Edge.Cuts extents). Revision A was 45.1 x 35.8 mm by the same measure (the first version was 92.6 x 66.8 mm), so the board grew by 4.3 x 4.0 mm.
  Every part has a position in `usb_uart.placement.layout.toml`; the placer adds 4 mm around the part bounding box. The new RS-485 parts (D8, R_TERM, J_TERM, R_A, R_B) sit on the right side. **The delivered board does not match the sidecar for J4:** the sidecar says (35.66, 33.4) and J4 carries an edge hint, but in the routed board it sits 4.0 mm lower and 4.0 mm further left, while J_VIO, J2, J3 and J_TERM are at y 33.4 and no longer flush with the bottom edge. Not tidied.
- J1 sits flush on the top edge. Courtyard overlaps were not re-checked for revision B beyond KiCad DRC (no overlap errors).
- Naming: "revision B" is the name used in these documents for this change. `usb_uart.synth` still declares `revision "A"`, so the title block and BOM say A.
- Pad-edge gap to the target pad on the same net (revision A, not re-measured for revision B):

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

Routing, revision B (FreeRouting 2.4.1, `--router free-routing --krt-allow-via-in-pad`; run on 2026-10-13 with a local build of Synth main 5f62566 plus PR #139 and PR #140, both merged into main afterwards):
- Result: `validation_failed`. Synth's connectivity check: 29 nets connected, 0 open. kicad-cli DRC (`--severity-all --refill-zones`, KiCad 10.0.7) on the file in `kicad/`: 0 errors, **2 unconnected items**, 76 warnings (36 footprint library links, 25 silk over copper, 11 silk overlap, 4 silk edge clearance). The release gate blocks on the 2 unconnected items, so `reports/release.json` says `production_status: untrusted`.
- 295 track segments, 29 vias, 587 mm of copper, 617 s.
- The 2 unconnected items are ground-pour (zone) items. The same 2 appeared in every route that completed (3 of 3: this one and two placement variants). Two other variants left a USB net open, and two timed out at 20 minutes. Cause not found. The exporter only adds 4 corner stitch vias; the rest of the ground connection depends on the router's GND vias. Revision A, routed the same way, had 0 unconnected items. Needs a tool fix or another route; not done here.
- The file in `kicad/` is the router's output as Synth delivers it for a failed gate. Its zones are not filled (KiCad fills them when it runs DRC). The 3D render is from a KiCad-refilled copy.
- DRC caveat: re-saving the file through KiCad (`kicad-cli pcb drc --save-board`) turns the router's old-style `(net 0 "")` pad nets into real no-net pads, and DRC then reports 67 clearance errors on this board (and 59 on revision A) against the project's 0.2 mm default clearance, mostly CC tracks beside the J1 pads (0.5 mm pitch). The numbers above are from the raw file. I did not find out which view is right. A person checking this board should open it in KiCad and look at J1.
- Via-in-pad: one place, the U1 exposed pad (pin 25): a GND via, 0.6 mm pad / 0.3 mm drill (`via at (23.800, 17.300) mm U1.25` in the connectivity record). JLCPCB would have to fill and cap it (epoxy or copper paste, extra cost on a 4-layer board) and the order must ask for it. Not confirmed with JLCPCB for this board. With PR #139 (merged), a routed board lists this site under `routing.fabrication_notes` in `release.json`; this run is not routed, so its `release.json` has no note, and the requirement is stated here and in `QUALIFICATION.md` instead.
- USB pair (revision A numbers, not re-measured for revision B): width 0.242 mm. D+ was 17.03 mm (F.Cu only). D- was 18.43 mm (11.21 F.Cu, 7.23 B.Cu, 2 vias). Skew 1.40 mm. Closest gap 0.133 mm; 12.6 mm of D+ within 0.5 mm of D-.
  FreeRouting does not couple the pair, so the 90 ohm geometry holds only where the tracks happen to run side by side.
- KiCadRoutingTools (now Synth's default router) was tried earlier on an older build (`--best-of`): 17 open nets. It has not been tried again on current main.
- Other widths FreeRouting used: CC and UART nets 0.127 mm, VBUS 0.5 mm. The CC net width is not a design choice.
- The silkscreen reference text is large and sits away from its part. Labels overlap each other, and some sit outside the board.

Commands (use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts):
```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out <dir> --router free-routing --krt-allow-via-in-pad
kicad-cli pcb drc --severity-all --refill-zones --format json <dir>/usb_uart.freerouting.kicad_pcb
kicad-cli sch erc --severity-all --format json <dir>/usb_uart.kicad_sch
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```
`synth export-kicad` writes the project, `release.json` and the routing records (copies are in `reports/`, as captured: they contain the absolute paths of the machine that ran them, and `engine_version` holds the jar path). A board that fails the gate leaves the router's output as `usb_uart.freerouting.kicad_pcb`; a board that passes replaces `usb_uart.kicad_pcb`. Current Synth main defaults to KiCadRoutingTools, which needs its own checkout; this board was routed with FreeRouting, hence `--router free-routing`. The fabrication note (PR #139) and the RS-485 checks (PR #140) are on Synth main now, so any main from 2026-10-13 has both.

## Known limits and gaps

- Schematic: an earlier exporter bug drew the CH342F wires 1.27 mm short of its pins. Fixed by PR #104 (merged); the netlist shows U1 connected (only RST and RTS0 left open).
- Schematic look: R_TERM text overlaps, notes sit on top of J1 pin text, U1 top pins crowd their title, and the sheet is A2 with most of it empty. Not tidied.
- Validate still warns: E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio, a count heuristic), W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001 (inner layers not verified), W-SYNTH-SUPPLY-002 x5, W-SYNTH-PART-UNVERIFIED x10. No errors. With PR #140 (merged), revision A gets `E-SYNTH-ESD-001` twice (the A and B lines reach J4 with no TVS) and `E-SYNTH-RS485-001` once (R_TERM hard-wired across A/B); revision B gets neither (see the RS-485 termination limit below).
- KiCad schematic ERC (`kicad-cli sch erc`, KiCad 10.0.7): 1 error, `power_pin_not_driven` on the VIO power symbol (#PWR2211): VIO comes only through the passive J_VIO jumper, so KiCad sees no power output driving it. The same error is on revision A. Plus 81 library-symbol and 36 footprint-link warnings. Not fixed: Synth's exporter has no power flag for a jumper-selected rail.
- No VBUS bulk capacitor, no fuse, no reverse or inrush protection.
- RS-485 termination: R_TERM is behind the J_TERM jumper. With no jumper the bus is unterminated; with it, 120 ohm sits across the bus side. Synth's `E-SYNTH-RS485-001` and `E-SYNTH-ESD-001` follow only the same net, and R_A/R_B split the transceiver nets from the bus-side nets, so on revision B neither rule checks the bus side. The termination and the TVS were reviewed by reading the schematic only.
- RS-485 surge: R_A, R_B (10 ohm) and D8 follow TI's 1 kV surge circuit (SLLSEY3E fig. 37). Why both are needed: the THVD1450 bus pins have an absolute maximum of +/-18 V (7.1), while the SM712 clamps at 19 V at 1 A and 26 V at 17 A (Bourns REV. 06/24), so a TVS alone does not protect the chip. Not surge-tested. R_A and R_B are generic 0603, where TI lists pulse-proof parts. No common-mode choke. No bias resistors (the THVD1450 has idle, open and short bus failsafe built in, per SLLSEY3E).
- No silkscreen text support in Synth: header labels (GND/TXD1/RXD1 and so on) exist only in the schematic.
- The U1 exposed-pad via needs a filled and capped via at the fab (see Routing).
- 4-layer stackup and 90 ohm USB pair are declared but not verified.

## Not verified

- Lite-On LED numbers are second-hand (from PR #90); the Lite-On site was not reachable.
- CH342F datasheet: V1E is the newest I found on wch-ic.com. WCH's CH340 sheet was out of date once before, so a newer CH342 sheet may exist.
- Exposed pad: KiCad footprint 2.6 mm, supplier drawing 2.4 mm. Not reconciled.
- Parts availability, JLCPCB assembly, and the registry LCSC numbers were not checked.
- The LCSC number for CDSOT23-SM712 (C404012) and the Bourns part's 400 W / 17 A figures come from a search result and the Bourns file; availability and price were not checked.
- Whether the 67 clearance errors seen after a KiCad re-save are real (see Routing).
- Nothing was powered or tested.
