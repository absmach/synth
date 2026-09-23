# E-SYNTH-CONNECT-007 — conflicting pin types on one net

**Severity:** configurable (see below)
**Stage:** erc — connectivity

## What this means

Two pins whose electrical types fight each other are driving the same
net — most often two push-pull outputs, or an output against an open-drain
pin's pull-up. The severity comes from the **configurable pin-type
conflict table** (modelled on KiCad's ERC matrix), so a project can
relax or promote any pair.

Pairs that already have a dedicated, patch-bearing rule are *not*
reported here, to avoid duplicate findings: `output`/`output`
(`E-SYNTH-CONNECT-004`), `power_output`/`power_output`
(`E-SYNTH-POWER-002`), and anything against `do_not_connect`
(`E-SYNTH-CONNECT-003`).

## Minimal reproduction

```synth
board "x" {
  component U1: ic "lm555_timer"
  connect U1.out -> U1.disch   // push-pull `output` against `open_drain_low`
}
```

Configure it from `<design>.synth.erc.toml`:

```toml
[pin_conflicts]
default = "warning"            # unlisted pairs

[pin_conflicts.pairs]
"output:output" = "info"       # relax a pair
"open_drain_low:output" = "error"
```

Key pin-type names are validated on load, so a misspelt type is an
error rather than a silently dead entry.

## Suggested fix

Remove one of the drivers, or separate them with a series resistor or a
buffer. If the pair is intentional, lower its severity in
`[pin_conflicts]`.
