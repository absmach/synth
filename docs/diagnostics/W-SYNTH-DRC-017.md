# W-SYNTH-DRC-017 — rail drops more than the IR budget

**Severity:** warning
**Stage:** drc

## What this means

A net that declares a `current` has the resistance of its routed copper summed
from the track geometry — length, width, and the layer's copper thickness from
the stackup — and turned into an IR drop `I·R`. Above the 50 mV budget the rail
sags under load.

The sum treats the net's tracks in series, so a net with parallel paths is
over-estimated, and the budget is a default rather than the design's own. It is
advisory and needs a declared `stackup` for the copper thickness.

## Minimal reproduction

A `netclass "PWR" { current 1A }` on a net routed as 100 mm of 0.2 mm track
(about 0.24 Ω, so 240 mV).

## Suggested fix

Widen the track, shorten the run, add copper on another layer (parallel tracks
or a plane), or move the load closer to the source.
