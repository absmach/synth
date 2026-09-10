# Diagnostics

One markdown page per stable diagnostic code.

Each page MUST follow this shape:

```markdown
# E-SYNTH-XXX-NNN — short title

**Severity:** error | warning | info | fatal
**Stage:** parse | semantic | erc | place | route | drc | mfg

## What this means
One paragraph explaining the rule.

## Minimal reproduction
A small `.synth` snippet that triggers it.

## Suggested fix
The patch primitive most commonly applied.
```

The build fails if any code emitted from source has no corresponding doc
page, or if a page exists for a code never emitted.
