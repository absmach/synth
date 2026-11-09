# How this board was made

An agent produced the design; Synth's own tools and scripts then packed and
tidied it, and a reviewer's list of faults was worked through. Every step is
logged under [`agent-run/`](agent-run/).

| Step | What ran | Result |
|---|---|---|
| 1. The design | An agent from one prompt ([`agent-run/prompt.md`](agent-run/prompt.md)), driving `synth mcp` with Read, Write and Edit only, from an empty workspace ([`report.md`](agent-run/report.md), [`tool-calls.md`](agent-run/tool-calls.md)) | 86 components |
| 2. Compaction | `synth_compact_layout` in `gap` mode | 75.1 x 65.4 mm after the later steps, released |
| 3. Fill | Two agent runs ([`fill-pass-prompt.md`](agent-run/fill-pass-prompt.md), reports [1](agent-run/fill-pass-report-1.md), [2](agent-run/fill-pass-report-2.md)): parts in the voids, each wired to a real function | 26 parts added |
| 4. Placement advice | [`tools/apply_advice_par.py`](agent-run/tools/apply_advice_par.py) applies Synth's placement advice in released steps | two passes, 15 and 18 moves kept |
| 5. Review fixes | An agent from [`prompt-1-review-fixes.md`](agent-run/review-fixes/prompt-1-review-fixes.md), report [`report-1-review-fixes.md`](agent-run/review-fixes/report-1-review-fixes.md): the clamps, the input TVS, the crystal load capacitors, the valve current, the schematic groups | source fixed |
| 6. Release search | Scripts ([`release_search.py`](agent-run/tools/release_search.py), [`fix_one.py`](agent-run/tools/fix_one.py), [`fix_pairs.py`](agent-run/tools/fix_pairs.py), [`verify_gap.py`](agent-run/tools/verify_gap.py)) that move one or two parts near a blocking finding and keep a move only if the export releases | released |
| 7. Input MOSFET | An agent from [`prompt-2-input-mosfet.md`](agent-run/review-fixes/prompt-2-input-mosfet.md), report [`report-2-input-mosfet.md`](agent-run/review-fixes/report-2-input-mosfet.md): `Q1` becomes an AO3401A (30 V) | released |
| 8. Fill again | An agent from [`prompt-3-fill.md`](agent-run/review-fixes/prompt-3-fill.md), report [`report-3-fill.md`](agent-run/review-fixes/report-3-fill.md): 18 parts, from values that already have a sourced part | 126 parts, 31.1 % fill |
| 9. Edits by the orchestrator | see below | final source and sidecar |
| 10. Final blockers | An agent from [`final-blockers/prompt.md`](agent-run/final-blockers/prompt.md) moved `D_TVS` ahead of `Q1` and rewrote the notes at 13.0 V | source edit kept |
| 11. Searches | [`final-blockers/tools/`](agent-run/final-blockers/tools/): decoupler placement on a x1.4 layout, then `R_NRST`, `C_NRST` and `SW_RESET` spots and pairs, a release search | decouplers 2.8 to 3.6 mm from their pins on the released board |
| 12. Silkscreen and vias | Synth commits `8052ca52`, `a38802ad` (silkscreen widened and cut back), `2ef37388` (0.2 mm drill, Synth PR #388), then on `SYN-case1-binary` the 0.25 mm clearance with 0.18 mm strokes and `7972c911` (test-point rings dropped) and `be19e90a` (pin-1 triangles dropped too) | in the exports below |
| 13. Input capacitors at the regulator pin | `C_IN1` and `C_IN2` change from a 1206 to an 0805 part (same 10 uF 25 V); [`make_room_search.py`](agent-run/tools/make_room_search.py) and [`tht_chain.py`](agent-run/tools/tht_chain.py) place them; Synth commits `ffa0c7ba` and `efb2f8be` on `SYN-case1-binary` clear the last silkscreen overlaps and `f7b110db` adds the missing power flag | both inside 4 mm of `U1.2`; 0 `silk_overlap`; 0 ERC errors; released |

### Edits applied by the orchestrator

Added 2026-10-31, after Sammy's review of 2026-10-30 10:57:

- Source: `C_IN1` and `C_IN2` changed from `c_generic_1206` to `c_generic_0805`, value `10uF 25V` unchanged, by `sed`, so that both fit within 4 mm of `U1.2`. The 0805 part is the Samsung CL21A106KAYNNNE (X5R, 25 V, +/-10 %, LCSC C15850). The rail also carries the 100 uF `C_BULK` and the fill capacitors `C_N7` and `C_N9`.
- Sidecar: `C_IN1` to (26.5, 13.6) turned 180 degrees (so its supply pad faces `U1.2`), `C_IN2` to (30.6, 13.6), `R_B1` to (42.3, 4.5) and `C_BULK` to x 38.25, by [`tht_chain.py`](agent-run/tools/tht_chain.py). It exports with the two capacitors at each of eight candidate pairs, moves a plain resistor or capacitor that ends under 2.2 mm from a through-hole pad to the nearest free spot, exports again, and keeps a chain only when it releases with no such pair (`C_BULK`'s x was set to where the exporter had already settled it). The 2.2 mm limit is 0.1 mm above the 2.1 mm that JLCPCB's DFM treats as a Danger.

These small edits were made by script and checked by export. Each one changes position or group
membership only.

- Sidecar positions: `R_D1` to x 45.0, `R_FBB` to (34.9, 25.1), `C_BS` to (27.1, 15.5) and `L1` to (25.0, 20.3), each found by the release search above because it cleared a blocking finding; `J_SOIL1` restored to (42.5, 63.0), where the other two 3-pin connectors sit, after an earlier slide moved it 2 mm off its edge, and `R_N5` moved to (50.0, 60.5) so it no longer overlaps that connector's courtyard; `C_OUT2` to (41.25, 23.0), 11.6 mm from the regulator instead of 29 mm.
- Groups: the review and fill runs left 18 schematic groups; [`merge_groups_remove_part.py`](agent-run/tools/merge_groups_remove_part.py) (with `KEEP_TP_V1=1`) folds them into 13. No part, value or net changed.

- Source: the agent's note edit left a stray `"` after "valve loads." in the `R_B1` note, which made Synth report three parse errors and drop that note; the one character was corrected.
- LEDs: `D_PWR` and `D_N2` changed from `led_green_0603` to the registry part `ltst_c190kgkt` (same footprint and pins), by `sed`; the old part is a NationStar NCD0603G1 with a forward voltage of 2.6 to 3.6 V, which cannot light from 3.3 V through 680 ohm.
- Netclasses: `via_diameter 0.6mm via_drill 0.2mm` added to `PWR12` and `PWR33`, and a `Default` class with the same.
- Sidecar: only `D_TVS` moved, from (27.5, 7.0) to (14.5, 7.5) (the agent, to sit at the input).
- Sidecar: `C_U4` moved from (55.0, 44.5) to (51.62, 41.78) and the test points `TP_SWDIO` from (71.5, 26.5) to (70.34, 26.81) and `TP_SWCLK` from (71.5, 31.5) to (70.3, 31.5), each found by a release search ([`tht_search.py`](agent-run/tools/tht_search.py) for the test points, which keeps a position only when the export still releases) to clear a JLCPCB finding.
- Sidecar: `C_U5` moved from (54.0, 56.0) to (54.64, 48.76), `C_IN1` from (31.0, 11.5) to (27.54, 13.19), `C_BS` from (27.1, 15.5) to (25.1, 15.5) and the fill capacitors `C_N13` from (54.5, 48.0) to (52.5, 39.0) and `C_N9` from (26.5, 13.0) to (23.0, 6.0), each by [`make_room_search.py`](agent-run/tools/make_room_search.py): it moves a decoupling capacitor to a spot near its supply pin and moves the parts that do not serve that IC out of the way (the named input and supply capacitors, the IC's own support parts and connectors never move), and keeps a move only when the export still releases and no through-hole to SMD gap falls under 2.4 mm.

The `.synth` source and the placement sidecar beside this file are the result.
The board needs Synth built from pull request #370 and the ones it stacks on
(#310, #345, #348, #352, #368), the silkscreen commits above (`SYN-silk-clear-of-vias`) and `SYN-case1-binary`, the drill change of #388, the parts of #326, and #266 for `polyfuse_1r5a`
and `l_1210_10uh`, which are not in the registry on `main`. `smaj13a_tvs` and
`p_mosfet_ao3401a` were authored for this board and are in `registry/parts/`;
the repository owner (`JeffMboya`) reviewed both on 2026-10-31, recorded under `[provenance]`
(`reviewed_by`, `reviewed_at`); the copies in `reproduce/user-registry/` are the ones the exports ran with. They differ from the
records in `registry/parts/` in the review date (2026-11-02 there, 2026-10-31 here), in the `datasheet_url` of
`smaj13a_tvs`, and in the lifecycle of `mp2307dn_buck_ep`: `active` with no substitutes in the user registry,
`obsolete` with `substitutes = ["mp2307_buck_ep"]` in `registry/parts/`.

The agent stages ran on KiCad 10.0.7 and the Synth binary of the day; the final exports were made on KiCad 10.0.6 with commit `a12aa289b7b001e8d5a197b15d20302e4718da5c` of `absmach/synth` (`upstream/main`: the merge of Synth pull request #269, which carries the router fix that counts the routing report's statistics after the repair passes, on top of `0ceb7b80`, the merge of #388 with #370 and #394 before it). `allow_incomplete` was never set, in any run log or
script.

