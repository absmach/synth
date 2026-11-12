# Placement and layout metrics

Status: accepted, 2026-11-13. Covers #332, #353, #377, #380 and #427.

## Courtyard fill (#427)

`PlacementDescription` now reports `courtyard_fill_pct`: the sum of the placed
courtyards divided by the board rectangle. A protected-power-entry board at
12.4 % is visibly sparse; the review floor is 30 %. The metric is reported, not
enforced, because a board with large connectors or a keepout can be correct and
still sparse.

## Connector flushness (#332)

A hard `edge:` hint is what makes a connector flush. The review names the
connector and, when another courtyard reaches that edge first, the blocking
part. A mounting hole is measured by its copper plus the 0.5 mm edge clearance,
not by its screw-head courtyard, so it no longer holds a bottom connector
1.5 mm inside.

## THT/SMD clearance (#380)

`compact_gaps` enforces a 2.1 mm SMD-pad-to-through-hole-pad gap during the
compaction pass. The gap is not applied during the first placement, because a
hard connector or a mounting hole can leave no legal alternative; the review
reports the remaining pair rather than moving it blindly.

## Per-pin decoupling (#377)

A member binding records the exact anchor pin, and `far_support_parts` reports a
decoupling capacitor farther than its limit (4 mm by default) from that pin.
The placer packs passives by HPWL, so a dense board can still leave a capacitor
outside the limit; the finding is what makes that visible and actionable.

## Routed-net invariant (#353)

The internal-router regression guard for `iot_sensor_board` accepts ten
unrouted nets after the exact-contact area pack. Restoring nine requires a
routing-aware packer candidate, not a placement-only tie-break; the area corpus
stays DRC-clean and the external router remains the release evidence in the
meantime.
