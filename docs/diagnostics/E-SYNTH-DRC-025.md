# E-SYNTH-DRC-025 — copper too close for its voltage

**Severity:** error (blocking)
**Stage:** drc — routed board

## What this means

Two conductors are closer than the voltage between them allows. The flat
manufacturer minimum (`E-SYNTH-DRC-002`, `kicad-cli` clearance) is the spacing
a fab can etch. It says nothing about the spacing a 12 V or 48 V rail needs
from ground, from a mounting hole or from the board edge.

The rule gives every net a working voltage. It looks up the spacing the
difference needs and measures the routed `.kicad_pcb`:

- copper to copper of another net, on every copper layer, including zone
  fills and inner layers;
- copper to the wall of a hole with no net (an NPTH, a mounting hole), on
  every layer the hole passes through. Such a hole counts as chassis at 0 V;
- copper to the board edge, also chassis at 0 V.

The message names both nets, their voltages, the measured and required
spacing, the layer and the position.

## Where the voltage comes from

1. A netclass `voltage` (`netclass "VIN" { voltage 12v }`).
2. The rail voltage power-domain inference finds: a `power "VIN" 12v`
   declaration, a regulator output, or a rail name such as `+12V`.
3. Ground is 0 V. A net with no voltage is measured as 0 V against a known
   rail, which is the conservative difference.

## Unprotected rails

A rail with a pin on a connector (`connector`, `jack`, `terminal_block`) is
unprotected: nothing on the board limits a surge or hot-plug transient on it.
The net past a fuse or eFuse does not touch the connector, so it is
protected. A netclass overrides the inference with `protected` (fused
off-board) or `unprotected`.

An unprotected rail at or above `unprotected_min_v` (6 V by default) adds
`unprotected_margin_mm` (0.2 mm by default) to the spacing. A 5 V USB rail
is a limited-energy source and gets no margin by default.

## The table

The default is IPC-2221B Table 6-1. Column B1 applies to inner layers. Outer
layers use B2 (bare copper, sea level to 3050 m), or B4 with
`coating = "polymer"`. Past 500 V the table gives spacing per volt.

| ΔV (V) | B1 internal | B2 external | B4 coated |
|--------|-------------|-------------|-----------|
| 0–15 | 0.05 | 0.1 | 0.05 |
| 16–30 | 0.05 | 0.1 | 0.05 |
| 31–50 | 0.1 | 0.6 | 0.13 |
| 51–100 | 0.1 | 0.6 | 0.13 |
| 101–150 | 0.2 | 0.6 | 0.4 |
| 151–170 | 0.2 | 1.25 | 0.4 |
| 171–250 | 0.2 | 1.25 | 0.4 |
| 251–300 | 0.2 | 1.25 | 0.4 |
| 301–500 | 0.25 | 2.5 | 0.8 |
| > 500, per V | 0.0025 | 0.005 | 0.00305 |

A profile TOML can change all of it:

```toml
[voltage_clearance]
coating = "polymer"
unprotected_margin_mm = 0.3
unprotected_min_v = 9.0
table = [
  { up_to_v = 15, internal_mm = 0.1, external_mm = 0.2, coated_mm = 0.1 },
]
```

## When the rule is silent

The rule reports only where the voltage asks for more than the flat floor
the other rules already enforce: copper clearance, drill-to-copper, or
copper-to-edge. A board with no voltage information gets no finding. A
3.3 V or 5 V board needs nothing past the fab floor.

A zone that has not been filled has no copper to measure. The export writes
the same spacing into `<board>.kicad_dru` as `clearance`, `hole_clearance`
and `edge_clearance` rules for each net that needs them, for outer and inner
layers. KiCad's fill and `kicad-cli pcb drc` then keep it.

The rules are sized the way this check sizes the spacing. A pair of nets
whose voltages are both known gets its own rule, sized by their difference:
two 48 V rails side by side need only the flat floor, not the 0.6 mm each
needs against chassis. Against a net of unknown voltage, a hole and the board
edge, a net is sized against chassis (0 V). A rule never goes below the net's
own netclass clearance, which a KiCad custom rule would otherwise replace. A
`'` in a net name is matched as the wildcard `?`, since KiCad's rule
expressions have no escape for it.

## Minimal reproduction

A 12 V rail from a DC jack, with an inner GND pour 0.1 mm from the jack pin:

```synth
component J1: connector "dc_jack"
power "VIN" 12v { J1.vin U1.vin }
```

B1 gives 0.05 mm and the unprotected margin adds 0.2 mm, so 0.25 mm is
needed. The pour at 0.1 mm is reported.

## Suggested fix

Increase the spacing: route the rail further away, or give the pour or the
rail's netclass a larger `clearance`. A fused rail can say `protected` in its
netclass. Do not set `protected` on a rail that is not fused.
