# 0002 — Under RePlAce, a fold may move a gated row inside its limit when it seats a connector

**Status:** accepted (2026-11-21)

## Context

`fold_is_better` keeps a folded candidate only when no measured row is worse
than the input. On `rtd_afe` the folded candidate is 25 % smaller, seats all
three connectors where the input seats one, and clears six group-gap failures.
It moves the USB 0.435 mm, inside the judge's 0.5 mm limit. The strict
not-worse comparison rejected it and kept the larger, undocked board, so the
compression tests that assert a clean judge could not pass.

## Decision

Under the RePlAce seam (`place_with_replace`), a row with a pass/fail limit may
move while the value after the fold still passes, but only when the fold also
reduces the docking failure count. The default placement paths pass
`allow_usb_move = false` and keep the strict not-worse comparison. Every other
row keeps the strict comparison everywhere, alignment and ring including their
slack, and a value past a limit always rejects.

`usb_offset` is currently the only row with a limit and a lower-is-better
direction, so the exemption is written for that row.

## Consequences

- The rtd fold is accepted under the RePlAce seam; the two compression tests
  are no longer ignored.
- A RePlAce fold may trade up to 0.5 mm of USB centring for a docked connector
  and a smaller board. The production default path may not.
- A fold that does not change the docking count keeps the strict comparison,
  so the FPV default path is unchanged (verified by its USB-frame and
  edge-header tests).
- If another gated row with a lower-is-better limit is added, it needs the same
  decision, not a copy of this branch.
