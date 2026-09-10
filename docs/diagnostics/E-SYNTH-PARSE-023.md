# E-SYNTH-PARSE-023 — unknown engineering unit

**Severity:** error
**Stage:** parse (lexer)

## What this means

A numeric literal followed by letters is interpreted as a value with an
engineering unit. Phase 1 recognizes:

- Length: `mm`, `mil`
- Resistance: `ohm`, `kohm`, `mohm`
- Voltage: `v`, `mv`
- Current: `a`, `ma`
- Frequency: `mhz`, `ghz`
- Capacitance: `pf`, `nf`, `uf`

## Minimal reproduction

```synth
board "x" {
  diff_pair A B { impedance 90parsec }
}
```

## Suggested fix

Replace the unit with a recognized one.
