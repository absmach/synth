# W-SYNTH-DRC-025 — voltage clearance not measured on an unfilled pour

**Severity:** warning
**Stage:** drc — routed board

## What this means

The board has a net whose working voltage needs more spacing than the flat
manufacturer minimum (see `E-SYNTH-DRC-025`), and a copper pour on it declares
an outline but carries no `filled_polygon`. KiCad writes the poured copper only
after a zone refill, so there is no copper to measure. The voltage-aware
clearance to that pour is unknown. It has not passed.

Rule areas (`keepout`) never pour and are not reported.

## How to fix

Refill the zones and run the check again: press `B` in KiCad's PCB editor, or
run `kicad-cli pcb drc --refill-zones`. The exported `.kicad_dru` carries the
same voltage spacing as KiCad rules, so the refill keeps it.

## Example

```
the `GND` pour on In2.Cu has no fill, so its voltage clearance was not
measured; refill the zones (KiCad `B`, or `kicad-cli pcb drc --refill-zones`)
and rerun
```
