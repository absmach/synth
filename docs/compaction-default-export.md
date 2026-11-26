# Compaction in the default export

**Status:** implementation note, 2026-11-26.
**Scope:** `place_for_export_default` in `crates/synth-kicad/src/export.rs`.

## What changed

For an auto-sized board with no placement sidecar, the export used to finish the
chosen candidate with `finish_default_placement`, the tight/passive outline fit
and the grid trim only. It now calls `synth_place::finish_outline` after
`finish_default_placement`. `finish_outline` runs the same
`fit_outline_with_passive_margin_mm` and `trim_outline_to_grid` with the same
margins, then the refine loop and compaction. The rim and grid-trim behaviour is
therefore unchanged and no extra candidate is added; the candidate is finished,
not re-selected, though the refine loop may move its parts, as it does for every
other `finish_outline` caller.

Calling `compress_outline` after the same fit and trim was tried first: it keeps
the parts exactly as placed, but on the `rtd_afe` fixture it shrank nothing
(2127.4 mm² with and without it), so the export would still not compact. The
chosen path is `finish_outline`; the test below pins the compacted result.

Boards with a declared outline or a placement sidecar take the same path as
before.

## Corpus

Every board under `qualification/` and `examples/` with no placement sidecar and
no declared outline, exported through `placement_for_export` +
`export_with_placement` on a debug build. `gates` is
`synth_place::judge(...).gate_failures`; time is the whole placement plus
export. Boards with a placement sidecar are listed as skipped.

| board | before W × H | after W × H | before mm² | after mm² | area | gates before → after | time before → after |
| --- | --- | --- | ---: | ---: | ---: | --- | --- |
| `examples/divider` | 8.160 × 3.200 | 7.160 × 2.200 | 26.1 | 15.8 | −39.5 % | 0 → 0 | 1.33 s → 0.63-0.85 s |
| `examples/led` | 7.960 × 3.200 | 6.960 × 2.200 | 25.5 | 15.3 | −40.0 % | 0 → 0 | 1.87 s → 0.89-1.42 s |
| `examples/placement_and_diff_pair` | 16.460 × 25.810 | 15.460 × 25.810 | 424.8 | 399.0 | −6.1 % | 0 → 0 | 24.69 s → 30.65 s |
| `examples/rc_filter` | 8.180 × 3.260 | 7.180 × 2.260 | 26.7 | 16.2 | −39.3 % | 0 → 0 | 1.38 s → 1.16 s |
| `examples/sensor_logger` | 41.990 × 51.990 | 41.990 × 51.990 | 2183.1 | 2183.1 | 0 | 1 → 1 | 225.69 s → 195.92 s |
| `qualification/class-a/negative-broken-mcu-board` | 32.990 × 20.060 | 32.990 × 18.060 | 661.8 | 595.8 | −10.0 % | 0 → 0 | 16.30 s → 13.92 s |
| `qualification/class-b/buck/buck_neg_fb_sw` | 35.495 × 45.650 | 35.495 × 42.990 | 1620.3 | 1525.9 | −5.8 % | 0 → 0 | 356.2 / 388.1 s → 467.7 / 420.4 s |

No board grew and no board gained a placement-gate failure.

Time: the divider and led first AFTER runs (3.93 s, 6.49 s) were cold-start
outliers; warm repeats are 0.63-0.85 s and 0.89-1.42 s, faster than BEFORE.
`buck_neg_fb_sw` is the only board whose first pair crossed +30 % (+31.3 %); the
repeat was +8.3 %, and the two-sample medians are 372.2 s → 444.1 s (+19.3 %).
The run-to-run spread is about ±10 %, so this is not a consistent >30 % export
time regression. `placement_and_diff_pair` is +24.1 %, under the 30 % threshold.

## Carrier

`qualification/class-d/cm4-carrier-clean/cm4_carrier.synth` from the cached
archive, no sidecar. The full default export did not finish inside 30 minutes:
its pre-existing 16-candidate sweep is about 16 × 192 s of placement on this
121-part board before the changed branch runs. The branch itself was measured on
the cached `synth place` arrangement:

| | Edge.Cuts | area | gates | time |
| --- | --- | ---: | --- | --- |
| before | 153.680 x 114.715 mm | 17,629.4 mm² | 45 | placement 192.3 s (cached) |
| after | 141.180 x 108.215 mm | 15,277.8 mm² | 45 | compression 117.7 s |

The result is below the uncompressed board but above the 14,925 mm² a run that
skipped `centre_usb` reached; the group-aware pass stops at 108.215 mm high for
the reason in `docs/compaction-stacked-usb.md`. The full export time is
therefore unreported, not silently omitted.

## Tests

`crates/synth-kicad/tests/default_export_compaction.rs` places the same
auto-sized fixture twice: once in a child with `SYNTH_PLACE_COMPRESS=0` and once
through the default export path, and asserts the export is strictly smaller than
the uncompressed fit. Before this change both were 2319.5 mm²; with the change
the export is smaller.
