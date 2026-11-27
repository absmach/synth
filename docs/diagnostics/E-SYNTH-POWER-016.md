# E-SYNTH-POWER-016 — ground domain declaration does not hold together

**Severity:** error
**Stage:** erc — power

## What this means

A `ground_domain` block declares one ground of a split ground
([docs/ground-domains.md](../ground-domains.md)). This error means the
declaration cannot be true of the design:

- the domain names a net that does not exist, or one with no ground pin and
  no ground name (`gnd`, `vss`, `0v`);
- its `join` names a part that does not exist, or one that does not have a pin
  on the domain's net and a pin on another ground, or one that is not a net
  tie (a `net_tie` part or a KiCad `NetTie` footprint) or a resistor of value
  `0R`. A capacitor or ferrite that touches both grounds is a stitch or a
  filter, not the DC join;
- two or more domains are declared and their joins do not bring them into one
  ground, so one domain floats;
- a `crossing` names a net that does not exist, or a stitching part that does
  not connect two of the domain grounds.

Synth never invents a join point: a split with no declared join is reported,
not guessed.

## Minimal reproduction

```synth
board "x" {
  layers 2
  component U1: ic "t_chip"
  component U2: ic "t_chip"
  net "AGND" { U1.gnd }
  net "DGND" { U2.gnd }
  ground_domain AGND { }
  ground_domain DGND { }
}
```

## Suggested fix

Add the part that joins the grounds, usually a net tie or a 0 ohm resistor
with one pin on each, and name it with `join <REFDES>` on the domains it
bridges. Point a `crossing … stitch` at a capacitor that sits between the two
grounds, or use `stitch join` for a signal routed over the join.
