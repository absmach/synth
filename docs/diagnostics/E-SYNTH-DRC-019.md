# E-SYNTH-DRC-019 — matched bus exceeds its skew budget

**Severity:** error
**Stage:** drc

## What this means

A `netclass` declares `max_skew` and its member nets are routed to lengths
whose spread (longest minus shortest) is larger than that. This is group
length matching — a parallel bus (DDR, a matched interface) — as opposed to the
per-pair skew of `E-SYNTH-DRC-014`. Nets with no copper are not counted.

## Minimal reproduction

```synth
board "b" {
  layers 2
  netclass "DDR" { max_skew 1mm }
  # … member nets routed to a 10 mm spread …
}
```

## Suggested fix

Lengthen the shortest members with serpentine or detours, shorten the longest,
or relax `max_skew`.
