# USB-C dual UART + RS-485 adapter (SYN-66)

Status: design, placement and a FreeRouting route that passes Synth's release gate. The routed PCB, KiCad project and reports are in this PR. Not fabricated, not tested on hardware.
Do not call it proven or clean. The list of what is unchecked is at the bottom.

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

Status: compact placement done and routed with FreeRouting. The route passes Synth's own checks; it is not reviewed or fabricated.

- Outline: 45.0 x 39.73 mm as routed. The placement sidecar gives 45.0 x 35.7 mm; the exporter's repair step moves J3 down out of the header row to y 37.4 mm.
- The four M2.5 holes are board features with a 2.2 mm rim: 3.5 mm from hole centre to the left, right and top edges, 2.2 mm from the hole edge to the board edge. PR 272's 1.5 mm tight rim would put the holes on the edge and split the F.Cu GND pour into two unconnected islands. The 2.2 mm board-feature rim in this branch restores the 45.0 mm width and the pour connection; it is the rim the board was routed with before PR 272.
- Every part has a position in `usb_uart.placement.layout.toml`. J1 sits flush on the top edge. J_VIO, J2, J3 and J4 sit flush on the bottom edge.
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

Routing (FreeRouting 2.4.1, `router: "freerouting"`, `allow_via_in_pad: true`), on Synth main `1eac8a1` (PR #272) plus the board-feature rim fix in this branch:
- Result: routed. `release.json` has `release_ready` true and the gate reports no open net. `kicad-cli pcb drc --severity-all --refill-zones`: 0 errors, 0 unconnected, 42 warnings (36 footprint library links, 5 silk over copper, 1 silk overlap).
- 247 track segments, 41 vias, 544.4 mm of copper.
- The routed board is 45.0 x 39.73 mm.
- J4 is at x 34.66 mm in the sidecar. At 35.66 and 36.66 mm, and turned 180 degrees, FreeRouting left one to three nets open. Its result changes with small placement changes and is not bit-for-bit repeatable.
- Via-in-pad at the U1 exposed pad (pin 25), at (23.800, 17.300) mm. The fab must fill and cap it. JLCPCB has not been asked.
- Signal-integrity pass: 21 warnings, none blocking. 20 are layer changes with no ground return via within 0.64 mm, one is a parallel run of 5.9 mm at 0.177 mm.
- The USB pair is not coupled and its length and skew were not measured for this route. FreeRouting does not couple the pair, so the 90 ohm geometry is unverified.
- No part has been reviewed (`reviewer_state` is false for every part in `release.json`).
- The silkscreen reference text is large and sits close to its part. D4 and D7 labels overlap.

## Committed artifacts

- `kicad/`: the routed PCB, schematic, symbol library, project, DRC rules and BOM.
- `reports/`: `release.json`, the delivered DRC, ERC and the routing record.
- `images/`: 3D top and angled, top and bottom copper, and the schematic.
- `SHA256SUMS`: hashes of the artifacts above.

The reports are as captured, so they contain the absolute paths of the machine that produced them.

The committed board came from the MCP `synth_export` tool with `router: "freerouting"` and `allow_via_in_pad: true`. The CLI equivalent (same binary, same engine defaults) is below.

## Commands (use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts)

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh --freerouting-only
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out /tmp/usb_uart --router free-routing --krt-allow-via-in-pad --allow-incomplete
kicad-cli pcb drc --severity-all --refill-zones --format json /tmp/usb_uart/usb_uart.kicad_pcb -o /tmp/usb_uart/usb_uart-drc.json
kicad-cli sch erc --severity-all --format json /tmp/usb_uart/usb_uart.kicad_sch -o /tmp/usb_uart/usb_uart-erc.json
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```

A route that passes the gate is `usb_uart.kicad_pcb`; a failed one stays in `usb_uart.freerouting.kicad_pcb`. The unrouted placement is `usb_uart.synth.kicad_pcb`.
Current Synth main defaults the MCP export to KiCadRoutingTools; this board was routed with FreeRouting. Add `--best-of --kicad-routing-tools-repo <dir> --kicad-routing-tools-python <python>` to the CLI to try KiCadRoutingTools.

## Known limits and gaps

- Schematic: an earlier exporter bug drew the CH342F wires 1.27 mm short of its pins. With commit 9592ad5 (PR #104) applied, the netlist shows U1 connected (only RST and RTS0 left open). Without that fix, KiCad sees every U1 pin as unconnected.
- Schematic look: the compiler's own layout, not tuned by hand. See `images/schematic.png`.
- KiCad schematic ERC: 1 error, `power_pin_not_driven` on the VIO symbol (VIO arrives only through J_VIO), plus 119 warnings (81 lib_symbol_issues, 36 footprint_link_issues, 1 label_multiple_wires, 1 multiple_net_names).
- Validate still warns: E-SYNTH-POWER-004 x2 (VBUS and GND load-to-capacitor ratio, a count heuristic), E-SYNTH-SCHEM-005 x2 (CH342F bridge junction fan-out), W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001 (inner layers not verified), W-SYNTH-SUPPLY-002 x8, W-SYNTH-PART-UNVERIFIED x9.
- No VBUS bulk capacitor, no fuse, no reverse or inrush protection.
- R_TERM (120 ohm across A/B) is always connected. The registry has no 2-pin header (`header_1x2`) for a jumper.
- RS-485: no bias resistors (the THVD1450 has idle, open and short bus failsafe built in, per SLLSEY3E). No surge or TVS on A/B: only the on-chip +/-18 kV IEC contact ESD. TI shows an external circuit for 1 kV surge. No common-mode choke.
- No silkscreen text support in Synth: header labels (GND/TXD1/RXD1 and so on) exist only in the schematic.
- The route passes Synth's own gate but nobody has reviewed it, so the board is not ready to fabricate.
- 4-layer stackup and 90 ohm USB pair are declared but not verified.

## Not verified

- Lite-On LED numbers are second-hand (from PR #90); the Lite-On site was not reachable.
- CH342F datasheet: V1E is the newest I found on wch-ic.com. WCH's CH340 sheet was out of date once before, so a newer CH342 sheet may exist.
- Exposed pad: KiCad footprint 2.6 mm, supplier drawing 2.4 mm. Not reconciled.
- Parts availability, JLCPCB assembly, and the registry LCSC numbers were not checked.
- Nothing was powered or tested.
