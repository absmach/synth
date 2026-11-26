# Placement compression: shrink the finished outline while the judge agrees

**Status:** implementation note, 2026-11-11 (round 22).
**Scope:** outline shrink and the round-22 inward group pull. No re-packing.

## Why

`finish_outline` fits the export outline round the packed courtyards with a
tight rim, but the board is still the span of its docked connectors and holes.
A connector row can sit well inside the body, and a hole re-cornered on a wide
board leaves usable area at the edge. Round 21 walks each wall in and keeps
only the steps the round-20 judge accepts.

## What it does

`synth_place::compress_outline(board, placement)` runs at the end of
`finish_outline` unless `SYNTH_PLACE_COMPRESS=0` is set.

For each side in left, right, top, bottom order, it tries one 0.5 mm step:

1. Move that wall inward, keeping the opposite wall fixed.
2. Carry connectors flush on that wall, and their passive frames, by the same
   step. Re-corner the free mounting holes at 3.5 mm from the new corners.
3. Run the existing `space_groups` (2 mm minimum), re-dock every connector on
   its wall (or the nearest one), and slide each group of USB connectors flush
   to a wall so the group's bounding box sits at the middle of that wall.
4. Recompute the round-20 judge. Keep the step only when no gate's failure
   count increases (so a board that already fails a gate can still shrink),
   `overlaps`, `group_gap`, `outside_outline`, `hole_corner` and
   `usb_offset` do not get new failures, `docking` and header quality do not
   fall, `alignment` and `ring` drop by at most 0.02, and, on a board that
   passed before the step, the composite does not fall beyond the report's own
   0.005 point rounding. Re-corner the free holes after the settle, so a group
   that contains a hole cannot pull it out of its corner.
5. A rejected step restores the previous placement and marks that side done.
   The pass stops after 40 side attempts or when all four sides are done.

Parts and groups are never re-packed: a group that is not carried by a wall
keeps its internal geometry. `groups_under_2mm_apart` is the judge's
`group_gap.failures`, `edge_overhang` is `outside_outline`, and
`holes_in_corners` is `hole_corner.failures`.

## Report, round 21

Both boards were exported through `place_with_replace` + `finish_outline` with
`SYNTH_PLACE_REPLACE=1` and compressed by default. All hard gates hold, no
visual-review finding was added, and no alignment or ring point was lost.

| board | area before | area after | drop | judge score | gates | align | ring | empty mm² |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rtd_afe` | 2066.7 | 1851.8 | 10.4 % | 96.69 (was 96.30) | 0 | 0.935 | 0.857 | 106.7 → 45.4 |
| `fpv_fc` | 4471.0 | 4000.8 | 10.5 % | 97.58 (was 97.59) | 0 | 0.946 | 0.880 | 82.8 → 78.7 |

`SYNTH_PLACE_COMPRESS=0` reproduces the HEAD board byte for byte.

## Round 22: pull the groups inward first

`synth_place::pull_groups_inward(board, placement)` is the round-22 fix.
When the feedback loop is enabled, `finish_outline` tries it before the wall
walk when the judge's `largest_empty` or `group_gap` row is short of 1.0.

For every non-MCU group, nearest to the largest chip first:

1. Take the axis of greatest separation from the MCU chip. A group left of the
   chip moves right, one right of it moves left, one above moves down, one
   below moves up.
2. Measure the free gap to the first obstacle in that direction: any courtyard,
   a mounting hole's keep-out, a wall-docked connector, a USB connector plus its
   frame, and another group's box (the 2 mm `group_gap` gate).
3. Move the whole group rigidly by `free gap - 2.0 mm`, rounded down to the
   0.5 mm grid. A group with a hinted part is left alone. A move that would take
   a docked connector off its wall is dropped.
4. Run the existing `space_groups`, then the round-21 wall compression, then the
   judge. Keep the move only when every gate passes and the placement error
   drops; otherwise restore the placement and add the group to the tried list.

The error is the summed gap from the other groups to the MCU chip, then the
outline area, then the judge deficit. The MCU's own parts are checked unchanged
before a move is kept. Parts move rigidly, so a group keeps its internal
geometry and its alignment.

Tests: `crates/synth-place/tests/pull_groups.rs` covers an 8 mm group landing
2.0 mm from the MCU, a group stopped by a connector, a group already at 2.0 mm,
and the MCU and a wall-docked connector staying put.

## Report, round 22

The same two boards, exported through `place_with_replace` + `finish_outline`;
the pull is tried at the start of the round-21 compression.

| board | area before | area after | drop | judge score | gates | align | ring | empty mm² |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rtd_afe` | 1851.8 | 1851.8 | 0.0 % | 96.69 | 0 | 0.935 | 0.857 | 45.4 → 45.4 |
| `fpv_fc` | 4000.8 | 3945.3 | 1.4 % | 97.57 (was 97.58) | 0 | 0.946 | 0.880 | 78.7 → 78.7 |

The FPV target was a further 5 %, 3800.8 mm². The pull reaches 1.4 %, 145 mm²
short. It moves POWER 0.5 mm right to its 2.0 mm box gap with USB. The left
wall follows and one more right-wall step becomes reachable, for the 1.0 mm
width drop. The remaining steps are blocked:

- **Right.** The next step shifts the outline midpoint and `centre_usb` then
  slides J_USB 0.25 mm into R_CC1/R_CC2, which sit 0.2 mm away; the judge
  rejects it on `overlaps`. The ESC headers still have 1.1 mm of `group_gap`
  room to the IMU, but the USB collision comes first.
- **Top.** USB is 2.2 mm from the MCU group. A 0.5 mm step leaves 1.7 mm, below
  the 2.0 mm `group_gap` gate.
- **Bottom.** The MCU courtyard is 0.45 mm from the edge, less than one step,
  and the MCU does not move.
- The 78.7 mm² largest empty rectangle is the MCU-to-IMU group corridor widened
  by the judge's 1 mm inflation. Closing it would break `group_gap`.

The remaining edge slack is at most 1.1 mm right + 0.2 mm top + 0.45 mm bottom
(the right wall has already taken one of its 1.6 mm of header/IMU room). Even
spending all of it with finer than 0.5 mm steps would leave the board at about
3839 mm² (4.0 %); 5 % needs a re-pack that moves the MCU or a docked connector,
which this fix does not do.

## Round 22B: the feedback loop

`synth_place::refine_by_feedback(board, placement)` runs after the outline fit
and before the final wall pass. It scores the placement, fixes the worst
finding, re-scores, and keeps a change only when the total error strictly
falls. `SYNTH_PLACE_REFINE=0` disables the loop and the pull, reproducing the
round-21 placement exactly.

The error is the weighted sum from the judge:

```
overlaps*1000 + edge_overhang*100 + usb_offset_mm*10
+ undocked_connectors*50 + groups_under_2mm_apart*100
+ non_parallel_headers*50 + lone_parts*5
+ largest_empty_rect_mm2*0.05 + area_mm2*0.01
+ (1-alignment)*20 + (1-ring)*20
```

The loop maps the top finding to one fix: overhang → nudge it inside; USB
off-centre → `centre_usb_blocks`; undocked → `dock_loose_connectors`; group gap
→ `pull_groups_inward` then `space_groups`; header → `pack_edge_headers`; lone
part → move it one proportional step toward its group; empty rectangle →
`pull_groups_inward` followed by compression. The step is
`min(2.0 mm, finding_error * 0.1)`. A reverted fix is added to the tried list.

The loop stops after 12 iterations, when no untried finding remains, or after
two kept iterations gain less than 1 %. Every iteration is appended to the file
named by `SYNTH_REFINE_TRACE`; with it unset no trace is written. Tests (a)–(e)
are in `crates/synth-place/tests/default_rules.rs`: one-iteration overhang, a
raising fix reverted and not retried, a clean layout with zero iterations, the
12-iteration cap, and the flag-off round-21 output.

### Report, round 22B

| board | error off → on | drop | area off → on | judge score | gates |
| --- | --- | --- | --- | --- | --- |
| `rtd_afe` | 29.9369 → 29.9210 | 0.05 % | 1851.8 → 1850.3 | 96.69 → 96.69 | 0 |
| `fpv_fc` | 47.4252 → 46.8703 | 1.17 % | 4000.8 → 3945.3 | 97.58 → 97.57 | 0 |

Every measured row after the loop:

| row | rtd | fpv |
| --- | --- | --- |
| outline_area mm² | 1850.258 | 3945.339 |
| largest_empty mm² | 45.419 | 78.716 |
| alignment | 0.935 | 0.946 |
| ring | 0.857 | 0.880 |
| area_efficiency | 0.993 | 0.995 |
| docking | 1.000 | 1.000 |
| usb_offset mm | 0.00 | 0.00 |
| group_gap mm | 2.00 | 2.00 |
| header_alignment mm | n/a | 0.00 |
| overlaps / outside | 0 / 0 | 0 / 0 |
| hole_corner mm | 4.95 | 4.95 |

Runtime on the unrouted export: rtd 36.1 s → 38.1 s (+5.6 %), fpv 130.2 s →
120.2 s (noise; under the 20 % bound). Traces are
`/home/steve/work/exp009-out/refine_trace-rtd.tsv` and
`refine_trace-fpv.tsv`.

Attempts: (1) loop after compression: FPV did not improve, reverted; (2) loop
before compression: FPV gained but rtd did not, reverted; (3) loop before
compression with the extra wall pass: both errors fall, kept.

Five-line review:

- The pull is the loop's first empty-rect fix; it is the only FPV movement, and
  the trace records `largest_empty pull_groups_inward+compress_outline`
  52.3294 → 46.8703 kept.
- rtd's only gain is the extra wall pass after the pull: 1851.8 → 1850.3,
  0.05 %; the lone L2 fix is reverted and not retried.
- All hard gates hold on both boards; no alignment, ring, docking, USB, group
  or hole row moved.
- The loop adds one kept and one rejected iteration on FPV and three on rtd;
  the full unrouted run stays under the 20 % runtime bound.
- `SYNTH_PLACE_REFINE=0` reproduces the round-21 placement; the tests cover the
  overhang, revert/tried, clean, 12-iteration and flag-off paths.


## Round 24c: jump each wall to its rectangle limit

The 0.5 mm stepping is exact but pays for a group settle on every attempt. On
the 126-part board one rejected step took 311-345 s, and the settle moved the
holes that are declared inside `TEST POINTS AND MOUNTING` off their corners:
the trace showed a step that started with only `group_gap` failing and ended
with `group_gap,outside_outline,hole_corner`.

`compress_walls` now tries one jump per wall first. `wall_jump_limit` is
rectangle arithmetic: the furthest a wall can move while every stationary part
stays inside and every moving part keeps the 2 mm group gap from every
stationary part. A rejected jump is halved up to five times to a 0.5 mm grid.
A kept jump moves the wall, the connectors flush on it and their attached
frames, then re-corners the free holes; it does not run `space_groups`, so a
group cannot drag a mounting hole out of its corner. The jump phase is followed
by the round-21 0.5 mm stepping only while a wall's jump limit is at least one
step; on the 126-part board the jumps already reached every limit, so the
expensive settle was skipped. Boards with room left keep the stepping, because
that is what reaches their tight outline.

Acceptance is per gate: a step is kept when no gated row's failure count rises.
On a clean board this is the old "judge must pass"; on a board that already
fails a gate it allows an area improvement that neither adds a failing gate nor
increases the existing one. No judge limit changed.

| board | area before | area after | drop | runtime before | runtime after | gates |
| --- | --- | --- | --- | --- | --- | --- |
| `rtd_afe` | 1850.3 | 1850.3 | 0 | 36 s | 37 s | 0 (PCB byte-identical) |
| `fpv_fc` | 3945.3 | 3945.3 | 0 | 114 s | 115 s | 0 (PCB byte-identical) |
| `126-part board` | 14095.7 | 8679.8 | 38.4 % | 742 s | 420 s | 0 new; 67 `group_gap` before and after |

The 126-part board's dead area falls 8852.0 -> 3775.0 mm². The four jumps are
Left 31.0, Right 12.0, Top 2.5 and Bottom 8.0 mm; all are kept. The default
trace then records eight `shrink_false` rows from the later compression calls,
which find no room. The 67 pre-existing `group_gap` failures stay at 67;
`alignment` 0.6087 and `ring` 0.0714 are unchanged; `overlaps`,
`outside_outline`, `hole_corner` and the USB/docking/header rows do not
regress. The r24b
`hole_corner` regression is covered by
`compression_keeps_a_grouped_holes_corner_gate` (holes in a group, slack on
every side) and `a_mounting_hole_moved_off_its_corner_fails_hole_corner`.

## Environment switches

| variable | effect |
| --- | --- |
| `SYNTH_PLACE_COMPRESS=0` | turn the compression pass off; any other value, or none, leaves it on |
| `SYNTH_PLACE_REFINE=0` | turn the feedback loop and its pull off |
| `SYNTH_PLACE_COMPRESS_BUDGET_S` | wall-clock budget for one compression pass, default 120 s |
| `SYNTH_PLACE_REFINE_BUDGET_S` | wall-clock budget for the feedback loop, default 90 s |
| `SYNTH_REFINE_TRACE` | append the loop's iteration and stop rows to this file; unset writes nothing |
| `SYNTH_PLACE_COMPRESS_TRACE` | `1` writes `compress_trace.tsv` beside `SYNTH_REFINE_TRACE` (the working directory when that is unset); any other non-zero value is the trace file itself; unset or `0` writes nothing |
| `SYNTH_PLACE_STAGE_TRACE` | `1` writes `stage_trace.tsv` in the working directory; any other non-empty value is the trace path. Each row is the placement judge's metrics after one stage |

## What it does not do

- It does not move parts for their own sake, re-pack a group, or change the
  placement candidate chosen before `finish_outline`.
- It does not make the judge a production gate; the judge only decides whether
  a compression step is kept.
- It does not measure routing, edge copper clearance or DRC.
