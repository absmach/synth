# Compaction with stacked USB connectors

**Status:** implementation note, 2026-11-26.
**Scope:** the `centre_usb` step of `compress_outline` only.

## Cause

`compress_outline` moves a wall inward, carries the connectors docked to it,
then calls `centre_usb` (the same function from `jump_wall` and from the 0.5 mm
`shrink_side` path). The old rule moved every USB connector flush to a wall to
that wall's midpoint independently.

The 121-part CM4 carrier has two USB-A connectors (`J3`, `J4`) 46 mm apart on
the right wall and an RJ45 (`J2`) at the wall midpoint. Every wall jump moved
both USBs to the same midpoint, collapsing the pair and overlapping `J2`, `J3`
and `J4`. The judge saw `overlaps` rise from 0 to 3 and rejected all 16 jumps;
the board never shrank.

## Change

`centre_usb` now groups the USB connectors flush to each wall and translates
the group's bounding box so its midpoint sits at the wall midpoint. The whole
group, and each member's attached passive frame, moves by one translation, so
the spacing inside the group is kept. For a single USB the delta, clamp and
shift are identical to the old rule. A multi-USB group longer than its wall is
left where it is instead of being pushed further out.

No caller changed: `jump_wall` and the 0.5 mm step path both call the same
`centre_usb`.

## Carrier result

`compress_outline` on the cached `synth place` arrangement
(`qualification/class-d/cm4-carrier-clean/cm4_carrier.synth`, no sidecar,
`SYNTH_PLACE_COMPRESS_BUDGET_S=600`, release probe):

| | width x height | area | dead area | gate failures | score |
| --- | --- | --- | --- | --- | --- |
| before | 153.680 x 114.715 mm | 17,629.4 mm² | 0.4167 | 45 | 47.837 |
| after | 141.180 x 108.215 mm | 15,277.8 mm² | 0.3423 | 45 | 48.076 |

The gate-failure set is unchanged (`usb_offset` 2, `group_gap` 43); no new
failure is introduced. The run took 117.7 s: the left, right and bottom jumps
are kept. On the top wall the full 4.5 mm jump is rejected because shortening
the wall moves the right-wall midpoint, the centred pair follows it, and one
connector then overlaps the RJ45; the 2.0 mm top jump is kept. The remaining
2.5 mm of top wall goes through one rejected 0.5 mm fallback step, whose
`space_groups` settle costs 116.5 s. The kept jumps take 0.4 s in total. The board therefore stops at 108.215 mm rather than the 105.715
mm a run that skipped `centre_usb` reached. The fallback step is unchanged by
this fix.

## Tests

`crates/synth-place/tests/default_rules.rs`:

- `compression_centres_a_stacked_usb_group_without_overlaps`: two USBs and an
  RJ45 on one wall 46 mm apart; the outline shrinks and no courtyards overlap.
- `compression_centres_a_lone_usb_on_its_wall`: one USB still lands at the wall
  midpoint, flush to the wall.
- `compression_centres_usbs_on_different_walls_separately`: one USB on the
  right wall and one on the top wall are centred independently.
- `compression_leaves_a_centred_usb_group_where_it_is`: a pair whose midpoint
  already matches the wall midpoint keeps its positions while the wall moves.
- `compression_translates_a_usb_group_as_a_block`: a pair off the wall midpoint
  moves by one delta and keeps its 46 mm separation.
- `compression_without_a_usb_still_shrinks`: no USB is a no-op for the rule.

`crates/synth-place/src/default_rules.rs` (unit):

- `a_usb_group_longer_than_its_wall_is_not_moved`: an already-overflowing group
  is not moved further out.

## Known limit

The judge's `usb_offset` row measures each USB body centre against the board
midpoint individually (`crates/synth-place/src/judge.rs`, `usb_centring`). Two
stacked USBs 46 mm apart cannot both be within 0.5 mm of the midpoint, so the
carrier still reports 2 `usb_offset` failures. Compaction tolerates the
pre-existing failures; making that row pass needs a group-aware judge and is a
separate change.
