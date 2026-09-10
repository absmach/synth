# E-SYNTH-PARSE-019 — unexpected attribute inside diff_pair

**Severity:** error
**Stage:** parse

## What this means

Phase 1 recognizes only `impedance <value><unit>` inside a `diff_pair`
body. Any other token is rejected. Later phases will add length matching,
skew tolerance, and routing-priority attributes.

## Minimal reproduction

```synth
board "x" {
  diff_pair USB_DP USB_DN {
    speed fast
  }
}
```

## Suggested fix

Remove the unsupported attribute or replace it with a recognized one.
