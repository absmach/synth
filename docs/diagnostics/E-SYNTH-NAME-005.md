# E-SYNTH-NAME-005 — conflicting net names shorted together

**Severity:** error
**Stage:** lowering — named nets

## What this means

Two different explicit net names (`net "A"`, `power "B"`, or
`connect … as "…"`) ended up on one electrical net because their
endpoints are wired together. One copper net cannot have two names —
downstream export, labels, and ERC would disagree about what to call
it — so lowering reports the conflict and keeps the first-seen name.

## Minimal reproduction

```synth
board "x" {
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  component R3: resistor "r_generic_0603"
  connect R1.p1 -> R2.p1 as "+3V3"
  connect R2.p1 -> R3.p1 as "+5V"
}
```

`R2.p1` is shared, so `+3V3` and `+5V` are shorted: one diagnostic,
pointing at the second (`+5V`) declaration.

## Suggested fix

Give the connection one name (rename one side so both agree), or
split the nets so the two names live on disjoint endpoint sets.
Separate `power` declarations at different voltages additionally
trigger `E-SYNTH-POWER-007`.
