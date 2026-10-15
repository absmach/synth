# W-SYNTH-DRC-021 — EMC placement

**Severity:** warning
**Stage:** drc

## What this means

One of two placement findings: a crystal or oscillator further than 20 mm from
the part it connects to (its loop is long and its edges radiate), or a switching
converter with no input capacitor within 12 mm on any of its nets (its hot loop
is large). Geometry `kicad-cli` does not judge, evaluated on the placement the
export chose.

## Minimal reproduction

A crystal placed on the far side of the board from its MCU.

## Suggested fix

Move the crystal next to its MCU, or the input capacitor next to the
converter's input pin.
