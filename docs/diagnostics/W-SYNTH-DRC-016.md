# W-SYNTH-DRC-016 — signal via without a ground return via

**Severity:** warning
**Stage:** drc

## What this means

A via takes a signal to another copper layer, but there is no ground via within
the stackup's thickest dielectric of it. Return current follows the signal's
reference plane; when the signal changes layers that plane is interrupted, so
the return current has to change layers too. Without a nearby ground via it
takes the long way round, and the resulting loop is a discontinuity a fast edge
radiates from.

Ground is identified the same way the exporter's plane synthesis identifies it —
by power-domain topology, then by a `gnd`/`vss`/`0v` net or pin name — so the
rule judges a via against the same ground the planes are built from. A board
with no ground net is not checked, and a board without a declared `stackup` is
not checked either, because the distance is a property of the dielectric.

The router's return-via repair holds a board back for a missing return via
only on a net that rides a fast edge: a differential or clock pin, an RF or
analog pin, a declared `diff_pair`, or a switching node at an inductor. On any
other net a layer change with no room for a return via is listed in the route
report's `advisories`, with the net and the position, and does not block.

The rule is advisory: whether a discontinuity matters depends on the edge rate
and the signalling scheme, which geometry alone cannot decide. As a `W-` code it
never blocks a release.

## Minimal reproduction

A board with a ground net and a `stackup`, routed with a signal that changes
layers where no ground via is placed nearby.

## Suggested fix

Place a ground via within a dielectric height of the signal via — the usual
"return via" beside every layer change — or keep the signal on one layer.
