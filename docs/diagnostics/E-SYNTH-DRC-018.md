# E-SYNTH-DRC-018 — net is longer than its class allows

**Severity:** error
**Stage:** drc

## What this means

A `netclass` that declares `max_length` has a member net whose routed copper is
longer than that. This is the length half of a propagation-delay budget: on a
known stackup delay is proportional to length, so a length target states the
same thing as a delay target without a separate time unit. Only classes that
declare a length are checked.

## Minimal reproduction

```synth
board "b" {
  layers 2
  netclass "DDR" { max_length 40mm }
  # … a member net routed longer than 40 mm …
}
```

## Suggested fix

Shorten the run (move the parts closer, or change the layer), or raise the
class's `max_length` if the interface tolerates it.
