# E-SYNTH-KEEPOUT-003 — keepout with nothing to centre on

**Severity:** warning
**Stage:** erc — geometry

## What this means

A `keepout` is a radius around a place, and no declared component
matches the keepout's explicit `anchor "REFDES"` — or, when it declares
no anchor, its own name. The exporter therefore has no geometry to
write: the keepout is not exported. (It used to be placed silently at
the board centre, which is a corridor over whatever happens to sit in
the middle of the board.)

## Minimal reproduction

```synth
board "x" {
  layers 2
  component U1: mcu "atmega328p"
  keepout tmds_corridor { radius 6mm }
}
```

`U1` is the only component and the keepout is named
`tmds_corridor`, which matches no refdes and no kind.

## Suggested fix

Point the keepout at the component it guards:

```synth
keepout tmds_corridor { radius 6mm anchor "U1" }
```

or rename the keepout to the component's refdes or part kind, which is
the implicit anchor.
