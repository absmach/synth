# E-SYNTH-CLOCK-001 — clock source collision

**Severity:** error
**Stage:** erc — clock

## What this means

Two `clock_output` pins are wired together. Two oscillators fighting over a single net produce undefined timing.

## Minimal reproduction

```synth
board "x" {
  component Y1: crystal "osc_smd_25mhz"
  component Y2: crystal "osc_smd_25mhz"
  connect Y1.out -> Y2.out
}
```

## Suggested fix

Pick one oscillator. If you genuinely need both clocks, route them to separate nets and use a clock mux.
