> **Retrospective documentation reconstruction, not reproducible prompt-to-PCB evidence.** The exact prompt, ordered tool calls, model and tool versions, and export session report were not saved. This board is **not Class-B qualified** and is outside the qualification set: `synth check --fab` fails, thermal and via-in-pad, sourcing, DFM and independent sign-off are open (see `VERIFICATION_REPORT.md`).

# Verification report: buck rev A

Verdicts copied from the committed reports and `../QUALIFICATION.md`. Nothing was rerun for this document. Tools recorded: KiCad 10.0.7, FreeRouting 2.4.1, Synth 0.1.1, 2026-10-29 (`../reports/buck.routing.json`, `../reports/buck.kicad-drc.json`).

**Overall status as recorded in `QUALIFICATION.md`: "requires expert review. This is not a completed qualification result." No expert has reviewed this board. Nothing was fabricated or powered.**

## Gates that passed

| Gate | Verdict | File |
|---|---|---|
| `synth validate buck.synth` (A1) | exit 0, 0 errors, 14 warnings (3 W-SYNTH-PART-UNVERIFIED, 3 E-SYNTH-SCHEM-003, 1 E-SYNTH-SCHEM-004, 4 E-SYNTH-SCHEM-005, 1 W-SYNTH-ANOMALY-001, 2 W-SYNTH-SUPPLY-002) | QUALIFICATION.md, README.md (not a committed report) |
| Negative twin (A2) | `buck_neg_fb_sw.synth` exits 1 with E-SYNTH-POWER-011; good board does not raise it | QUALIFICATION.md |
| Connectivity (A3) | 13 nets connected, 0 open, 0 unconnected pads, 97 segments, 21 vias, 200.08 mm; `fabrication_ready: true`; `blocking_reasons: []` (FreeRouting copper only; the `--fab` check in A18 fails and overrides this for qualification) | `reports/buck.connectivity.json`, `reports/buck.routing.json` |
| KiCad DRC (A4) | 0 violations, 0 unconnected, 0 schematic-parity items; KiCad 10.0.7, 2026-10-29T13:51:01, severities error/warning/exclusion | `reports/buck.kicad-drc.json` |
| KiCad ERC (A7) | 0 violations on the sheet | `reports/buck.kicad-erc.json` |
| Release gate, no fab (A5) | `release_ready: true`, `production_status: production`, state `routed`, signal integrity 0 blocking / 0 warnings / no findings | `reports/release.json` |
| Release gate, fab package (A6) | `release_ready: true`, `production_status: production`, `fab_requested: true`, state `routed`, signal integrity 0 findings; reviewer_state lists `lmr38010` and `ltst_c190kgkt` as reviewed (Steve Munene, 2026-10-29; the author, not an independent reviewer) | `reports/release-fab.json` |
| Netlist vs source (A8) | 13 nets, 27 components, node for node | QUALIFICATION.md |
| Registry qualify (B8) | 8 of 8 registry parts qualify (`lmr38010`, `l_bourns_srn6045ta_6r8m`, `ss36`, `smbj30a`, `mf_msmf110_33x`, `jst_b2b_xh_a_2pin`, `jst_b4b_xh_a_4pin`, `c_electrolytic_smd_8x10`); `mf_msmf110_33x` carries review finding E-SYNTH-QUAL-016 (3D model not installed) | `reports/registry-qualify.txt` |
| Return-via proximity (A9) | 0 W-SYNTH-DRC-016 in both release files; a proximity test, not a return-path analysis | `reports/release.json`, `reports/release-fab.json` |
| Exports exist (A10) | 22 Gerbers plus job file, drill PTH/NPTH, BOM, pick-and-place | `kicad/` |
| Thermal copper present (A16) | 2.41 x 3.1 mm pad on GND, six GND vias inside; GND vias within 1/2/3/4 mm of pad centre: 2/6/6/7 | `kicad/buck.kicad_pcb` |

The release reports' own `gate_scope` text says `release_ready` and `production_status` cover only Synth's routing and export gates, not a fabrication approval, qualification result or component review.

## Failures and open gates

| Item | Status | Detail |
|---|---|---|
| `synth check --fab` (A18) | **fail** | Release stage passes. With `--fab`, the manufacturing stage fails: it routes with KiCadRoutingTools (not FreeRouting) and its copper leaves 11 signal vias without a ground return via within 0.64 mm (W-SYNTH-DRC-016); manifest `validation_failed`, `untrusted`. Matrix row has `expect = "pass"`, `expect_fab = "fail"`. The committed FreeRouting copper has none. |
| A14 placement review | not rerun | `synth_describe_placement` not rerun on the hand-pinned placement; the earlier score 100 was for the auto placement and no longer applies. |
| B1 surge / overvoltage | open | TVS clamp 48.4 V below the 85 V abs max; no surge test, no IEC 61000-4-5 claim; capacitors have 1.6 V margin at the clamp. |
| B2 thermal | open | Six vias in pad where TI asks for at least 4 x 3; no junction temperature estimated; filled-via process not confirmed with a fab. |
| B3 power good | open | Connection proven; thresholds, delay and fault behaviour not checked; Synth marks `power_sequencing_and_power_good` unsupported. |
| B4 filtering, loop stability, derating | open | No 3.3 V row in Table 9-1; DC-bias derating unknown; crossover and transient not computed. |
| B5, B12 loop area, EMI, ripple, decoupling | open | Only pad-edge gaps measured; ground leg of input caps longer than VIN leg (2.69 / 5.35 mm vs 0.82 / 1.46 mm). |
| B6 grounding / analog separation | open | No split ground planes (unsupported). R_FB_B ground 11.31 mm from U1 GND; R_FB_T FB pad 2.58 mm from L1 pad 1. |
| B7 reverse block, PTC margin, UVLO, Vout | open | Hand calculations only. |
| B8 footprint differences | open | Pad differences listed in `ASSEMBLY.md`; C1 polarity unverified; J1/J2 pin-1 polarity unstated. |
| B9 compactness | not compared | 2413 mm2 vs 1935 mm2 for the earlier LMR33640 auto placement; cause not isolated. |
| B10 keepouts | unsupported | None declared under the switch node or inductor. |
| B11 sequencing, soft start, inrush | unverified | |
| B13 stackup | not applicable | Default 4-layer stackup, no controlled impedance. |
| B14 orderability | not done | No fab accepted the files, no LCSC numbers on parts, generic passives have no MPN, availability not checked. |
| B15 schematic readability | readable, not tidy | `tools/schematic_quality.py`: 0 overlapping text, 7 texts with a wire through them, 1 wire crossing, 1313.2 mm of wire, mean passive-to-pin 66.4 mm (max 172.8). |
| B16 other #38 negative cases | not applicable here | Pair constraints and stackup cases belong to the USB board (#103). |
| B17 human reference comparison | not done | |
| B18 hardware | not done | Nothing powered or measured. |
| B19 independent SI/PI/RF sign-off | not done | No reviewer. |

## Discrepancies between committed files

- `README.md` says the fab package reports `release_ready: false` / `untrusted` because `lmr38010` and `ltst_c190kgkt` have no reviewer, and that `synth check` was not run to a verdict. The committed `reports/release-fab.json` shows `release_ready: true`, `production_status: production`, both parts reviewed 2026-10-29, and `QUALIFICATION.md` A6/A18 records the reviews and a `synth check` run. The report and QUALIFICATION.md are the later statements; the README text is stale on these points.
- `QUALIFICATION.md` and the README say "Not signed. `reviewed_by` stays empty" in the two review records for D3/D4 and U1, while the part files now carry `reviewed_by = "Steve Munene"`. Which is current: the part files and reports.
- `README.md` says `reports/buck.routing.json` records `source_revision` a9c7546...; `QUALIFICATION.md` says it holds a79570cd... (regenerated). The committed `reports/buck.routing.json` holds a79570cd84d7c9dcc67931593a42fc3836b76065, so the README line is stale.

## Not run for this reconstruction

No Synth tool, kicad-cli, router or hash check was run. `SHA256SUMS` was not rechecked. The Gerbers and drill files were not opened.
