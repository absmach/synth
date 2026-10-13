# Qualification matrix: USB-C dual UART + RS-485 adapter (SYN-66)

**Status: requires expert review. This is not a completed qualification result.**
Nothing here was fabricated or powered. Section A lists what Synth or KiCad checked and the evidence. Section B lists what no tool here proves and an SI/PI/RF or manufacturing expert has to sign off. No expert has reviewed this board.

Revision B routes with 0 open nets but does not pass Synth's release gate (2 unconnected ground-pour items, see A5), so the package is marked UNTRUSTED in `reports/release.json`.

## Reproduce

Run from the repository root. Use Synth `main` from 2026-10-13 or later: it includes PR #139 (fabrication note, merged as `de4a5b2`) and PR #140 (RS-485 checks, merged as `bb9ae95`). The delivered board's reference-label positions also need PR #144 (label placement, not merged yet); without it the same layout has label-overlap and label-clipped warnings (see A3). Use an empty `XDG_DATA_HOME` so a user registry cannot shadow parts.

```
export XDG_DATA_HOME=$(mktemp -d)
scripts/setup-routing-engines.sh
synth validate qualification/class-b/usb-uart/usb_uart.synth
synth export-kicad qualification/class-b/usb-uart/usb_uart.synth --out <dir> --router free-routing --krt-allow-via-in-pad
kicad-cli pcb drc --severity-all --refill-zones --format json <dir>/usb_uart.freerouting.kicad_pcb
kicad-cli sch erc --severity-all --format json <dir>/usb_uart.kicad_sch
(cd qualification/class-b/usb-uart && sha256sum -c SHA256SUMS)
```

Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.4.1, Synth `main` bb9ae95 (includes PR #139 and PR #140) plus the label placement of PR #144 (not merged), on 2026-10-13. Routing is not bit-for-bit repeatable here: small placement changes gave different results (see A5).

## Artifact hashes

`SHA256SUMS` in this folder lists every design artifact (source, sidecars, KiCad files, BOM, 3D render, routing and DRC reports). It leaves out `README.md`, `QUALIFICATION.md` and itself. The key ones:

| Artifact | SHA-256 |
| --- | --- |
| `usb_uart.synth` | `60bb2485baad72ba6c4cbcc9a32580ddd7c32428df4374a3ae69d216f9b139ca` |
| `usb_uart.placement.layout.toml` | `a6b9071b08505d7b3cdd9314f9fb8d118fec1a7a84433e24494e7f6f26c94131` |
| `kicad/usb_uart.kicad_pcb` | `276022fb8894e65dcdfac26cc21c16d76b13b5fc0ae4f3af41d1d5b563cdd5cd` |
| `kicad/usb_uart.kicad_sch` | `4792bbc7e34f1f09e000ea9b24ba26f1ad404d6a69aa9fc29370ec415857e1b5` |
| `kicad/bom.csv` | `e5bd8b574e18c867a3f9b8c625cbdd0f64f2e31033f26620578ac2fb59949cef` |
| `reports/release.json` | `18bf8be60405541b32b4fa7038984b13e38ced616c88ad3ad2397daded30c29b` |
| `reports/usb_uart.drc.json` | `3fa3789f1279cfadd219dddc0f982e0c84ea0bb90957bab468c648608536df50` |

`reports/` holds the files as captured, so they contain the absolute paths of the machine that produced them, and `engine_version` in `release.json` is a jar path.

Datasheet files read for this revision (not committed, so the figures taken from them are not checkable from this repository): Bourns CDSOT23-SM712 REV. 06/24, `75c1a765dbea8b5d7e57611b8787ce6b2856e57eef387b334bc0a894d91a19d2` (VWM 12 V / 7 V, clamp 19 V at 1 A, 26 V at 17 A, 400 W / 17 A); TI THVD1450 SLLSEY3E (May 2018, revised May 2019) (bus abs max +/-18 V in 7.1, surge circuit in fig. 37 and table 7). The JLCPCB statement in B8 comes from JLCPCB's own pages as read on 2026-10-12.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The source parses, resolves and lowers | `synth validate` | No errors. Warnings only (the `E-` ones are also severity warning): E-SYNTH-POWER-004 x2, W-SYNTH-ANOMALY-001, W-SYNTH-IMPEDANCE-001, W-SYNTH-SUPPLY-002 x5, W-SYNTH-PART-UNVERIFIED x10 |
| A2 | Every net is connected end to end | Synth connectivity re-derived from copper, `reports/usb_uart.connectivity.json` | 29 nets connected, 0 open, 0 unconnected pads |
| A3 | No KiCad DRC rule violation on the routed board | `kicad-cli pcb drc --refill-zones` on the raw router file, `reports/usb_uart.drc.json` | 0 errors, 51 warnings (36 library links, 15 silk over copper; no label overlap and no label clipped by the board edge, which needs PR #144). **Caveat:** after a KiCad re-save the same copper shows 63 clearance errors (revision A shows 59); see B9 |
| A4 | The RS-485 line is protected and the terminator is switchable | `E-SYNTH-ESD-001` and `E-SYNTH-RS485-001` (PR #140) | Revision A: 2 + 1 warnings. Revision B: none. **Weak evidence:** both rules follow only the same net, and R_A/R_B split the nets, so on revision B the rules cannot see the bus side. This was confirmed by reading the schematic only |
| A5 | The package passes Synth's release gate | `synth export-kicad`, `reports/release.json` | **No.** `validation_failed`: KiCad reports 2 unconnected ground-pour items. Seen in every route that reached 0 open nets (4 of 4). Many other placements left nets open or timed out; the aligned header row only routed after J4 was turned 180 degrees and D8 moved beside it |
| A6 | Schematic electrical rules | `kicad-cli sch erc` | 1 error (`power_pin_not_driven` on the VIO symbol, jumper-selected rail, also on revision A), 117 warnings |
| A7 | Via-in-pad sites are found and recorded | Synth connectivity `via_in_pad`; PR #139 fabrication note | One site: U1 exposed pad (GND). The note is only written for a routed board, so this run's `release.json` has none |
| A8 | The three parts added or changed here match their KiCad footprints | `synth registry qualify` | `header_1x2`, `cdsot23_sm712` and `thvd1450d` have no pin-to-pad, mirrored-footprint or dimension finding. Each has one blocking finding, `E-SYNTH-QUAL-008` (no reviewer or provenance). Like every part that command listed in my run (all of them blocked), they stay `blocked` until a reviewer is named, and none has one |

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | USB D+/D- is a 90 ohm differential pair | Declared (`impedance 90ohm`). Synth gives only a closed-form estimate and says it does not verify inner layers (W-SYNTH-IMPEDANCE-001). The 4-layer stackup and the fab's real stackup are unverified. FreeRouting does not couple the pair. Revision A: width 0.242 mm, closest gap 0.133 mm | Requires expert review |
| B2 | USB return path and layer transitions | D- changes layer through vias (revision A: 2). Whether a ground return via sits beside each signal via was not analysed | Requires expert review |
| B3 | USB coupling and skew | Revision A skew 1.40 mm, 12.6 mm of D+ within 0.5 mm of D-. Not re-measured for revision B. No simulation | Requires expert review |
| B4 | RS-485 surge protection (IEC 61000-4-5) | Topology follows TI's 1 kV circuit (SLLSEY3E fig. 37): 10 ohm series, TVS on the bus side. R_A and R_B are generic 0603; TI lists pulse-proof parts. THVD1450 bus abs max is +/-18 V and the SM712 clamps at 19 V (1 A) to 26 V (17 A), so the series resistors matter. Not surge-tested, no expert approval | Requires expert review |
| B5 | RS-485 common-mode range, bias and fail-safe | Relies on the THVD1450's own fail-safe (per SLLSEY3E). No bias network, no common-mode choke. SM712 stand-off is 12 V / 7 V, matching the +12 / -7 V range, from the Bourns datasheet only | Requires expert review |
| B6 | RS-485 termination | Selectable through J_TERM. With no jumper the bus is unterminated. Correct jumper use depends on where the board sits on the bus. Bus-side layout and stub lengths were not analysed | Requires expert review |
| B7 | Power integrity | LDO, VBUS and VIO rails, decoupling placement and loop areas were not analysed. No VBUS bulk capacitor, fuse, reverse or inrush protection | Requires expert review |
| B8 | Via-in-pad fabrication | The U1 exposed-pad via (0.6 mm pad / 0.3 mm drill) needs a filled and capped via. JLCPCB lists it as a paid option on 4-layer boards. The order must ask for it. **Acceptance by the manufacturer is not confirmed for this board** | Requires manufacturer confirmation |
| B9 | Clearance at the 0.5 mm-pitch J1 pads | The raw router file passes DRC; a KiCad re-save shows 63 clearance errors against the 0.2 mm default (59 on revision A). Which view is right was not determined | Requires review |
| B10 | Thermal and EMC | Not analysed | Requires expert review |
| B11 | Function on real hardware | Nothing was powered, programmed or measured | Not done |
| B12 | Datasheet and part currency | CH342F V1E is the newest sheet found; the exposed pad is 2.6 mm in the KiCad footprint and 2.4 mm in the supplier drawing; LCSC numbers and stock were not checked | Not done |

Rows A4 and B1-B10 are the ones that decide whether this board meets a Class B bar (issue #38 treats missing SI/PI/RF evidence as a failure). They are open.
