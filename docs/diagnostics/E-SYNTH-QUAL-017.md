# E-SYNTH-QUAL-017 — declared pin name exists on no KiCad symbol pad

**Severity:** error
**Stage:** mfg — registry qualification

## What this means

A part declares a pin name that appears on no pad of the KiCad symbol it
names — not on the pad the number points at, not on any other pad, under
no spelling the synonym table in `crates/synth-registry/src/kicad_pin_check.rs`
recognizes. The symbol therefore verifies nothing about that pin: the
swapped-name check (`E-SYNTH-QUAL-005`) only fires when the declared name
shows up somewhere else, so without this finding an unprovable name
passed silently.

## Minimal reproduction

A part naming `Device:D_Zener` with a pin called `clamp` on pad 1, where
the symbol prints only `K` and `A`, and no synonym group covers `clamp`:

```toml
id = "subject"
kind = "diode"
kicad_symbol = "Device:D_Zener"

[[pins]]
name = "clamp"
number = "1"
electrical_type = "passive"
```

## Suggested fix

Spell the pin the way the symbol prints it, or — when the spelling is a
real datasheet alias — add it to the documented synonym group it belongs
to. A name that stays unverifiable keeps the part out of the clean
qualification list, which is the point: the check cannot be defeated by
an unusual pin name.
