# E-SYNTH-DRC-024 — connector mating plane off the board edge

**Severity:** error (blocking)
**Stage:** drc — connector placement

## What this means

An edge-mounted connector faces the right way, but its mating plane does not meet the board edge. A recessed receptacle cannot be reached by a plug. A connector that hangs past the edge sticks out of the board.

`E-SYNTH-DRC-011` checks only the direction the opening faces. This rule checks how far the opening sits from the edge.

## How the check works

The mating plane is the connector body face on its mating side. The courtyard is centred on the placement. The part's `footprint_dimensions.courtyard_margin_mm` is the rim the courtyard adds around the body, so the plane sits one margin inside the courtyard edge.

A connector with its courtyard flush on the edge is the reference position. The check accepts a plane within one `courtyard_margin_mm` of that position, in either direction. A part that declares no margin gets 0.05 mm, which only absorbs grid rounding.

| Part | `courtyard_margin_mm` | Source |
|------|-----------------------|--------|
| USB-C receptacle | 0.25 | `registry/parts/connectors/usb_c_receptacle.synth.toml` |
| RJ45 jack | 0.5 | `registry/parts/connectors/rj45_hr911105a.synth.toml` |

The tolerance comes from the registry margin. It is not a datasheet value. Mid-mount parts that need a specific overhang are not modelled, because the registry has no per-part overhang field yet.

The rule uses the same edge as `E-SYNTH-DRC-011`: the edge named by a hard `edge:` hint, or the nearest edge within the connector's courtyard half-depth plus 6 mm. A connector in the board interior is not judged. A connector that faces inward is reported by `E-SYNTH-DRC-011` only.

## Minimal reproduction

```synth
board "b" {
  layers 2
  component J1: connector "usb_c_receptacle"
}
```

Place J1 facing the bottom edge with its body face 1.71 mm inside the outline. The rule reports the distance in mm.

## Suggested fix

Apply the suggested override to the named connector. It moves the part along the edge normal so its courtyard sits flush with the outline. The rotation does not change.

If the connector must stay where it is, move the board outline instead.
