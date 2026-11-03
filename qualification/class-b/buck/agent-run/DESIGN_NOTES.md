Design notes reconstructed from the committed files. The board was built by an agent; `prompt.md` is the cleaned brief it was given (same text as the website showcase), not the exact text that was run. No tool-call log or report was saved.

# buck rev A: design notes (24 V in, 3.3 V out, 1 A)

Sources: `../buck.synth`, `../buck.placement.layout.toml`, `../README.md`, `../QUALIFICATION.md`, `../kicad/`, `../reports/`, and the registry part files under `registry/parts/`. Ticket SYN-67 under #38 (Class B). Anything not recorded in those files is written "not verified". No datasheet was opened for this reconstruction; datasheet values below are the ones the board's README/QUALIFICATION record, with the datasheet URL.

Status as recorded in `QUALIFICATION.md`: "requires expert review. This is not a completed qualification result." Not fabricated, not powered, no hardware test.

## 1. Architecture

```
J1 (JST XH 2p) -> VIN_RAW -> F1 PTC 1.1 A -> VIN_FUSED -> D1 SS36 (reverse block) -> VIN
VIN: D2 SMBJ30A (TVS to GND), C1 47 uF, C2 10 uF, C3 220 nF, U1 VIN (pin 3), R_UV_T (UVLO divider top)
U1 LMR38010 (SW pin 8) -> SW -> L1 6.8 uH -> +3V3 -> C5, C6, C9 (3 x 22 uF) -> J2 pin 1
FB divider R_FB_T 100k / R_FB_B 43.2k tapped from +3V3 (L1 pad 2 side), C7 56 pF across R_FB_T
PG (U1 pin 6, open drain) -> R_PG 100k to +3V3, J2 pin 3, D4 fault LED
```

Group layout in the source: INPUT PROTECTION (J1, F1, D1, D2), INPUT FILTER (C1, C2, C3), UVLO (R_UV_T, R_UV_B), BUCK (U1, C4, R_RT, L1, C5, C6, C9), FEEDBACK (R_FB_T, R_FB_B, C7), POWER GOOD (R_PG, D4, R_PGLED), OUTPUT (J2, D3, R_LED), MECHANICAL (H1, H2). 27 components, 13 nets.

## 2. Requirements and assumptions (from the source and README)

| Item | Value | Where recorded |
|---|---|---|
| Input | 24 V +10 % (26.4 V max) | `buck.synth` header, README |
| Output | 3.3 V, rated 1 A (the converter's limit) | README |
| Switching frequency | 800 kHz from R_RT 33k | `buck.synth` notes, README |
| Efficiency | 85 % is an assumption | README "Limits" |
| Surge | SMBJ30A clamp 48.4 V at 12.4 A; no surge test run; no IEC 61000-4-5 rating claimed | README, QUALIFICATION B1 |

## 3. Design decisions recorded in the files

1. **LMR38010 instead of LMR33640.** The first revision used the LMR33640 (38 V abs max). A review found the SMBJ30A clamps at 48.4 V, above that maximum. LMR38010 is 85 V abs max (SW 85 V). The change removed the VCC pin and capacitor, moved PG to pin 6 and VIN to pin 3, and added R_RT (README).
2. **Reverse-polarity block is a Schottky, not a P-FET.** SS36 VRRM 60 V against 26 V; 0.75 V max drop at 3 A, so under 0.13 W at 0.162 A (a bound; VF at low current not read). A P-FET at 24 V needs a gate clamp and no P-FET datasheet was read (README).
3. **TVS after D1, directly on VIN.** D2 sits on the VIN node so its 48.4 V clamp is below U1's 85 V absolute maximum (source notes).
4. **UVLO divider.** R_UV_T 1.37M / R_UV_B 100k, ratio 14.7. Start 18.4 V typ (16.2 to 20.6), stop 16.2 V typ (14.0 to 17.9). EN at 28 V in is 1.90 V (EN abs max is VIN + 0.3 V) (README, from LMR38010 SNVSBM3B 9.2.2.8, Eq 13 and 14).
5. **Feedback.** Vout = 1.0 V x (1 + 100k/43.2k) = 3.315 V typ, 3.26 to 3.36 V over Vref 0.985 to 1.015 V with ideal resistors. The divider is tapped from the output node, never from SW; the negative twin `buck_neg_fb_sw.synth` hangs R_FB_T and C7 from SW and must raise E-SYNTH-POWER-011. C7 56 pF feed-forward (datasheet Eq 9 gives about 60 pF per the source note).
6. **Inductor.** L1 Bourns SRN6045TA-6R8M 6.8 uH. Ripple 0.53 A pp at 26.4 V in and 800 kHz; Eq 11 minimum 1.03 uH; peak about 1.27 A at 1 A out against Isat 5.7 A.
7. **Output capacitors.** 3 x 22 uF 25 V X7R 1206. Datasheet Table 9-1 has no 3.3 V row (its 5 V rows use 2 to 3 x 22 uF). DC-bias derating at 3.3 V unknown; stability unverified.
8. **Input capacitors.** Datasheet 9.2.2.6 asks at least 4.7 uF plus 100-220 nF; C2 10 uF X7R 50 V, C3 220 nF X7R 50 V, C1 47 uF 50 V electrolytic. Effective C2 value at 24 V unknown.
9. **Power good.** Open drain, 100k pull-up to +3V3, brought to J2 pin 3. D4 "PG FAULT" green LED with R_PGLED 470 from +3V3 to PG: lit while PG is low.
10. **Thermal.** U1 exposed pad (pin 9, 2.41 x 3.1 mm) on the F.Cu GND pour with a 2 x 3 array of six GND vias (0.6 mm pad, 0.3 mm drill, 1.2 mm apart), declared as `[[vias]]` in the sidecar. TI asks for at least 4 x 3 (9.5.1.1), so six is fewer. `manufacturer "jlc-filled-via"` and `requires via_in_pad`.
11. **Placement.** Hand-pinned sidecar, all 27 parts `priority = "hard"`. The unassisted auto placement left C3 19.8 mm from VIN, output caps 12.9 to 19.3 mm from L1 and 8 W-SYNTH-DRC-016 warnings (a reviewer's finding). Pad-edge gaps are in `QUALIFICATION.md`.

## 4. Board

4 layers, outline 70.99 x 33.99 mm (2413 mm2, x 0.55 to 71.54, y 0.55 to 34.54), GND pours on all layers, power tracks 0.5 mm (netclass PWR: trace 0.5 mm, clearance 0.2 mm), signal 0.127 mm, 97 segments all on F.Cu, 21 vias all GND, two M2.5 NPTH holes in opposite corners. Routed by FreeRouting 2.4.1, KiCad 10.0.7, Synth 0.1.1, 2026-10-29.

## 5. Open items and known limits (all recorded in the committed files)

- Nothing powered, measured or thermally tested; efficiency 85 % and loss 0.58 W are assumptions.
- Junction temperature not estimated: LMR38010 RthetaJA 42.9 C/W is "not valid for design" per the datasheet text and there is no copper-area curve.
- Current-loop area, return currents, EMI, ripple, loop stability and load transient: not analysed.
- R_PG is 22.63 mm from the PG pin; R_FB_B's ground is 11.31 mm from U1 GND; R_FB_T's FB pad is 2.58 mm from L1 pad 1 (SW).
- Via-in-pad with filled and capped vias: not confirmed with a fab.
- Silkscreen: "PG FAULT" and "PWR" values are on F.Fab only; silk shows D3, D4 references.
- No comparison with a human reference design (#68). No independent mixed-signal/RF reviewer (#38).
- Sustained overvoltage is limited by F1 (Vmax 33 V) and the 50 V capacitors (1.6 V margin at the 48.4 V clamp). Not tested.

- Source inconsistency: the `buck.synth` header comment says "There are no vias in the pad", but the placement sidecar has six `[[vias]]` (QUALIFICATION and the sidecar agree). This document follows the sidecar.

## 6. Not verified (for this reconstruction)

- Every datasheet figure above is as recorded in README/QUALIFICATION; the datasheets are not committed and were not re-read.
- Hardware behaviour of any kind.
- The generic passives (C2 to C7, C9, all resistors) and the mounting holes have no MPN, no datasheet and no reviewer record.
