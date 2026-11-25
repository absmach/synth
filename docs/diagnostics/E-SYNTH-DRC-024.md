# E-SYNTH-DRC-024 — connector mating plane off the board edge

**Severity:** error (blocking)
**Stage:** drc — connector placement

## What this means

An edge-mounted connector faces the right way, but its mating plane does not meet the board edge. A recessed receptacle cannot be reached by a plug. A connector that hangs past the edge sticks out of the board.

`E-SYNTH-DRC-011` checks only the direction the opening faces. This rule checks how far the opening sits from the edge.

## How the check works

### A part that declares its mating plane

A part can state where its plane is and how far past the edge it may sit,
under `[footprint_dimensions]`:

```toml
mating_face = "bottom"
# Distance from the footprint origin to the mating plane, along mating_face.
mating_plane_offset_mm = 3.65
# Allowed [min, max] overhang past the board edge: 0 is flush, positive
# hangs past the edge, negative is recessed. Defaults to [0, 0].
mating_overhang_mm = [-0.5, 0.5]
```

The rule places the plane from the footprint origin after rotation and
measures it against the edge. It fails when the overhang is outside the range
by more than 0.05 mm. The suggested override moves the part to the nearest
bound, so the delta is the exact distance out of range. The outline fit docks
such a part on its plane too. It keeps the courtyard-flush edge when that is
already in range, and otherwise moves the edge so the plane is in range.

| Part | Plane offset | Overhang range |
|------|--------------|----------------|
| USB-C receptacle (HRO TYPE-C-31-M-12) | 3.65 mm | -0.5 to 0.5 mm |
| RJ45 jack (Hanrun HR911105A) | 17.26 mm | -0.5 to 0.5 mm |
| JST PH 2/3/4-pin, side entry | 6.25 mm | -0.5 to 0.5 mm |
| JST XH 2/4-pin, side entry | 7.6 mm | -0.5 to 0.5 mm |

The offsets are the body fronts on the stock footprints' F.Fab layer. Each
stock courtyard runs 0.5 mm past its body front, so a courtyard-flush part
has its plane 0.5 mm in. The ranges accept that recess and an overhang of the
same size. They are read from the footprints, not from a mechanical drawing.
A mid-mount part that needs a specific overhang states its own range.

### A part without a declared plane

The mating plane is the connector body face on its mating side. The courtyard is centred on the placement. The part's `footprint_dimensions.courtyard_margin_mm` is the rim the courtyard adds around the body, so the plane sits one margin inside the courtyard edge.

A connector with its courtyard flush on the edge is the reference position. The check accepts a plane within one `courtyard_margin_mm` of that position, in either direction. A part that declares no margin gets 0.05 mm, which only absorbs grid rounding.

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

Apply the suggested override to the named connector. It moves the part along the edge normal. A part with a declared plane moves to the nearest end of its overhang range. Any other part moves until its courtyard sits flush with the outline. The rotation does not change.

If the connector must stay where it is, move the board outline instead.
