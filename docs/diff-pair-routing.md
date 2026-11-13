# Differential-pair and via-in-pad routing policy

Status: accepted, 2026-11-13. Covers #439 and #447.

## Current engine contract

Synth **checks** a declared differential pair: `max_skew` is measured from the
emitted copper and `E-SYNTH-DRC-014` blocks a board that exceeds it. It does
**not** currently keep the two halves coupled while routing. The KiCadRoutingTools
adapter therefore leaves `route_diff.py` unused and the capability descriptor
records coupled routing as unsupported.

This is a real limit, not a hidden one:

- `diff_pair { max_skew 0.15mm }` is enforced after routing.
- A pair that routes uncoupled but stays inside its skew budget releases.
- A pair outside the budget does not release.

## Via-in-pad policy

`requires via_in_pad` is a **board-level** permission because that is what the
fabricator quotes. It is not a licence to put a via in every pad. The router is
expected to use an in-pad via only where the pad cannot be escaped beside it:

- a declared `escape { method underpad }` component, or
- a dense grid array whose arithmetic leaves no beside-pad room.

The 0.8 mm TSOP case (#439) shows the failure mode: a board-level permission
plus a general-router sentinel puts a via in about 60 signal pads, and no gap
is left for a ground return via. The fix is per-component accounting in the
escape stage, not removing the permission.

## Microvia tier

`jlc-advanced` now carries a 0.10 mm laser drill and a 0.25 mm finished
diameter. That land fits a 0.35 mm ball at 0.8 mm pitch. A tier without a
microvia keeps the through-via land and is refused, naming the sizes.

## Follow-ups

- Pass the per-component escape set to the helper so the general router cannot
  use an in-pad via outside a declared under-pad escape.
- Invoke `route_diff.py` when a pair is declared, and re-check skew after it.
