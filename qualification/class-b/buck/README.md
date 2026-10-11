# 24 V in, 3.3 V out buck with input parts (PTC, reverse-block diode, TVS) (SYN-67)

Status: designed, placed, routed, DRC and ERC counted. Not fabricated, not powered, no hardware test.
All new parts are UNVERIFIED (imported, no reviewer). No reviewer has signed off this board.
Intended for 24 V +10% in and 1 A out. 1 A at 70 C is not shown (see Limits, thermal). Not an overvoltage-safe or 2 A design.

## Purpose

Ticket #67 under #38 (Class B qualification): a switching-regulator board with input parts, feedback, UVLO
and a power LED, plus a negative variant that the feedback-safety rule (E-SYNTH-POWER-011, PR #56) must flag.

## What

- `buck.synth`, board-mm sidecar `buck.placement.layout.toml`, negative `buck_neg_fb_sw.synth`, hashes in `artifacts.sha256`.
- Chain: J1 (JST XH) -> F1 PTC -> D1 SS36 Schottky (reverse block) -> VIN node.
  VIN node: D2 SMBJ26A TVS, C1 47 uF bulk, C2 10 uF, C3 100 nF (on the U1 VIN pin), U1 TPS54202, UVLO divider.
- U1 -> L1 10 uH -> C5, C6 (2x 22 uF, 25 V X7R) -> J2. C4 100 nF boot. D3 green LED + R_LED 470.
- Feedback: R_FB_T 100k from the output node (L1 pad 2 side), R_FB_B 22.1k, C7 56 pF across R_FB_T.
  Vout = 0.596 x (1 + 100k/22.1k) = 3.293 V typical, 3.21 to 3.376 V over Vref 0.581 to 0.611 V (ideal resistors).
- Negative file: identical except R_FB_T and C7 hang from SW. Besides E-SYNTH-POWER-011 it emits the same 14 warnings as the good board.
- Board: 63.0 x 37.6 mm (larger than a hand layout would need because of the SMC diode, the 8 x 10 mm electrolytic and the placer's 4 mm margin), 4 layers, GND pours on all layers. Power tracks 0.5 mm, signal 0.127 mm.

## UVLO (TPS54202 SLVSD26C section 6.3.5, Eq 1 and 2, Figure 6-1)

Datasheet values: Ip 0.7 uA, Ih 1.55 uA, VENrising 1.21 V (max 1.28), VENfalling 1.19 V (min 1.1). R4 = R_UV_T = 1.5M, R5 = R_UV_B = 100k.
- Start = VENr + R4 x (VENr/R5 - Ip) = 1.21 + 1.5M x (12.10 uA - 0.70 uA) = 18.31 V.
- Stop = VENf + R4 x (VENf/R5 - Ip - Ih) = 1.19 + 1.5M x (11.90 - 0.70 - 1.55 uA) = 15.67 V.
- Check: Eq 1 with these thresholds returns R4 = 1.5M and Eq 2 returns R5 = 100k.
- 1% resistors, typical thresholds: start 17.96 to 18.67 V, stop 15.35 to 15.99 V.
- 1% resistors plus the datasheet EN threshold limits (VENr up to 1.28, VENf down to 1.1; Ip and Ih typical, no tolerance given):
  start 17.96 to 19.81 V, stop 13.93 to 15.99 V.
- EN at 28 V in is 1.75 V (EN recommended max 5.5 V).

## Parts and datasheets (read with curl + pdftotext, 2026-10-11)

| Part | Datasheet | Revision | Values taken | Conflict or gap |
| --- | --- | --- | --- | --- |
| U1 TI TPS54202 (SOT-23-6 DDC) | https://www.ti.com/lit/ds/symlink/tps54202.pdf | SLVSD26C, Feb 2026 | VIN 4.5-28 V recommended, abs max 30 V; SW abs max 30 V; EN abs 7 V, rec 5.5 V; Vref 0.581/0.596/0.611 V; 500 kHz (390-630); 2 A; HS limit 2.5/3.2/3.9 A; pins 1 GND, 2 SW, 3 VIN, 4 FB, 5 EN, 6 BOOT; Table 7-2 at 3.3 V: L 10 uH, Cout 44 uF, R2 100k, R3 22.1k, C6 56 pF (based on VIN 28 V); CIN over 10 uF ceramic; boot 0.1 uF; RthetaJA 118.6 C/W (57.2 on the TI EVM); no power-good pin | KiCad symbol is TPS54202DDC. Pin numbers equal the TI table. R2/R3 in TI text are R_FB_T/R_FB_B here |
| L1 Bourns SRN6045TA-100M | https://www.bourns.com/docs/Product-Datasheets/SRN6045TA.pdf | REV. 06/24 (footer) | 10 uH +-20%, DCR 52 mOhm typ (+-20%), Irms 3.20 A typ, Isat 4.60 A typ (L -30%), 6.0 x 6.0 mm, land 6.5 x 5.1 mm | Registry has no Isat field; Isat is in the description |
| D1 Vishay SS36 | https://www.vishay.com/docs/88751/ss32.pdf | 23-Apr-2020 (doc 88751) | VRRM 60 V, 3 A, IFSM 100 A, VF 0.75 V max at 3 A, IR 0.5 mA at 25 C and 10 mA at 100 C, SMC | KiCad `Diode:SS36` defaults to SMA, this sheet is SMC (SMC used). VF at low current not read |
| D2 Vishay SMBJ26A | https://www.vishay.com/docs/88392/smbj.pdf | 09-Jan-2024 (doc 88392) | 600 W 10/1000 us, VWM 26 V, VBR 28.9-31.9 V at 1 mA, VC 42.1 V at 14.3 A, VBR temperature coefficient 0.097 %/C, SMB | No KiCad SMBJ26A symbol; Device:D_Zener used. Registry `max_voltage_v` 26 is VWM |
| F1 Bourns MF-MSMF110/33X | https://www.bourns.com/docs/Product-Datasheets/mfmsmf.pdf | REV. BE, 09/26 (footer) | Vmax 33 V, Imax 20 A, Ihold 1.1 A at 23 C, 0.93 A at 40 C, 0.73 A at 60 C, 0.63 A at 70 C, 0.50 A at 85 C, Itrip 2.2 A, R 0.06-0.20 Ohm | none found. Registry `max_current_ma` 1100 is Ihold at 23 C |
| J1, J2 JST B2B-XH-A | https://www.jst-mfg.com/product/pdf/eng/eXH.pdf | none printed | 2.5 mm pitch, 3 A, 250 V | Existing registry `jst_xh_2pin` says 5 V / 100 mA; conflicts with this sheet, left untouched, new part added |
| D3 Lite-On LTST-C190KGKT | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF | not checked | none (existing registry part) | Site unreachable (curl code 000) |
| C1 47 uF | none | | generic 8 x 10 mm SMD electrolytic, 50 V class | no MPN, ESR and ripple rating unknown |
| C2, C5, C6 (1206), C3, C4, C7 (0603), resistors | none | | generic registry parts; voltage and dielectric are set per instance in `buck.synth` (C2 50 V X7R, C3 50 V X7R, C4 16 V X7R, C5/C6 25 V X7R, C7 50 V C0G, resistors 1%) | no MPN. Murata and TDK pages returned 404/403/500: no DC-bias data |

## Limits and honesty notes

- Overvoltage: NEEDS EXPERT. TPS54202 abs max VIN is 30 V (recommended 28 V). SMBJ26A VBR is 28.9 to 31.9 V and rises 0.097 %/C
  (about 30.2 V minimum at 70 C, by calculation from the datasheet coefficient), VC is 42.1 V at 14.3 A. The TVS therefore clamps surge energy only; it gives no protection against a
  sustained 28 to 32 V rail, and a rated surge reaches 42 V at the TVS. SMBJ24A (VC 38.9 V) is also above 30 V. The board tolerates 24 V +10% (26.4 V, 0.4 V above the TVS standoff). A real
  24 V industrial rail needs a series OVP stage or surge stopper in front of U1, or a 40 to 60 V rated buck. Not designed here. The surge waveform of an application is not defined.
- PTC and rating: output current is rated 1 A, not 2 A. Input current = Pout / 0.85 / Vin. The 85% efficiency is an assumption, not a datasheet value.
  At 1 A out and the UVLO stop voltage 15.67 V: 3.3 / 0.85 / 15.67 = 0.248 A. F1 Ihold is 0.63 A at 70 C (2.5x margin) and 0.50 A at 85 C (2.0x).
  At the worst-case UVLO stop of 13.93 V: 3.3 / 0.85 / 13.93 = 0.279 A, margin 2.26x at 70 C and 1.79x at 85 C. SS36 drop and PTC self-heating are not included.
  At 2 A out the same input current is 0.50 A, no margin at 85 C. Ambient ceiling from the PTC alone: 70 C.
- Thermal: UNVERIFIED, and this estimate does not show 1 A at 70 C. Total loss at 1 A with the assumed 85% is 3.3 / 0.85 - 3.3 = 0.582 W (estimate). L1 DCR loss is 1 A^2 x 0.052 Ohm = 0.052 W,
  leaving 0.53 W in U1 if all of it is dissipated there. At RthetaJA 118.6 C/W that is +62.9 C: junction about 133 C at 70 C ambient, above the 125 C recommended limit; 125 C is reached at about 62 C ambient,
  below the PTC's 70 C ceiling. With the EVM figure 57.2 C/W it is +30.3 C, about 100 C at 70 C. Which figure applies depends on copper, which Synth does not model.
  The 70 C ceiling is the PTC ceiling only, not a thermal clearance for U1.
- Output capacitors: Table 7-2 asks for 44 uF at 3.3 V (2 x 22 uF, 25 V X7R 1206 on a generic registry part, voltage set per instance). Crossover fo = 3.95 / (Vout x COUT) = 27.3 kHz at 44 uF, staying under 40 kHz needs
  at least 30 uF effective (68% of 44 uF). DC-bias derating at 3.3 V is unknown. Crossover and stability are UNVERIFIED.
- Input capacitor: TI asks for over 10 uF ceramic. C2 is 10 uF X7R 50 V; its effective value at 24 V is unknown and may be well under 10 uF. C1 47 uF electrolytic adds bulk with unknown ESR. UNVERIFIED.
- Reverse polarity: Schottky chosen over a P-FET. SS36 VRRM 60 V against 26 V. Drop 0.75 V max at 3 A, so under 0.19 W at 0.248 A (bound; VF at low current not read). A P-FET at 24 V needs a gate clamp (typical Vgs max is +-20 V); no P-FET datasheet was read.
- Inductor: ripple at 24 V is 0.57 A pp; at 1 A out the peak is 1.29 A against Isat 4.6 A.

## Commands

```
export XDG_DATA_HOME=$(mktemp -d)   # empty dir: a user overlay in ~/.local/share/synth shadows registry parts
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
scripts/setup-routing-engines.sh --freerouting-only          # FreeRouting jar + Java, or symlink tools/freerouting and tools/jre25
B=$(pwd)/qualification/class-b/buck   # absolute path, set from the repo root before any cd
synth validate $B/buck.synth
synth route $B/buck.synth
synth export-kicad $B/buck.synth --out OUT --allow-unverified-parts --validate-erc
kicad-cli pcb drc --severity-all --refill-zones --format json -o OUT/drc.json OUT/buck.kicad_pcb
kicad-cli sch erc --severity-all --format json -o OUT/erc.json OUT/buck.kicad_sch
synth validate $B/buck_neg_fb_sw.synth
python3 -I $B/pad_gaps.py OUT/buck.kicad_pcb
```
Registry import: `synth part import-kicad Regulator_Switching:TPS54202DDC --id tps54202 --footprint Package_TO_SOT_SMD:SOT-23-6`;
likewise `Diode:SS36` (Diode_SMD:D_SMC), `Device:D_Zener` (SMBJ26A, Diode_SMD:D_SMB), `Device:Polyfuse`, `Device:L`, `Device:C_Polarized`, `Connector_Generic:Conn_01x02`.
The files were then edited by hand (names, MPN, dimensions, datasheet URL, `switch_node` on SW and `feedback` on FB as for `mp2307_buck`).

## Measured results

The export routes with FreeRouting 2.4.1; the routed board is written as `OUT/buck.kicad_pcb` when Synth's gate passes (state `routed`). Numbers below are from that file.

| Number | Value | Command |
| --- | --- | --- |
| validate, good board | exit 0, 0 errors, 14 warnings (8 part-unverified, 3 E-SYNTH-SCHEM-003 (schematic-sheet distance of a cap to U1, not PCB distance), 1 anomaly, 1 supply, 1 power symbol) | `synth validate $B/buck.synth 2>&1 \| grep -c '^warning'` and `grep -c '^error'` |
| validate, negative | exit 1, 1 error (E-SYNTH-POWER-011), same 14 warnings | same on `buck_neg_fb_sw.synth` |
| routing | `routed`, 10 nets connected, 0 open, 52 segments, 5 vias | `synth route $B/buck.synth` (JSON `state`, `statistics`); cross-check `grep -cP '^\t\(segment' OUT/buck.kicad_pcb`, `grep -cP '^\t\(via' ...` |
| Synth routing check | kicad_drc 0 errors, 0 unconnected, 38 warnings; `fabrication_ready` true (routing check only) | `synth route` JSON `validation` |
| DRC | 0 errors, 0 unconnected, 38 warnings: 20 lib_footprint_issues, 5 silk_overlap, 11 silk_over_copper, 2 silk_edge_clearance | `kicad-cli pcb drc ... -o OUT/drc.json`, then `python3 -I -c "import json,collections as c;d=json.load(open('OUT/drc.json'));print(c.Counter((v['severity'],v['type']) for v in d['violations']),len(d['unconnected_items']))"` |
| ERC | 0 errors, 54 warnings: 34 lib_symbol_issues, 20 footprint_link_issues (libraries not in the KiCad config) | `kicad-cli sch erc ... -o OUT/erc.json`, same one-liner over `sheets[].violations` |
| Outline | 63.0 x 37.6 mm (x -2.2 to 60.8, y -1.375 to 36.23) | `grep -A3 gr_line OUT/buck.kicad_pcb \| grep start` |
| Hashes | pcb 0ea0af61cb4a..., sch 47b7451740a8..., two exports identical | `cd OUT && grep -v '^#' "$B/artifacts.sha256" \| sha256sum -c -` (full values in `artifacts.sha256`) |

Pad-edge gaps in the placed, routed board, from `pad_gaps.py` (pcbnew pad bounding boxes, edge to edge):

| Pair | Gap mm |
| --- | --- |
| C3 (100 nF) to U1 VIN | 1.18 |
| C2 (10 uF) to U1 VIN | 4.34 |
| C3 / C2 ground pad to U1 GND | 3.08 / 5.15 |
| C4 to U1 BOOT / SW | 2.77 / 3.75 |
| L1 pad 1 to U1 SW | 1.95 |
| C5, C6 to L1 pad 2 | 1.20 |
| D2 TVS to U1 VIN | 9.43 |
| R_FB_T / R_FB_B to U1 FB | 1.98 / 3.73 |
| C7 to R_FB_T | 0.75 |
| R_FB_B ground pad to U1 GND | 4.25 |
| R_FB_T FB pad to L1 pad 1 (SW) | 7.53 |

The sidecar holds the placed positions (checked against the file; only the JST connectors differ, by their fixed anchor offset of (-1.25, -0.525) mm).
Synth moves a sidecar position that collides with a neighbour's courtyard plus margin, so `pad_gaps.py` on the final routed file is the reference, not the sidecar coordinates.

## Negative case: feedback from the switch node

`synth validate $B/buck_neg_fb_sw.synth` exits 1 (JSON, schema 1.3):
```
error: [E-SYNTH-POWER-011] regulator feedback is taken from the switching node
  found:    feedback pin `U1.fb` (group "BUCK") is joined through resistor `R_FB_T` to switch-node pin `U1.sw` (group "BUCK")
  expected: the feedback pin to sense the regulated output, not the switching node
```
`synth export-kicad` stops with "Synth ERC validation failed; use --force to export anyway" (exit 2). The good board does not raise it. The rule follows one resistor only.

## Qualification matrix

| Claim | Evidence command | Result | Status |
| --- | --- | --- | --- |
| Source validates, no blocking diagnostic | `synth validate buck.synth` | exit 0, 0 errors | VERIFIED |
| Feedback taken from the output, not SW | same, E-SYNTH-POWER-011 | not raised. The rule follows one resistor only, so longer paths are not checked | VERIFIED (with that caveat) |
| Feedback from SW is flagged | `synth validate buck_neg_fb_sw.synth` | E-SYNTH-POWER-011, exit 1 | VERIFIED |
| Routed, no open nets | `synth route` | routed, 0 open | VERIFIED |
| KiCad DRC | `kicad-cli pcb drc` | 0 errors, 0 unconnected, 38 warnings | VERIFIED |
| KiCad ERC | `kicad-cli sch erc` | 0 errors, 54 library warnings | VERIFIED |
| Deterministic export | two exports, sha256 | identical | VERIFIED |
| Part pins and footprints | `synth registry qualify --part <id>` | pin/pad coverage passes; E-SYNTH-QUAL-008 (no reviewer) blocks. The stock KiCad `Package_TO_SOT_SMD:SOT-23-6` footprint was NOT compared with the TI DDC package drawing | UNVERIFIED |
| Part values vs manufacturer sheets | curl + pdftotext | read, listed above | UNVERIFIED (one reader, no reviewer) |
| Survives a rated surge or sustained 28-32 V | datasheet numbers above | VC 42.1 V and VBR 28.9 V against abs max 30 V | NEEDS EXPERT |
| Reverse polarity blocked | SS36 VRRM 60 V | datasheet number only | UNVERIFIED |
| PTC margin at 1 A out | Ihold table | 0.248 A vs 0.63 A at 70 C, by calculation with an assumed 85% | UNVERIFIED |
| UVLO thresholds | Eq 1 and 2 | 18.31 / 15.67 V typical, ranges above | UNVERIFIED (calculation) |
| Output voltage | Eq 7 | 3.293 V typical | UNVERIFIED (calculation) |
| Output cap value, crossover, stability | Table 7-2, fo equation | 27.3 kHz at 44 uF nominal, derating unknown | UNVERIFIED |
| Input cap effective value at 24 V | none | vendor data not obtained | UNVERIFIED |
| Power-good | none | TPS54202 has no PG pin | UNSUPPORTED |
| Thermal copper, thermal vias, junction temperature | loss estimate, datasheet RthetaJA | Synth has no thermal-pour or via model. Estimated junction about 133 C at 70 C ambient with 118.6 C/W (125 C at about 62 C ambient); about 100 C with 57.2 C/W | UNVERIFIED |
| Loop area, SW node, return paths | gap table | pad gaps in mm, no loop area or inductance | UNVERIFIED |
| EMI, ripple, efficiency | none | not simulated or measured | UNVERIFIED |
| Independent mixed-signal / RF sign-off (#38) | none | no reviewer | NOT DONE |

## Not true yet

- Not fabricable on this evidence: parts UNVERIFIED, overvoltage case open, nothing built.
- Layout: the +3V3 sense path to R_FB_T was routed by FreeRouting on B.Cu (0.5 mm), crossing under the buck core above the inner ground planes; not hand-tuned. FB is 0.127 mm and 8.3 mm long.
  C3 sits on the VIN pin and C7 against R_FB_T, but R_FB_B's ground is 4.25 mm from U1 pin 1: pin 1 and pin 4 are on opposite sides of the SOT-23-6, so the ground return goes through a via to the plane.
- Schematic (rendered and read): readable, but value text overlaps (TPS54202DDCR on the GND symbol, 47uF on C1's wire), the BUCK title crowds U1, and a wire loops over the top of J1/F1.
  Synth labels signal nets from pin names, so +3V3 is drawn as `P2` and LED_PWR as `P2_R_LED`; the schematic netlist names (`/P2`, `Net-(D1-A)`) differ from the PCB names (`+3V3`, `LED_PWR`). No schematic sidecar is used (default layout).
- Silkscreen references are large and offset (5 overlaps, 11 over copper, 2 over the edge).
- Rotations are 0 and 180 only. With 90/270 degree parts, Synth's connectivity gate reported 5 open nets while kicad-cli reported none, and `synth route` delivered the unrouted baseline. Not reduced to a minimal repro (a two-resistor board routes clean), cause not found.
- A GND pour island on F.Cu fails Synth's gate in some placements; the final placement avoids it, others may not. Open PR #108 (join cut-off ground pour pieces) and PR #109 (rotated pad positions) address this and the 90/270-degree open-net finding, so the hashes may change after they merge.
- Routing used FreeRouting defaults, no length or coupling constraints.

## Not verified

- Nothing powered, measured or thermally tested. Efficiency (85%) and the 0.58 W loss are assumptions.
- The MCP placement tool `synth_place_with_hints` reports a stale outline (57.15 x 55.51 mm, no tightening after the sidecar) and a matching "unused area" finding, so no placement-review result is claimed.
- No reviewer. Availability, LCSC numbers and assembly not checked.
- LED datasheet unreachable. J1/J2, C1, and all generic passives have no MPN or manufacturer sheet.
- PTC behaviour with the TVS in a surge, and inrush into C1 and the output caps, are not analysed.
- Hash reproducibility on another machine depends on FreeRouting 2.4.1 and the Java build.
