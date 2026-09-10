# E-SYNTH-KEEPOUT-001 — keepout missing radius

**Severity:** warning
**Stage:** erc — geometry

## What this means

A `keepout` block has no `radius` attribute. Routers and manufacturing exports cannot represent a zero-area exclusion, so the keepout has no effect.

## Minimal reproduction

```synth
board "x" {
  keepout antenna_zone {}
}
```

## Suggested fix

Add a `radius <value><unit>` line inside the keepout body.
