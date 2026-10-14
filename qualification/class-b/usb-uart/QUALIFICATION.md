# Qualification matrix: USB-C dual UART + RS-485 adapter (SYN-66)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the evidence. Section B lists what no tool here proves and an SI/PI/RF or manufacturing expert has to sign off. No expert has reviewed this board.

Revision B routes with 0 open nets and passes Synth's release gate (`release_ready: true` in `reports/release.json`, see A5). A passed gate means the tool's checks passed; it is not a qualification.

## Reproduce

Run from the repository root. Use Synth `main` from 2026-10-13 or later: it includes PR #139 (fabrication note, merged as `de4a5b2`) and PR #140 (RS-485 checks, merged as `bb9ae95`). The board's reference-label positions depend on PR #144 (label placement, merged), its project file and DRC rules on PR #154 (manufacturer clearances in the project file, not merged yet; without it a KiCad re-save shows 63 clearance errors, see B9), and the outline round the mounting holes on PR #162 (not merged yet; without it the four M3 holes grow the board by about 7 mm in each direction). Use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out <dir> --router free-routing --krt-allow-via-in-pad
kicad-cli pcb drc --severity-all --refill-zones --format json <dir>/usb_uart.kicad_pcb
kicad-cli sch erc --severity-all --format json <dir>/usb_uart.kicad_sch
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```

Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.4.1, Synth `main` 157f6ea (includes PR #139, #140 and #144) plus PR #154 and PR #162 (not merged), on 2026-10-14. Routing is not bit-for-bit repeatable across placements: small placement changes gave different results (see A5); the same inputs gave the same board on repeat.

## Artifact hashes

`SHA256SUMS` in this folder lists every design artifact (source, sidecars, KiCad files, BOM, 3D renders, copper and schematic pictures, the board STEP, routing and DRC reports). It leaves out `README.md`, `QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `usb_uart.synth` | `cdd5eb9b9070265da6bd69e23081cf5155b759ee721d0aa513c11218777a55f6` |
| `usb_uart.placement.layout.toml` | `d38408e9d0e080ebdd91577fdf8b47b0fe71a93a8561e5c9f9040a5ed5197230` |
| `kicad/usb_uart.kicad_pcb` | `1a9df574466f38ce8de18e62e2b7c1f563bb0db9a499a9f7d6610792bfe6c266` |
| `kicad/usb_uart.kicad_sch` | `8670d99a27b044f011658d51605427d25fb1bdae44ce48820820d3e5e674d127` |
| `kicad/bom.csv` | `5852e79981c28ef46446095c433feff0890cfda97c7a32e97357a0c309aeede8` |
| `reports/release.json` | `b7f1415987614d8b12e91ec9270be460d6fdb0be2037605857a170f2983d09c9` |
| `reports/usb_uart.drc.json` | `1b264dd4cb04036b58080700df1efed7f6df67b0d32600c3e4de61e072216dcb` |

`reports/` holds the files as captured, so they contain the absolute paths of the machine that produced them, and `engine_version` in `release.json` is a jar path.

Datasheet files read for this revision (not committed, so the figures taken from them are not checkable from this repository): Bourns CDSOT23-SM712 REV. 06/24, `75c1a765dbea8b5d7e57611b8787ce6b2856e57eef387b334bc0a894d91a19d2` (VWM 12 V / 7 V, clamp 19 V at 1 A, 26 V at 17 A, 400 W / 17 A); TI THVD1450 SLLSEY3E (May 2018, revised May 2019) (bus abs max +/-18 V in 7.1, surge circuit in fig. 37 and table 7). The JLCPCB statement in B8 comes from JLCPCB's own pages as read on 2026-10-12.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate` | No errors. Warnings only (the `E-` ones are also severity warning): E-SYNTH-POWER-004 x2, W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001, W-SYNTH-SUPPLY-002 x9, W-SYNTH-PART-UNVERIFIED x11 |
| A2 | Every net is connected end to end | Synth connectivity re-derived from copper, `reports/usb_uart.connectivity.json` | 29 nets connected, 0 open, 0 unconnected pads |
| A3 | No KiCad DRC rule violation on the routed board | `kicad-cli pcb drc --refill-zones` on the raw router file, `reports/usb_uart.drc.json` | 0 errors, 0 unconnected, 54 warnings (40 library links, 14 silk over copper; no label overlap and no label clipped by the board edge), run with the project's `usb_uart.kicad_dru` (copper to a board edge 0.3 mm, to a hole or an NPTH hole 0.2 mm, from the JLCPCB profile, instead of KiCad's 0.5 and 0.25 mm defaults; see B9). After a KiCad re-save (`--save-board`) the same copper also shows 0 errors with PR #154's project file; see B9 |
| A4 | The RS-485 line is protected and the terminator is switchable | `E-SYNTH-ESD-001` and `E-SYNTH-RS485-001` (PR #140) | Revision A: 2 + 1 warnings. Revision B: none. **Weak evidence:** both rules follow only the same net, and R_A/R_B split the nets, so on revision B the rules cannot see the bus side. This was confirmed by reading the schematic only |
| A5 | The package passes Synth's release gate | `synth export-kicad`, `reports/release.json` | **Yes**, with C4 moved to x 21.6. Before that move the gate failed: KiCad reported 2 unconnected ground-pour items, the ground pins of C4 and D1 in top-layer pour pieces with no via to the ground planes (a real missing ground connection, which Synth's own connectivity check did not see because it counts a pad on a pour as connected). The router's stitching step printed `pour piece on F.Cu still cut off from its net` for both. Other placements left nets open or timed out; the aligned header row only routed after J4 was turned 180 degrees and D8 moved beside it. A passed gate is not a qualification |
| A6 | Schematic electrical rules | `kicad-cli sch erc` | 1 error (`power_pin_not_driven` on the VIO symbol, jumper-selected rail, also on revision A), 117 warnings |
| A7 | Via-in-pad sites are found and recorded | Synth connectivity `via_in_pad`; PR #139 fabrication note | One site: U1 exposed pad (GND, at 23.8, 19.8). The note is written for a routed board and is in this board's `release.json` |
| A8 | The four parts added or changed here match their KiCad footprints | `synth registry qualify` | `header_1x2`, `cdsot23_sm712`, `thvd1450d` and `usb_c_receptacle_amphenol_12401610e4_2a` have no pin-to-pad, mirrored-footprint or dimension finding. Each has one blocking finding, `E-SYNTH-QUAL-008` (no reviewer or provenance); the Amphenol part also has `E-SYNTH-QUAL-009` (no datasheet URL: the manufacturer's site refused every request, so its datasheet was not read). Like every part that command listed in my run (all of them blocked), they stay `blocked` until a reviewer is named, and none has one |

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | USB D+/D- is a 90 ohm differential pair | Declared (`impedance 90ohm`). Synth gives only a closed-form estimate and says it does not verify inner layers (W-SYNTH-IMPEDANCE-001). The 4-layer stackup and the fab's real stackup are unverified. FreeRouting does not couple the pair. Revision A: width 0.242 mm, closest gap 0.133 mm | Requires expert review |
| B2 | USB return path and layer transitions | D- changes layer through vias (revision A: 2). Whether a ground return via sits beside each signal via was not analysed | Requires expert review |
| B3 | USB coupling and skew | Revision B skew 13.5 mm (D+ 31.77 mm all on F.Cu with no vias, D- 45.28 mm with 3 vias and 39.2 mm on B.Cu; centre-line length from the board file). A long detour on one line; this is a poor pair for USB, left as the router produced it. Gap and coupled length not re-measured (revision A: 1.40 mm skew, 12.6 mm of D+ within 0.5 mm of D-). No simulation | Requires expert review |
| B4 | RS-485 surge protection (IEC 61000-4-5) | Topology follows TI's 1 kV circuit (SLLSEY3E fig. 37): 10 ohm series, TVS on the bus side. R_A and R_B are generic 0603; TI lists pulse-proof parts. THVD1450 bus abs max is +/-18 V and the SM712 clamps at 19 V (1 A) to 26 V (17 A), so the series resistors matter. Not surge-tested, no expert approval | Requires expert review |
| B5 | RS-485 common-mode range, bias and fail-safe | Relies on the THVD1450's own fail-safe (per SLLSEY3E). No bias network, no common-mode choke. SM712 stand-off is 12 V / 7 V, matching the +12 / -7 V range, from the Bourns datasheet only | Requires expert review |
| B6 | RS-485 termination | Selectable through J_TERM. With no jumper the bus is unterminated. Correct jumper use depends on where the board sits on the bus. Bus-side layout and stub lengths were not analysed | Requires expert review |
| B7 | Power integrity | LDO, VBUS and VIO rails, decoupling placement and loop areas were not analysed. No VBUS bulk capacitor, fuse, reverse or inrush protection | Requires expert review |
| B8 | Via-in-pad fabrication | The U1 exposed-pad via (0.6 mm pad / 0.3 mm drill) needs a filled and capped via. JLCPCB lists it as a paid option on 4-layer boards. The order must ask for it. **Acceptance by the manufacturer is not confirmed for this board** | Requires manufacturer confirmation |
| B9 | Clearance at the 0.5 mm-pitch J1 pads | The router works to 0.127 mm, the JLCPCB minimum. The 63 errors a KiCad re-save used to show (59 on revision A) came from the project file giving net classes KiCad's 0.2 mm default (PR #154); with 0.127 mm the re-saved board has none. The J1 connector's NPTH holes sit 0.30 to 0.33 mm from its pads and a track passes 0.24 mm from one; the custom DRC rules hold copper to an NPTH hole at JLCPCB's 0.2 mm (a board edge stays at 0.3 mm), not KiCad's 0.5 / 0.25 mm defaults. Whether 0.127 mm clearance and 0.2 mm to the NPTH are acceptable for this order is for the fab and a reviewer to confirm | Requires review |
| B10 | Thermal and EMC | Not analysed | Requires expert review |
| B11 | Function on real hardware | Nothing was powered, programmed or measured | Not done |
| B12 | Datasheet and part currency | The Amphenol 12401610E4#2A datasheet was not read (amphenol-cs.com returned 403), so its pinout, ratings and mechanical drawing are KiCad's footprint only; distributor listings give 5 A and 100 V and disagree between sites. CH342F V1E is the newest sheet found; the exposed pad is 2.6 mm in the KiCad footprint and 2.4 mm in the supplier drawing; LCSC numbers and stock were not checked | Not done |

Rows A4 and B1-B10 are the ones that decide whether this board meets a Class B bar (issue #38 treats missing SI/PI/RF evidence as a failure). They are open.
