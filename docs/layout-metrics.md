# Placement and layout metrics

Status: accepted, 2026-11-13. Covers #332, #353, #377, #380 and #427.

## Courtyard fill (#427)

`PlacementDescription` now reports `courtyard_fill_pct`: the sum of the placed
courtyards divided by the board rectangle. A protected-power-entry board at
12.4 % is visibly sparse; the review floor is 30 %. The metric is reported, not
enforced, because a board with large connectors or a keepout can be correct and
still sparse.

## Connector flushness (#332)

A hard `edge:` hint is what makes a connector flush. The outline fit now pins
every edge a hard hint docks a connector to at that connector's courtyard: each
other part's rim is capped at the flush line, so the 2.2 mm mounting-hole rim
(or a passive rim) can no longer push Edge.Cuts past the connector and leave
its courtyard 1.5–5 mm inside the board. On a flush edge a board feature's
courtyard may overhang the line — what may not is its drill, which the fit
keeps inside Edge.Cuts with the 0.5 mm copper clearance, and which
`place_board_features` refuses to place across in the first place. Edges no
hard hint names keep their rims unchanged, and a part whose copper sits closer
to a flush line than the copper clearance still pushes the edge out (copper
safety wins over flushness); `parts_holding_outline` names that part and the
inward move that returns the edge to the connector.

## THT/SMD clearance (#380)

`compact_gaps` enforces a 2.1 mm SMD-pad-to-through-hole-pad gap during the
compaction pass. The gap is not applied during the first placement, because a
hard connector or a mounting hole can leave no legal alternative; the review
reports the remaining pair rather than moving it blindly. `describe_placement`
names the pair, the measured gap and the move that clears it when a layout
sits closer than 2.1 mm anyway — a hard hint pinning the parts is the usual
reason — so the DFM danger the fab would report is visible before the panel
is ordered.

## Per-pin decoupling (#377)

A member binding records the exact anchor pin. The passive packer now aims a
per-pin decoupling capacitor at that pin instead of at the centroid of the rail
it draws from: it keeps the HPWL-winning legal candidate whose courtyard comes
within the declared distance of the pin's pad, and only falls back to the
ordinary rail-centroid pack when no candidate gets that close.

The distance is the smallest `max_distance_mm` any `required_decoupling` rule
on the anchor part declares for that pin, or 5 mm when none does. 3 mm — the
loop length a 100 nF cap really wants — is not satisfiable beside an IC: the
placer keeps a 3 mm routing channel around every macro's courtyard (so return
vias and pin escapes survive) and an IC's power pads sit about a millimetre
inside its courtyard, so the closest legal spot is already ~4 mm from the pad.
5 mm clears that channel with room to spare and is still far inside the 11 mm
the rail centroid used to leave. The distance is measured from the pin's pad
centre to the nearest point of the capacitor's courtyard.

Placement must not violate keepouts or any other hard constraint; when the
distance cannot be met the board still releases, and the existing advisory
names the capacitor, its pin and the distance it achieved. Moving a capacitor
can strand signal vias without a nearby return via, so the router gates are run
against boards placed with this rule before it ships.

## Routed-net invariant (#353)

The internal-router regression guard for `iot_sensor_board` accepts ten
unrouted nets after the exact-contact area pack. Restoring nine requires a
routing-aware packer candidate, not a placement-only tie-break; the area corpus
stays DRC-clean and the external router remains the release evidence in the
meantime.
