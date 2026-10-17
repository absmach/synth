# E-SYNTH-PROCESS-002 — the declared manufacturer does not offer a required process

**Severity:** error
**Stage:** erc

## What this means

The board declares `requires <process>`, and the profile its `manufacturer`
resolves to does not offer that process.

Via-in-pad is the case this exists for. On a process that fills and caps the
via it is a normal escape technique, and at fine pitch the only one the
geometry leaves. On a process that does not, the same copper wicks solder
away from the joint during reflow. The two are indistinguishable in the
exported board, so the only place the difference can be caught is here,
against the declared tier.

A requirement is not a permission. `requires via_in_pad` states what the
design needs; the profile states what the fab sells; this check is where the
two meet. An undeclared `manufacturer` resolves to the standard tier, the
same way the router and the DRC resolve it, so leaving it unstated is not a
way to reach a capability.

## Minimal reproduction

```synth
board "b" {
  layers 4
  manufacturer "jlcpcb"
  requires via_in_pad
}
```

## Suggested fix

Name a tier that offers the process — `manufacturer "jlc-filled-via"` for
via-in-pad — and expect to pay for it. Otherwise redesign so the requirement
goes away: a coarser package, more layers, or a larger escape.

Do not reach for `--krt-allow-via-in-pad`. That override exists for an
operator who has taken responsibility for a specific panel, and it silences
the gate without changing what the fab will build.
