# Verification report — protected_power_entry rev A

Scope: what was actually run in this session, what it returned, and what was not run. No shell was available, so
`kicad-cli` (ERC/DRC/plot) was not run independently. The DRC figures below are the ones the Synth router
pipeline reports after it runs `kicad-cli pcb drc` itself.

## 1. Runs

| Check | Tool | Result |
|---|---|---|
| Compile + readability | `synth_validate` | 0 errors, 50 warnings first run (5 refdes-naming warnings fixed by renaming DZn to Dn) |
| ERC | `synth_erc_report` | `erc_clean: true`, `error_count: 0`, 34 warnings (read in full; all classes listed in section 3) |
| Placement review | `synth_describe_placement` with the sidecar | score 100, `requires_revision: false`, no findings (the auto-placer alone scored 80 and produced a 120 x 90 mm scatter, so the sidecar is used) |
| Route | `synth_route`, `synth_export` (kicad-routing-tools) | final export: `state: routed`, 34/34 nets connected, 0 open nets, 426 segments, 32 vias, 978 mm of copper, 0 via-in-pad |
| DRC | router pipeline `kicad-cli pcb drc` | **errors 0, unconnected 0, warnings 0**; `fabrication_ready: true`; `release_ready: true`, `route_complete: true` |
| Schematic | `synth_review_schematic` (rendered and looked at) | A1 sheet, seven group boxes, none overlap, nothing outside the frame; many empty areas (sheet is much larger than the content); 6 inverted power-symbol warnings (E-SYNTH-SCHEM-001), 4 decoupling-distance warnings (E-SYNTH-SCHEM-003), 1 long-wire warning (E-SYNTH-SCHEM-004), 1 info (no notes on TEST POINTS) |
| BOM | `export/bom.csv` read | 85 lines incl. header = 84 parts; every schematic part is in it |
| Supply chain | `synth_live_supply_chain` | **failed** (`ambiguous LCSC search for SMAJ15A (30 candidates)`); no stock or lifecycle data was obtained |

"Zero errors" is the tool's statement; it is not independent. It does not prove the circuit is right. The schematic was reviewed by hand in `DESIGN_NOTES.md`.

## 2. Defects found in Synth and the registry (reported exactly)
1. **Silent placeholder footprint.** `mp2307_buck` names `Package_SO:SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.4x3.4mm`, which does not exist in the KiCad library. The exporter did not stop or warn; it generated a procedural footprint with pads on a 3.6 mm grid (pad 8 at (2.91, 3.6), pad 9 at (2.91, 10.8) local), so the first routed board had shorts between U1 and L1 pads. `synth_validate`, `synth_erc_report` and `synth_describe_placement` all stayed quiet; only `synth_resolve_footprints` and the router DRC showed it. `synth_resolve_footprints` correctly refused to pick between three EP variants (`EP2.41x3.3`, `EP2.41x3.81`, `EP2.514x3.2`); I chose `EP2.41x3.3mm` by hand.
2. **Registry part lacks the exposed pad.** `mp2307_buck` has 8 pins; the SOIC-8-1EP footprint has pad 9. Left as is, pad 9 would be an unconnected copper pad. I authored `mp2307_buck_ep` with pad 9 = GND.
3. **TPS22918 pin map.** `tps22918_loadswitch` has 5 pins on a 6-pad SOT-23-6 and puts VOUT on pad 5; it lacks QOD. I authored `tps22918_sot23_6` from memory of the datasheet (VOUT pad 6, QOD pad 5). Unverified; the two parts disagree and one of them is wrong.
4. **Registry descriptions wrong.** `jst_xh_2pin` says "2.0mm pitch" (it is 2.50 mm); `d_schottky` is described as "SS14-class", has MPN 1N5819 and LCSC C8224, and uses a SOD-123 footprint (identity inconsistent).
5. **Inductors too small.** The shipped 6.8 uH / 10 uH 1210 inductors are small signal-class parts; none is suitable for 2.5 A peak.
6. **Net-class widths not honoured.** All power classes were declared in the source and appear in the PCB header with every net assigned, but the router narrowed copper (section 4) and used 0.6/0.3 mm vias where the classes declare 0.8/0.4 mm. The router's `narrowed` list is its own accounting; it does not list GND at all although GND stubs were 0.2 mm in an earlier export. `engine_policy_verdict` is "rejected" in the stats while the pipeline reports success and `release_ready: true`.
7. **Stacked duplicate copper.** The router reports duplicate GND segments stacked on the same coordinates (stacked_copper list); harmless to DRC.
8. **Net numbering mismatch.** Signal-integrity findings number nets from 0 while the `.kicad_pcb` numbers them from 1.
9. **`plane_fragility`.** `refill_status: no_kicad_python`, so GND zone islands were estimated from zone outlines (0 islands), not from a refilled zone. `islands: 0` is not a filled-copper check.
10. **No way to place silkscreen text**, no Gerber/drill/pick-and-place writer in `synth_export`.
11. **Supply-chain tool** fails on an ambiguous LCSC search instead of using the registry LCSC number.

## 3. Warnings accepted
| Code | Where | Reason accepted |
|---|---|---|
| W-SYNTH-SUPPLY-002 (no MPN/LCSC) | C1, C10, C17, J2, D2, D3, D6, TP1-10, H1-4 (passives are generic registry parts) | no identity exists in the registry; I did not invent numbers (listed in `ASSEMBLY.md`) |
| W-SYNTH-PART-UNVERIFIED | D1 (SMAJ15A), U1, L1, U3, J2, D11-13, D4, D7 | parts are seed/authored without a reviewer; **a human must review before fab** |
| W-SYNTH-THERMAL-001 | U2 107 C | Synth assumes 0.8 A; the real defended limit is 0.35 A (DESIGN_NOTES 3.6). The underlying concern is real and open |
| E-SYNTH-POWER-004 | +5V_SW one decoupling capacitor | C12 10 uF on U3 input and C14 22 uF on the output are present; the heuristic counts one on the net |
| W-SYNTH-ANOMALY-001 | whole design | statistical model; no physical meaning |
| E-SYNTH-SCHEM-001, -003, -004, -015 | schematic layout | cosmetic. SCHEM-003 measures schematic-group distance, not PCB distance (on the PCB C8/C9 are 17-20 mm from U2 and C12 is 20 mm) |
| W-SYNTH-DRC-013/016 | final export, 8 findings | see `DESIGN_NOTES.md` section 7; the +12V_PROT via at (60.1, 33.9) is not accepted as good, only unfixed |

## 4. Trace-width and via check (net classes)
Source of truth: the exported `.kicad_pcb` header (classes and assignment) and the router's `krt-stats.json` (`design_rules.narrowed`).
- Classes present and assigned: PWR12 0.8 mm (VIN_12V_RAW, VIN_12V_FUSED, +12V_PROT), PWR5 1.0 mm (BUCK_SW, OUT_5V, OUT_5V_SW, +5V, +5V_SW), PWR3 0.5 mm (+3V3), PGND 0.8 mm (GND), SIG 0.25 mm (23 signal nets). **Confirmed in the final file.**
- Narrowing reported by the router in the final export (22 events): +5V 0.70-0.90 mm; +12V_PROT 0.664 mm; VIN_12V_RAW 0.726 mm; BUCK_SW 0.85, 0.98 mm; +5V_SW 0.80 mm; SENSE_12V and SENSE_5V 0.2 mm. All power cases are "terminal neck" (pad-adjacent). VIN_12V_FUSED, +3V3, OUT_5V and OUT_5V_SW were not narrowed.
- I read the full segment list of an earlier export (same netlist, earlier placement): main runs of +5V, +5V_SW, OUT_5V and BUCK_SW were 1.0 mm; +12V_PROT 0.8 mm; with tapered necks and with 0.2 mm branch stubs for the LED and divider taps; GND stubs 0.2 mm; every via 0.6/0.3 mm. The final placement moved the LEDs, so those stubs changed. I did **not** re-read the final segment list.
- Verdict: classes declared and assigned = yes; applied everywhere = **no** (necks narrowed to as little as 0.66 mm on power nets; vias smaller than declared; GND stubs not at 0.8 mm). The necks are short and the current margins in `DESIGN_NOTES.md` 3.7 (IPC-2221: 0.42 mm for 1.3 A, 0.78 mm for 2 A) are met at the narrowest power neck (0.66 mm at 1.0 A, not at 2 A). A 0.3 mm via is about 1 A [estimate]; the +12V_PROT via beside U1 carries the buck input current and is the weak point.

## 5. Not verified
- Any datasheet value (all [recalled]); the authored parts' pin maps; U1 EP size; L1 ratings; BZT52C3V3 pinout (pad 1 = cathode assumed, SOD-123 standard); electrolytic polarity of pad 1; SMAJ15A pad polarity.
- Rendered PCB (no tool); connector orientation and test-point accessibility by picture; GND pour continuity after refill; copper area under U1/U2.
- Hardware: nothing was built or measured.
- Gerber, drill, pick-and-place: not produced.

## 6. Manufacturing readiness
**Not claimed.** The tool says `fabrication_ready: true` and DRC is clean, but the unresolved power-stage items (TVS clamp vs MP2307 limit, Q1 reverse-hot-swap Vds, U2 thermals, unverified TPS22918 pin map and EP footprint, 0.6 mm vias on power) and the unreviewed authored parts mean a human review is required first. This is a design candidate for review, not a release.
