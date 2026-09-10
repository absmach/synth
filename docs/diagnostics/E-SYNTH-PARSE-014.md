# E-SYNTH-PARSE-014 — expected import path (quoted string)

**Severity:** error
**Stage:** parse

## What this means

An `import` statement is written as `import "<path>"`. The parser found
something other than a quoted string after `import`.

## Minimal reproduction

```synth
import stdlib
board "x" {}
```

## Suggested fix

Wrap the path in double quotes.
