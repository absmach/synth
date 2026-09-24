# E-SYNTH-SCHEM-003 — decoupling capacitor separation

**Severity:** warning
**Stage:** schematic aesthetic ERC

## What this means

A decoupling capacitor sits too far from the IC it decouples for a reader
to see the two as one sub-circuit.

Two things about *how* this is measured matter, because both changed and
neither is what the original §7.7.7 wording ("> 15 mm") described:

**It is a body gap, not a centre distance.** The budget is the empty
space between the capacitor's symbol body and the IC's, zero when they
overlap. Measuring centre to centre is incoherent across part sizes: a
15 mm centre budget allowed roughly a 5 mm gap beside a small AMS1117 and
was *unsatisfiable* beside a stock LQFP-48, whose symbol is ~25 × 55 mm
and whose half-diagonal alone exceeds it. The default is therefore
restated as **25 mm of body gap** — about ten grid steps, the distance at
which a reader stops grouping the two parts. The old and new numbers are
not comparable.

**A capacitor is judged against one IC only.** A shared rail reaches
every part on the board, so sweeping every (cap, IC) pair on it reported
one misplaced capacitor N times and produced a set of findings no
placement could satisfy at once. The capacitor is attributed to the IC it
sits *nearest* on that rail — the same ownership
`patterns::ic_block::attach_orphan_rail_caps` uses when it forms the
cluster, so the rule judges the pairing the placer actually built.

The rule measures the drawing, not the netlist: it fires whether the
connection is drawn as a wire or mediated by power symbols.

## Minimal reproduction

```
board "far_cap" {
  layers 2
  component U1: mcu "stm32f103c8"
  component C1: capacitor "c_generic_0603" value "100nF"
  component R1: resistor "r_generic_0603" value "10k"
  // ... enough unrelated parts to push C1 into another cluster column
  connect U1.vdd -> C1.p1
  connect U1.vss -> C1.p2
}
```

## Suggested fix

Usually nothing in the source: the layouter attaches a rail capacitor to
its heaviest consumer's cluster automatically. A finding that survives
means the capacitor could not be claimed — most often because it sits in
a **different declared `group`** from the IC, which bounds cluster
membership. Move the declaration into the IC's group, or accept the
separation and raise `decoupling_max_mm` in the design's
`<design>.synth.erc.toml` `[schematic]` section.
