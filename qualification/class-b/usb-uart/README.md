# USB-C dual UART + RS-485 adapter (SYN-66)

**Not Class-B qualified**: no independent review, no hardware test, and the JLCPCB DFM record is for the previous revision. Status: design, placement and a FreeRouting route that passes Synth's release gate (which covers only Synth's routing and export gates). The routed PCB, KiCad project and reports are in this PR. Not fabricated, not tested on hardware.
Do not call it proven or clean. The list of what is unchecked is at the bottom.

Intended use: bench or lab adapter, short cables, point-to-point or last node on an RS-485 bus. Not for industrial installs, long cable runs or a node in the middle of a multidrop bus: A and B have no surge protection beyond the transceiver's built-in ESD, and the 120 ohm termination is always connected. The surge parts and a selectable termination are a later revision; the registry parts for them are in #356.

## What it is

- One USB-C port, one WCH CH342F (two UARTs).
- UART1 goes to pin headers as TTL (J2: GND, TXD1, RXD1; J3: DTR1, RTS1, CTS1).
- UART0 drives a TI THVD1450D RS-485 transceiver. Header J4: A, B, GND.
- DTR0/TNOW0 switches the transceiver direction. R_TNOW (4.7k to GND) puts UART0 in the CH342 half-duplex mode.
- Protection: PRTR5V0U2X on D+/D-, PTVS6V0S1UR on VBUS, CC1 and CC2. A 3.3 V LDO (LP5907) feeds the +3V3 rail.
- Four M2.5 mounting holes (H1 to H4), pinned in the placement sidecar: two at the top corners, two on the left and right edges at mid-height. The bottom corners are taken by the header row. Screw size and standoff height are not chosen.
- Source: `usb_uart.synth`. Sidecar: `usb_uart.placement.layout.toml` (board mm). The schematic has no sidecar: the compiler lays it out, one region per group.

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

Status: compact placement done and routed with FreeRouting. The route passes Synth's release gate; it is not reviewed or fabricated.

- Two copper layers: signals on F.Cu and B.Cu, a GND pour on both. The route uses no inner layer because the board has none; the previous revision was four layers with two inner GND planes that carried no routing.
- Outline: 40.99 x 32.23 mm as routed. The four M2.5 holes are at x 5.5 and 39.6 mm, y 3.0 and 21.0 mm, with a 2.2 mm board-feature rim. The header courtyards at y 29.9 mm set the 32.23 mm height. This is 3.5 mm shorter than the previous 35.73 mm compact layout and 7.5 mm shorter than the 39.73 mm committed board.
- Board-feature rim: Synth gives a mounting hole, test point or fiducial a fixed 2.2 mm rim in the fitted outline. With this sidecar the rim does not decide the outline: the holes are not the outermost parts, and the export gives the same outline with the rim set to 0. It matters if a hole is moved to the edge, where the tight 1.5 mm fit would otherwise cut it and split the F.Cu GND pour. `tests/usb_uart_board.rs` checks that every hole keeps at least 1.5 mm of room to Edge.Cuts on the exported board. The exporter sets `island_removal_mode 0`, dropping copper islands that touch no pad. KiCad lists a GND zone against itself when two fill islands that reach pads are not joined; Synth's gate now blocks on that. The previous revision had exactly this split.
- Every part has a position in `usb_uart.placement.layout.toml`. J1 sits flush on the top edge. J_VIO, J2, J3 and J4 sit at y 29.9 mm. J_VIO, J2, J3 and J4 are at x 7.36, 16.46, 26.0 and 35.1 mm. J3 and J4 each moved 1 mm left of their previous 27.0 and 36.1 mm: at the previous spot the GND pour split into two islands (C4 pin 2, J2 pin 1, D3 pin 1 and one via were cut off) and, with current KiCad and FreeRouting here, DTR1 was left unrouted. The 9.1 mm spacing between J3 and J4 keeps their 8.62 mm courtyards apart. The bottom parts sit at y 23.6 and 25.8 mm. R1 and R2 use the top corners; R3 moved to x 14.5 mm to clear H3.
- No courtyard overlaps. J1 courtyard to top edge: 0.01 mm. That is the 0.5 mm courtyard margin reaching the edge: the shell front face sits 0.50 mm inside Edge.Cuts. The HRO drawing states no overhang limit, so only the enclosure cutout fit is left to check.
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
- R1 and R2 are in the top corners; the remaining passives are placed by hand. The DRC checks their courtyards, not placement quality.

Routing (FreeRouting 2.4.1, `router: "freerouting"`, `allow_via_in_pad: true`), with the Synth sources at `08b917b` (`source_revision` in `reports/release.json`):
- Result: routed. `release.json` has `release_ready` true and `routing.state` "routed", with no open net. `kicad-cli pcb drc --severity-all --refill-zones`: 0 errors, 0 warnings and no `unconnected_items`. Synth's gate no longer accepts a GND zone that KiCad lists against itself, and the delivered board has none. pcbnew's connectivity puts every GND pad and via in one cluster, so the pour is connected. (The earlier revision's 36 library-link warnings came from a run without the KiCad libraries.)
- 273 track segments, 43 vias, 515.4 mm of copper.
- The routed board is 40.99 x 32.23 mm.
- FreeRouting's escape analysis flags J1 and U1 (0.5 mm pitch) as the packages where neither a fabrication-floor via nor a minimum track fits between adjacent pads, and suggests 4 layers. The route still completes, with its only via-in-pad at U1 pin 25.
- J3 is at x 26.0 mm and J4 at x 35.1 mm in the sidecar. Small placement changes change FreeRouting's result: shifts of 0.5 mm in J3 and J4 left DTR1, TNOW0 or +3V3 unrouted, and a 0.5 mm shift of C4 left USB_DN open. The same inputs gave the same board on three runs.
- Via-in-pad at the U1 exposed pad (pin 25), at (23.800, 17.300) mm. The fab must fill and cap it. JLCPCB has not been asked.
- Signal-integrity pass: 23 warnings, none blocking. All 23 are layer changes with no ground return via within 1.53 mm, so the return current has to jump between the F.Cu and B.Cu pours.
- The USB pair is not coupled. Measured on the committed board: full-speed part, pair on F.Cu only with no via (the 23 layer-change warnings are on other nets), DP 38.63 mm and DN 13.52 mm so 25.11 mm of skew (about 0.2 percent of a full-speed bit), and 91.4 percent of the length over solid B.Cu GND fill. These are my measurements, not an independent review; see `qualification.json`. FreeRouting does not couple the pair. On two layers at 1.6 mm, a 90 ohm differential pair needs about 1.18 mm traces, so the pair has no impedance target and no controlled-impedance geometry (the previous four-layer revision declared 90 ohm but never verified it).
- No part has been reviewed (`reviewer_state` is false for every part in `release.json`).
- Silkscreen: the D7, J4, J_VIO and U1 reference labels are placed by hand in the sidecar. `kicad-cli pcb drc` reports no silkscreen warning.
- TX LEDs: D4 and D7 take their current from VIO through R4 and R7 and sink it into TXD0 and TXD1, so they are dark while a line idles and lit on the low bits. Each draws about 3 mA at 3.3 V and 6.6 mA at 5 V from the CH342 TXD pin (an estimate from a 1.9 V forward drop; the CH342 output drive was not checked at 5 V).

## Committed artifacts

- `kicad/`: the routed PCB, schematic, symbol library, project, DRC rules and BOM.
- `reports/`: `release.json`, the delivered DRC, ERC and the routing record. Its `production_status` and `release_ready` fields only mean the route passed Synth's router and DRC gates, and its `gate_scope` field says so. Neither is a fabrication approval.
- `qualification.json`: the qualification gate. `qualified` stays false until every review gate in it is true.
- `dfm/`: the gerber zip, CPL and screenshots from the JLCPCB quote and DFM flow on 2026-10-27 (no order placed). It is partial: see `jlcpcb_dfm` in `qualification.json`.
- `images/`: 3D top and angled, top and bottom copper, and the schematic. The 3D and copper pictures were rendered with `kicad-cli pcb render` and `pcb export pdf` from the committed board.
- `dfm/`: regenerated Gerbers, drill and CPL for this board. The JLCPCB screenshots there are from the previous revision and were not redone; see `qualification.json`.
- `SHA256SUMS`: hashes of the artifacts above.

The reports are as the commands produce them, except that the absolute paths of the machine that produced them were cut to repo-relative ones (`out/...` is the export directory) and `source_revision` in `reports/release.json` was added by hand. The release report is the CLI's own format (`synth.release.v1`), which differs from the MCP report in the earlier revision.

The committed board came from the CLI commands below (FreeRouting engine, via-in-pad allowed).

## Commands (use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts)

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh --freerouting-only
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out /tmp/usb_uart --router free-routing --krt-allow-via-in-pad
kicad-cli pcb drc --severity-all --refill-zones --format json /tmp/usb_uart/usb_uart.kicad_pcb -o /tmp/usb_uart/usb_uart-drc.json
kicad-cli sch erc --severity-all --format json /tmp/usb_uart/usb_uart.kicad_sch -o /tmp/usb_uart/usb_uart-erc.json
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```

A route that passes the gate is `usb_uart.kicad_pcb`; a failed one stays in `usb_uart.freerouting.kicad_pcb`. The unrouted placement is `usb_uart.synth.kicad_pcb`.
Current Synth main defaults the MCP export to KiCadRoutingTools; this board was routed with FreeRouting. Add `--best-of --kicad-routing-tools-repo <dir> --kicad-routing-tools-python <python>` to the CLI to try KiCadRoutingTools.

## Known limits and gaps

- Schematic: an earlier exporter bug drew the CH342F wires 1.27 mm short of its pins. With commit 9592ad5 (PR #104) applied, the netlist shows U1 connected (only RST and RTS0 left open). Without that fix, KiCad sees every U1 pin as unconnected.
- Schematic look: the compiler's own layout, not tuned by hand. See `images/schematic.png`.
- KiCad schematic ERC: 0 errors and 1 warning (label_multiple_wires); a run without the KiCad libraries also reports 80 lib_symbol_issues and 36 footprint_link_issues. VIO arrives only through J_VIO, so the exporter puts a PWR_FLAG on it.
- Validate still warns: E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio, a count heuristic), E-SYNTH-SCHEM-005 x2 (CH342F bridge junction fan-out), W-SYNTH-ANOMALY-001, W-SYNTH-SUPPLY-002 x8, W-SYNTH-PART-UNVERIFIED x9.
- No VBUS bulk capacitor, no fuse, no reverse or inrush protection.
- R_TERM (120 ohm across A/B) is always connected. The registry has no 2-pin header (`header_1x2`) for a jumper.
- RS-485: no bias resistors (the THVD1450 has idle, open and short bus failsafe built in, per SLLSEY3E). No surge or TVS on A/B: only the on-chip +/-18 kV IEC contact ESD. TI shows an external circuit for 1 kV surge. No common-mode choke.
- No silkscreen text support in Synth: header labels (GND/TXD1/RXD1 and so on) exist only in the schematic.
- The route passes Synth's own gate but nobody has reviewed it, so the board is not ready to fabricate.
- Two-layer board with no declared stackup: the exporter writes the 1.6 mm default thickness. There is no controlled-impedance net, and the USB pair is not impedance-controlled.

## Not verified

- Lite-On LED numbers are second-hand (from PR #90); the Lite-On site was not reachable.
- CH342F datasheet: V1E is the newest I found on wch-ic.com. WCH's CH340 sheet was out of date once before, so a newer CH342 sheet may exist.
- Exposed pad: KiCad footprint 2.6 mm, supplier drawing 2.4 mm. Not reconciled.
- Parts availability, JLCPCB assembly, and the registry LCSC numbers were not checked.
- Nothing was powered or tested.

Reproducibility: on 2026-10-27 three runs of the commands above (FreeRouting 2.4.1 on Java 25, KiCad 10.0.7, Synth at the commit in `reports/release.json` `source_revision`, two of them with an empty `XDG_DATA_HOME`) gave a byte-identical `kicad_pcb`; the schematic is unchanged. The FreeRouting jar and JRE are untracked and come from `tools/` after `scripts/setup-routing-engines.sh --freerouting-only`. The router is sensitive: the previous placement did not route on this setup, so a different FreeRouting or KiCad version may need a different placement. I did not try another machine. `qualification.json` lists the input and output hashes.

The ERC findings and the silkscreen and edge-clearance items are dispositioned one by one in `qualification.json`. The 0.01 mm J1 courtyard clearance is the courtyard margin only (shell face 0.50 mm inside the edge, per the HRO drawing); independent mechanical approval of the cutout fit is still open; the silkscreen warnings are fixed with label overrides in the sidecar. The exported schematic used to tie RXD1_MCU to MODEM_IDLE through a name label that sat on a crossing wire; the exporter now keeps that label off other nets' wires, and the schematic and ERC report were regenerated.
