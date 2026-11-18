# E-SYNTH-PROCESS-006 — the declared process is not offered for this stackup

**Severity:** error
**Stage:** erc

## What this means

The board's `manufacturer` resolves to a profile that the fabricator quotes
only for certain layer counts, and this board does not have one of them.

A surcharged tier is quoted against a stackup rather than sold outright.
JLCPCB offers its 0.2 mm minimum drill on the 4- and 6-layer builds; a
2-layer board cannot buy it at any price. Profile resolution matches on the
manufacturer string alone, so without this check a 2-layer board could declare
`jlc-advanced`, clear the 0.2 mm drill and via-in-pad checks against a floor
no fabricator had agreed to, and be reported fabrication-ready.

## Example

    board "sensor" {
      layers 2
      manufacturer "jlc-advanced"   // offered on 4 and 6 layers
      ...
    }

## How to fix it

Either move the board to a stackup the process is offered for:

    layers 4

or declare a tier that is offered for the stackup you have:

    manufacturer "jlc-standard"

If the second leaves a footprint undrillable — a module paddle with 0.2 mm
thermal vias, say — then the board genuinely needs the layer count, and the
first is the fix.

## What this does not report

A profile that names no layer counts is offered on every stackup, and that is
every stock tier: `jlc-standard`, `pcbway-standard` and `oshpark-4layer` carry
the fabricator's baseline floors, which do not depend on how many layers are
stacked. Only a profile that declares `supported_layers` is constrained here.

This rule says nothing about whether the geometry on the board clears the
profile's floors — that is `E-SYNTH-PROCESS-003` for declared via geometry and
the DRC stage for routed copper. It only asks whether the process can be
bought for this stackup at all.
