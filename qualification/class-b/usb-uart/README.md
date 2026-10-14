# USB-C dual UART + RS-485 adapter (SYN-66)

Status: **requires expert review. Not a completed qualification result.** Not fabricated, not tested on hardware.
Revision B (this change) routes with FreeRouting 2.4.1 with 0 open nets and passes Synth's release gate (`release_ready: true`; a gate pass is not a qualification, see below). KiCad DRC reports 0 errors and 0 unconnected items, also after a KiCad re-save. The re-save used to show 63 clearance errors; that was a project-file bug, fixed in PR #154 (see Routing). The USB-C connector is now the Amphenol 12401610E4#2A, with its 3D model; four M3 mounting holes sit at the sides and J1 is centred on the board. See `QUALIFICATION.md` for what Synth checked and what needs an SI/PI/RF expert, and `SHA256SUMS` for the artifact hashes.
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
| J1 | USB-C receptacle | Amphenol 12401610E4#2A | not read: amphenol-cs.com returned 403; pad numbers are KiCad's; ratings and LCSC number not checked |
| H1 to H4 | M3 mounting holes (NPTH, 3.2 mm) | none | none |
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

- Outline, revision B: 50.5 x 38.4 mm (Edge.Cuts extents, measured on the delivered board). Revision A was 45.1 x 35.8 mm by the same measure (the first version was 92.6 x 66.8 mm). The board grew 5.4 mm in width for the RS-485 parts and 2.6 mm in height because the Amphenol connector is deeper and has two staggered rows of 0.5 mm-pitch pads.
  Every part has a position in `usb_uart.placement.layout.toml`; the placer adds 4 mm around the part bounding box. The five headers (J_VIO, J2, J3, J4, J_TERM) all carry the bottom-edge hint and sit in one straight row at y 36.2, flush with the bottom edge; the delivered board matches the sidecar. J4 is turned 180 degrees so its pin 1 (RS-485 A) is on the right, next to the RS-485 parts, which keeps A and B from crossing on the way to the header; D8 sits beside J4. Without turning J4, FreeRouting left the B net open at J4 in 6 of 6 tries; turning J4 alone still left two other nets open; turning it and moving D8 beside it routed every net.
- J1 sits flush on the top edge and is centred: its centre is at x 25.25 mm and the outline's is at x 25.25 mm. The four mounting holes (M3, 3.2 mm, NPTH) are at (3.5, 3.45) and (47.0, 3.45) in the top corners and at (3.5, 23.9) and (47.0, 23.9) halfway down both sides. They are not in the bottom corners: the header row is flush with the bottom edge and its courtyards start 4 mm from the sides, so a hole's 6.9 mm courtyard does not fit there. KiCad DRC reports no courtyard overlap and no label overlap or label clipped by the board edge on this board. That depends on the reference-label placement in PR #144 (merged); with the old fixed label offset the same layout had 11 to 12 label-overlap warnings and the labels were cut by the board edge. A few labels in the dense area near U1 (C6, D6, D1) still look crowded in the render.
- Naming: "revision B" is the name used in these documents for this change. `usb_uart.synth` still declares `revision "A"`, so the title block and BOM say A.
- Pad-edge gap to the target pad on the same net (revision A, not re-measured for revision B):

| Part | Target | Gap mm |
| --- | --- | --- |
| C4 (VDD5) | U1 pin 7 | not re-measured (C4 was moved 0.95 mm left in this revision, see Routing) |
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

Routing, revision B (FreeRouting 2.4.1, `--router free-routing --krt-allow-via-in-pad`; run on 2026-10-14 with a local build of this branch merged with Synth main ed5e658 (it includes PR #139, #140 and #144) plus PR #154, the net class clearance fix, not merged yet):
- Result: routed, gate passed. Synth's connectivity check: 29 nets connected, 0 open. kicad-cli DRC (`--severity-all --refill-zones`, KiCad 10.0.7, with the project's `usb_uart.kicad_dru`) on the file in `kicad/`: 0 errors, **0 unconnected items**, 54 warnings (40 footprint library links, 14 silk over copper). `reports/release.json`: `production_status: production`, `release_ready: true`. This only says the tool's checks passed; it does not make this board qualified (see `QUALIFICATION.md`).
- 306 track segments (229 F.Cu, 77 B.Cu), 36 vias, 584 mm of copper, 206 s. Reaching this took a search: the layouts with the Amphenol connector and the M3 holes left 1 to 3 nets open, a stray ground-pour piece or DRC errors in 22 of 23 tries (rows below J1 shifted by 2.5 to 5.9 mm, cluster offsets and hole positions varied); the delivered one is a shift of 2.8 mm with the USB protection cluster 3.05 mm to the right. FreeRouting's result changes from one placement to the next, so this is one routed result, not a tuned optimum.
- The two ground-pour items of the previous run were real. The ground pins of C4 (VBUS decoupling) and D1 (the USB ESD array) each sat in a small top-layer pour piece that no via tied to the ground planes, so they had no ground connection at all (KiCad's pour refill cut the pieces off; Synth's own connectivity check counts any pad lying on a pour as connected, so it did not see them). The router's stitching step found both and could not place a via, and printed `pour piece on F.Cu still cut off from its net (C4.2)` and `(D1.1)`. Moving C4 from x 22.55 to x 21.6 mm gave the pieces room: the stitching step then placed its vias and the warnings are gone. On the delivered board the stitching step placed 4 vias and reported no piece left cut off. A synth-side check that treats an isolated pour piece as unconnected is not done.
- The file in `kicad/` is the board as Synth delivers it for a passed gate: the router's output with the pours filled. The 3D render is from the same file after a KiCad refill. The pictures in `images/` (3D top, angled and bottom views, top and bottom copper, schematic) and `3d/usb_uart.step` are made from the same files. `3d/usb_uart.step` is the whole board as a STEP assembly (`kicad-cli pcb export step --subst-models`, from the same refilled file), including KiCad's own STEP model of the Amphenol connector and of the other parts that have one; the mounting holes are plain NPTH holes with no model. The connector's own model is `Connector_USB.3dshapes/USB_C_Receptacle_Amphenol_12401610E4-2A.step` in KiCad's `kicad-packages3d` (CC BY-SA 4.0 with KiCad's design exception).
- DRC re-save: the project file used to give every net class without a declared clearance KiCad's 0.2 mm default, while the router works to 0.127 mm; KiCad checks the larger class clearance, so a KiCad re-save (`kicad-cli pcb drc --save-board`) showed 63 clearance errors on this board (59 on revision A) that the raw file did not. PR #154 makes such classes take the manufacturer minimum. With that change the re-saved board reports 0 clearance errors. Clearance of the 0.5 mm-pitch J1 pads was therefore checked against 0.127 mm, the process minimum, not against a wider rule.
- DRC rules the project file cannot carry: KiCad checks copper to the board edge and to holes with rules that default to 0.5 mm and 0.25 mm, and `kicad-cli` does not read them from the `.kicad_pro`. The J1 connector's NPTH holes sit 0.30 to 0.33 mm from its pads, which JLCPCB allows (its stated minimum is 0.2 mm for copper to a routed edge and for an NPTH or via hole to track). PR #154 therefore writes `usb_uart.kicad_dru` with both rules at 0.2 mm and runs the gate's DRC with it. This is the manufacturer's number, so the DRC no longer holds this board to KiCad's stricter default; a reviewer may want the wider margin.
- Via-in-pad: one place, the U1 exposed pad (pin 25): a GND via, 0.6 mm pad / 0.3 mm drill (`via at (23.800, 19.800) mm U1.25` in the connectivity record). JLCPCB would have to fill and cap it (epoxy or copper paste, extra cost on a 4-layer board) and the order must ask for it. Not confirmed with JLCPCB for this board. With PR #139 (merged), a routed board lists this site under `routing.fabrication_notes` in `release.json`, and this board's does.
- USB pair (revision B, track centre-line length from the board file): width 0.242 mm. D+ is 31.77 mm (all F.Cu, no vias). D- is 45.28 mm (6.09 F.Cu, 39.2 B.Cu, 3 vias). Skew 13.5 mm: D- takes a long detour on the bottom layer. That is a poor USB pair; it is the router's result for this placement and nobody tuned it. Gap and coupled length were not re-measured (revision A: closest gap 0.133 mm, 12.6 mm of D+ within 0.5 mm of D-).
  FreeRouting does not couple the pair, so the 90 ohm geometry holds only where the tracks happen to run side by side.
- KiCadRoutingTools (now Synth's default router) was tried earlier on an older build (`--best-of`): 17 open nets. It has not been tried again on current main.
- Other widths FreeRouting used: CC and UART nets 0.127 mm. VBUS is routed at 0.3 mm (net class `VBUS_FINE` in `usb_uart.synth`): the connector's contacts are 0.3 mm wide at 0.5 mm pitch, so with the 0.5 mm Synth derived for VBUS from its current the router left VBUS open in every placement tried. By the IPC-2221 external-layer formula 0.3 mm of 1 oz copper carries about 1 A for a 10 C rise (my calculation, not a Synth check), above the 500 mA this bus-powered board is meant to draw; it is not sized for 3 A. The CC net width is not a design choice.
- The silkscreen reference text is large. With PR #144 no label sits outside the board or overlaps by DRC, but a few near U1 (C6, D6, D1) still crowd each other in the render.

Commands (use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts):
```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out <dir> --router free-routing --krt-allow-via-in-pad
kicad-cli pcb drc --severity-all --refill-zones --format json <dir>/usb_uart.kicad_pcb
kicad-cli sch erc --severity-all --format json <dir>/usb_uart.kicad_sch
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```
`synth export-kicad` writes the project, `release.json` and the routing records (copies are in `reports/`, as captured: they contain the absolute paths of the machine that ran them, and `engine_version` holds the jar path). A board that fails the gate leaves the router's output as `usb_uart.freerouting.kicad_pcb`; a board that passes replaces `usb_uart.kicad_pcb`. Current Synth main defaults to KiCadRoutingTools, which needs its own checkout; this board was routed with FreeRouting, hence `--router free-routing`. The fabrication note (PR #139) and the RS-485 checks (PR #140) are on Synth main now, so any main from 2026-10-13 has both.

## Known limits and gaps

- Schematic: an earlier exporter bug drew the CH342F wires 1.27 mm short of its pins. Fixed by PR #104 (merged); the netlist shows U1 connected (only RST and RTS0 left open).
- Schematic look: R_TERM text overlaps, notes sit on top of J1 pin text, U1 top pins crowd their title, and the sheet is A2 with most of it empty. Not tidied.
- Validate still warns: E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio, a count heuristic), W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001 (inner layers not verified), W-SYNTH-SUPPLY-002 x9, W-SYNTH-PART-UNVERIFIED x11. No errors. With PR #140 (merged), revision A gets `E-SYNTH-ESD-001` twice (the A and B lines reach J4 with no TVS) and `E-SYNTH-RS485-001` once (R_TERM hard-wired across A/B); revision B gets neither (see the RS-485 termination limit below).
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
- Whether any other project-file rule, besides the net class clearance, differs from what the router used.
- Nothing was powered or tested.
