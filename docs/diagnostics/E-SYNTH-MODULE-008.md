# E-SYNTH-MODULE-008 — fixed module parameter overridden without a deviation

**Severity:** error
**Stage:** lower

## What this means

A module parameter declared `fixed` is part of the reference the module
copies: a value from the data sheet or application note that the circuit's
behaviour depends on. An instantiation overrides it without saying why:

```synth
use "LdoAms1117" as LDO1 (c_out = 47uF) { … }
```

Copying a vetted reference 1:1 only helps if every departure from it is a
written exception, so a silent override is an error.

## Suggested fix

Use the reference value, or record the reason after the override:

```synth
use "LdoAms1117" as LDO1 (c_out = 47uF deviation "polymer part with the ESR the data sheet asks for") { … }
```

Every deviation is then listed in one place by
[`E-SYNTH-MODULE-009`](E-SYNTH-MODULE-009.md).
