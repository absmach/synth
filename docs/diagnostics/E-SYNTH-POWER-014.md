# E-SYNTH-POWER-014 — thin worst-case margin on a feedback divider

**Severity:** warning
**Stage:** erc — power

## What this means

The worst-case output of an adjustable regulator's feedback divider stays
inside the load's supply window, but closer to an edge than the margin
threshold. Ageing, a load step or a reference outside its data sheet test
conditions can use up what is left.

The analysis is the one [`E-SYNTH-POWER-013`](E-SYNTH-POWER-013.md)
describes. The margin is the smaller distance from the worst-case output to
either window edge, as a fraction of the nominal output.

## Threshold

The default threshold is 0.5 % of the nominal output. A regulator sets its
own threshold with `vout_margin`:

```synth
component U1: regulator "lmr33640" vout_margin "1%"
```

## Suggested fix

Tighten the resistors. The message names the loosest tolerance class that
fits the window. You can also choose a regulator with a tighter reference.
