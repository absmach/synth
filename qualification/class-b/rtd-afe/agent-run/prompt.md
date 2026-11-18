# Prompt: PT100 4-wire RTD analog front end (rtd-afe, SYN-363)

This board was built by agents, in stages, from the briefs below. There was no single prompt. The two briefs are copied verbatim from the session transcripts. After them, the board was changed by review rounds and by routing results; the section "Differences between these briefs and the committed board" lists what could be traced. A different run will not produce an identical board.

## 1. Request that started the work (2026-10-27)

> we need 2 other unique class B board. My first 2 board are 103 and 110 but I need 2 and they should be complex in class B

This went through the grilling, to-spec, to-tickets and implement-spec steps, which produced spec #361 and ticket #363 (this board).

## 2. Implementation brief for ticket #363 (2026-10-27 18:36, verbatim)

````text
Implement GitHub issue absmach/synth#363 (`gh issue view 363 -R absmach/synth`; parent spec #361, read it too). Work in the existing worktree /home/steve/work/synth-classb (branch syn-361-class-b-boards; #362 and #364 are committed there: ADS124S08 and REF5025 are in registry/parts/ic/). Do not touch /home/steve/work/synth.

Pattern to copy: the buck board's directory `qualification/class-b/buck/` on branch origin/syn-67-buck-board (look with `git show origin/syn-67-buck-board:qualification/class-b/buck/QUALIFICATION.md` etc.) and the USB-UART board on origin/syn-66-usb-uart-board; main already has a qualification/class-b dir, inspect it first. Create qualification/class-b/rtd-afe/ with the .synth source, placement layout, QUALIFICATION.md and SHA256SUMS.

Board: 4 layers, PT100 4-wire input, RC input filter, ADS124S08 with IDAC excitation and REF5025 reference, rp2350 over SPI with USB-C readout (rp2350, usb_c_receptacle and esd_usb are in the registry), AGND/DGND split with a single-point tie, decoupling from the part files. Novel and publication-quality: sensible placement (analog on one side, digital on the other, reference close to the ADC), silk labels, board outline.

Rules:
- Use the mattpocock `tdd` skill. Seam: existing golden/export tests; first write the failing check (e.g. the board exports, KiCad DRC clean including unconnected items, AGND and DGND each present and joined at exactly one tie), then build.
- Autoroute: `synth export-kicad <file> --out <dir> --autoroute --force` (FreeRouting jar at tools/freerouting/, JRE at tools/jre25/, both in the main checkout /home/steve/work/synth/tools; copy or symlink into the worktree, they are git-excluded).
- Prove: KiCad DRC clean via kicad-cli (include unconnected items, do not trust Synth's own gate), the exported .kicad_pcb reopens (pcbnew Python), AGND/DGND single-tie check. Record datasheet review per part and what needs an SI/PI/RF expert in QUALIFICATION.md.
- If a Synth defect blocks the build, fix it only when blocking, in a separate commit; otherwise write it down in your report (do not file issues yourself).
- Golden hashes: do NOT regenerate golden.json/SVG hashes locally; leave them for ticket #366.
- Do NOT run `cargo test --workspace` (hangs). Run the narrow crates you touch. `cargo clean` when done.
- Commit with `git commit -sm "SYN-363 - <message>"`, sign-off only, no Claude attribution. Do not push. Spawn no sub-agents.
Report: commit hashes, files, evidence (DRC output counts, reopen result, tie check), Synth defects hit, open items.
````

## 3. Finalisation prompt for opencode with DeepSeek (written 2026-10-29, verbatim)

````text
You are finishing ticket SYN-363, the RTD analog front end Class B board, in the Rust PCB tool "Synth" (absmach/synth). Another engineer (Rodney) is building the BLE board (#365) in parallel: do not touch the nRF52840, balun, antenna, BME280 or crystal parts, and do not touch anything for #365.

## Where to work
- Worktree: /home/steve/work/synth-classb (branch syn-361-class-b-boards, based on upstream/main). Work only there. Do not touch /home/steve/work/synth.
- Read first: `gh issue view 361 -R absmach/synth` (spec) and `gh issue view 363 -R absmach/synth` (your ticket). Always pass `-R absmach/synth` to gh.
- Reference boards for format: qualification/class-b/usb-uart/ in the worktree (README / QUALIFICATION.md / SHA256SUMS layout). The buck board is on branch origin/syn-67-buck-board (qualification/class-b/buck/).

## Spec in one paragraph
Novel 4-layer RTD measurement board: PT100 4-wire input, RC input filter, ADS124S08 24-bit delta-sigma ADC (IDAC excitation), REF5025 reference, rp2350 over SPI with a USB-C readout port, AGND/DGND split joined at exactly one tie point (R_TIE). Publication-quality: analog on one side, digital on the other, reference close to the ADC, clean silk labels and outline. Public post material, so every claim in QUALIFICATION.md must be evidence-backed.

## Done so far (do not redo)
- #362 committed 612ccc4: registry/parts/ic/ads124s08.synth.toml (hand-authored from TI SBAS660C, no KiCad symbol exists) and ref5025.synth.toml (imported, pins checked against TI SBOS410I), plus the pin test `rtd_front_end_parts_match_their_datasheet_pinouts` in crates/synth-registry/src/loader.rs.
- #364 committed d17e651 (BLE parts; not yours, leave alone).
- #363 commit 81bb62a: exporter fix so each split ground's fence vias stay inside its own pour (crates/synth-kicad/src/pcb.rs, test in tests/split_ground_pcb.rs).
- UNCOMMITTED in the worktree: qualification/class-b/rtd-afe/ containing rtd_afe.synth (237 lines), rtd_afe.placement.layout.toml, check.sh, kicad/ (exported .kicad_pcb/.kicad_sch, gerbers, drill, bom.csv, pnp.csv), reports/ (kicad-drc.json, erc, netlist, routing, connectivity, release), images/ (pcb-3d-top.png, pcb-3d-bottom.png).
- Known state: the last KiCad DRC report (reports/rtd_afe.kicad-drc.json) has 0 unconnected items and 12 entries in `violations`. Their severities are NOT yet reviewed.

## What is left (in order)
1. Re-run `qualification/class-b/rtd-afe/check.sh` from the repo root (build first: `cargo build -p synth-cli`; the script uses target/debug/synth; FreeRouting jar is tools/freerouting/freerouting-2.4.1.jar and JRE tools/jre25/, both git-excluded and living in /home/steve/work/synth/tools, symlink or copy them in). Read every one of the 12 DRC violations and the ERC report. Fix real errors (placement, source, or a blocking Synth defect). Warnings that remain must each be explained in QUALIFICATION.md. The acceptance bar: zero DRC errors, zero unconnected items, reopen OK, AGND/DGND tied only at R_TIE.
2. Visual check: open images/pcb-3d-top.png and bottom.png; fix anything ugly (overlapping silk, labels far from parts, parts crammed, reference far from ADC).
3. Write qualification/class-b/rtd-afe/QUALIFICATION.md and README.md in the style of the usb-uart board: what the board is, stackup (4 layers, which are planes), the check results with the real numbers, per-part datasheet review table (ADS124S08, REF5025, rp2350, USB-C, ESD, passives: datasheet URL, pins checked, decoupling), the AGND/DGND tie argument, and an explicit "needs an SI/PI/RF expert / human reference / independent sign-off" section. Do not claim anything you did not measure.
4. Generate SHA256SUMS for the deliverables (same convention as usb-uart).
5. Run only the narrow tests for crates you touched: `cargo test -p synth-kicad --test split_ground_pcb` and `cargo test -p synth-registry`. NEVER run `cargo test --workspace` (it hangs). Run `cargo clean` when finished.
6. Commit in small commits: `git commit -sm "SYN-363 - <message>"`. Sign-off only. No Co-Authored-By or any AI attribution line.
7. Stop and report. Do NOT push, do NOT open a PR, do NOT merge, do NOT comment on GitHub issues.

## Hard rules
- Golden hashes (fixtures/kicad-reference/svg/golden.json and the golden snapshots): do NOT regenerate locally. They come from CI output in ticket #366.
- Do not trust Synth's own "0 violations" line. The proof is `kicad-cli pcb drc --severity-all --refill-zones` plus unconnected items, and a pcbnew reopen.
- If a Synth defect blocks the build, fix it minimally in its own commit with a regression test, and write the defect down in your final report. Do not fix unrelated things.
- Match the style of surrounding code and files. Keep changes surgical.
- Use TDD for any Rust change: failing test first.

## Final report (keep short)
Commit hashes; DRC counts (errors / warnings / unconnected) before and after; reopen result; tie-check result; list of files in the board directory; every Synth defect you hit; anything left that needs a human (expert review, fab-house check, real hardware).
````

## Differences between these briefs and the committed board

Found by comparing the briefs with the committed files. Where no instruction could be found, the entry says so.

- **Ground.** The briefs ask for an AGND/DGND split joined at one tie point (R_TIE). The committed board has one GND plane and no AGND or DGND net. `QUALIFICATION.md` says the split version needed a tie and still left two isolated pours on the merged router. No instruction for the change was found.
- **Board outline.** The briefs ask for a "board outline". The committed outline is a closed notched path in `rtd_afe.placement.layout.toml` (`[[outline]]`), added in commit c08f766 after the review comment that a normal export produced a different rectangular board.
- **USB-C.** The committed board has the USB-C centred on the top edge (commit f4b3a37). No instruction for that was found.
- **Via-in-pad.** One via-in-pad at C_DV.2 is required, with the manufacturer profile `jlc-filled-via`. The briefs do not mention it.
- **Silkscreen.** 34 reference labels are hidden to avoid clipping. The briefs do not mention it.
- **Part review.** The briefs ask for a datasheet review per part. Only ADS124S08, LP5907 and REF5025 are recorded as reviewed (by the author). The release is `production_status: untrusted`.

## What the briefs cannot reproduce

- The placement and sidecar coordinates.
- The routed result. FreeRouting is not bit-for-bit repeatable and was sensitive to placement.
- The independent SI/PI/RF, part and fabrication reviews. They have not happened.
