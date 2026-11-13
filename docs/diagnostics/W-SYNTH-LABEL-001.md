# W-SYNTH-LABEL-001 — net name label has no clear segment

**Severity:** warning
**Stage:** export (kicad)

## What this means

The schematic exporter places a net's name label on the longest segment
of the net that has a spot clear of junctions, same-net branch endpoints
and other nets' wires. When no segment of the net qualifies, the label
falls back to the longest segment's midpoint — the spot that once merged
a net into the one it was flagged for — and the exporter emits this
warning instead of doing it silently.

The label is still placed (a named net has to carry its name), but the
board should not be trusted until the placement has been looked at.

## Minimal reproduction

A named net whose every segment is either crossed by another net's wire
or passes through a junction or branch endpoint:

```synth
board "x" {
  component U1: ic "sn74hc595_shift"
  component U2: ic "sn74hc165_pload"
  connect U1.ds -> U2.ser as "crowded"
  // every segment of `crowded` shares its length with a crossing
}
```

## Suggested fix

Give the net some clear wire to label — reroute one segment away from
the crossings and junctions — or move the label by hand after export.
