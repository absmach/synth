# E-SYNTH-NAME-010 — units of one part powered from different rails

**Severity:** error
**Stage:** erc — naming

## What this means

A multi-unit package (a dual op-amp, a quad gate) carries per-unit power
pins wired to *different* positive rails. A package has one supply, so
this is either a mis-wire or the wrong symbol. Because each unit is drawn
elsewhere in the schematic, the conflict is invisible locally.

This fires only for parts whose symbol repeats power pins per unit; when
a symbol shares one pair of power pins across all units, every unit
necessarily agrees and the rule stays quiet.

## Minimal reproduction

(with a per-unit-power dual op-amp `U1`)

```synth
board "x" {
  net "+3V3" { U1.vcc_a }
  net "+5V"  { U1.vcc_b }
}
```

## Suggested fix

Tie every unit's positive supply to the same rail.
