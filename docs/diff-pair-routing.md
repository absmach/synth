# Differential-pair and via-in-pad routing policy

Status: accepted, 2026-11-13. Covers #439 and #447.

## Coupled routing

A declared `diff_pair` is routed **as a pair**:

- **Priority.** Both halves route in priority class 1, ahead of every
  single-ended signal net, so the pair claims a clear corridor before the
  general routing can take it.
- **Coupled run.** The negative half routes along a *coupling lane* built from
  the positive half's committed path (`CouplingLane` in
  `crates/synth-route/src/advisor.rs`): every grid cell beyond the lane's
  reach is charged a penalty that grows with its distance from the positive
  half's copper, so the A* that routes the negative half keeps it beside the
  positive — the halves run parallel at the board's clearance, and the
  negative's layer changes land in the corridor the positive's did. The bias
  is a preference, never a barrier: where the lane is blocked the negative
  half detours and rejoins.
- **Skew tuning.** The declared `max_skew` is what the length tuning is held
  to (`crates/synth-route/src/serpentine.rs`). The shorter half is tuned with
  meander loops sized to the length still missing — a loop's amplitude is
  clamped between 0.1 mm and 0.6 mm — so a `max_skew 0.15mm` budget is met
  instead of being overshot by a whole full-size loop.
- **Verdict.** The independent validation pass re-derives each declared
  pair's intra-pair skew from the delivered copper
  (`check_diff_pair_skew` in `crates/synth-router/src/validate.rs`), reading
  only the board file, never the router's report. A pair whose halves differ
  by more than its declared budget produces a **blocking** finding in the
  routing report's `blocking_reasons` — the same verdict pipeline as the
  return-via blockers, code `E-SYNTH-DRC-014` — naming the pair, the measured
  skew and the budget. A board that releases with 9.9 mm of skew on a
  `max_skew 0.15mm` pair is no longer possible.

What is still advisory: the coupling lane biases the negative half toward the
positive's corridor but cannot force *geometric* coupling end to end — a
detour around an obstacle is a legal outcome, and the skew budget, not the
geometry, is the hard gate.

## Via-in-pad policy

`requires via_in_pad` is a **board-level** permission because that is what the
fabricator quotes. It is not a licence to put a via in every pad. The router
uses an in-pad via only where the pad cannot be escaped beside it:

- a declared `escape { method underpad }` component, or
- a dense grid array whose arithmetic leaves no beside-pad room.

**Per-component accounting** (`absmach/synth#439`): a declared escape is not a
licence to take the whole pad field. W-SYNTH-DRC-016 demands a ground return
via within its window of every layer change, and on a part whose pads sit
closer together than a return via fits between them (the 0.8 mm TSOP case), a
via in every pad leaves the return vias nowhere to sit and the board blocks.
So the in-pad repair (`move_vias_off_component_pads` in
`crates/synth-router/src/island_vias.rs`) budgets via-in-pad per component:

- An in-pad via of a dense-pitch declared component stays only where a return
  via can sit beside it within the rule's window, at the spacing the
  return-via stitcher will use; the spot it needs is reserved, so the budget
  for later pads does not promise the same room twice.
- Where the window cannot hold a return beside the pad, the via prefers a
  non-pad layer change and moves off the pad like any other, leaving the pad
  field's neighbouring copper-free space to the return vias.
- A sparse part (pads far enough apart for a return via between them) is
  never probed and is unaffected.
- A ground via in a pad is a return via itself, not a layer change that needs
  one, and is never budgeted — a grid array keeps its in-pad vias either way.

The window the budget searches in is W-SYNTH-DRC-016's own
(`return_via_window_mm`: the stackup's thickest dielectric, floored at the
signal via's pad plus the copper clearance), less the margin the stitcher
keeps inside it, so a kept in-pad via is one whose return the stitcher can
place. What the budget cannot see is the pour itself, which does not exist
until the fill runs after the repair; a missing pour blocks the board later
for its own reason, the same as it would have without the budget.

## Microvia tier

`jlc-advanced` now carries a 0.10 mm laser drill and a 0.25 mm finished
diameter. That land fits a 0.35 mm ball at 0.8 mm pitch. A tier without a
microvia keeps the through-via land and is refused, naming the sizes.

## Follow-ups

- Pass the per-component escape set to the KiCadRoutingTools helper so the
  general router cannot use an in-pad via outside a declared under-pad
  escape.
- Full geometric coupling: hold the negative half to the positive's layer
  change points as a constraint rather than a cost bias, and bias the declared
  `diff_pair_gap` into the lane width. The skew budget is the gate until then.
