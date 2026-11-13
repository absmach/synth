# E-SYNTH-PARSE-045 — `keepout allow` names no net

**Severity:** error
**Stage:** parse

## What this means

A `keepout` body contains `allow` but no quoted net name follows it.
`allow` lists the nets whose copper the keepout must let through, so an
empty one has nothing to say.

## Minimal reproduction

```synth
board "x" {
  keepout tmds_corridor { radius 6mm allow }
}
```

## Suggested fix

List the nets, comma-separated and quoted, the way a netclass `layers`
list is written:

```synth
keepout tmds_corridor { radius 6mm allow "TMDS_D2+", "TMDS_D2-" }
```
