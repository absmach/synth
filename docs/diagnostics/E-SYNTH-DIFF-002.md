# E-SYNTH-DIFF-002 — diff_pair self-reference

**Severity:** error
**Stage:** erc — connectivity

## What this means

A `diff_pair` named the same net for both positive and negative legs. A differential pair by definition routes two distinct signals.

## Minimal reproduction

```synth
board "x" {
  diff_pair dp dp { impedance 90ohm }
}
```

## Suggested fix

Correct the typo so the two leg names refer to different nets.
