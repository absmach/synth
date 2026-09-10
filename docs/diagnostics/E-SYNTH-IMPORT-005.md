# E-SYNTH-IMPORT-005 — import cycle detected

**Severity:** error
**Stage:** ir-lower (import resolution)

## What this means

The same file appears in the active import chain — file `a`
imports `b`, which imports `c`, which imports `a`. The resolver
breaks the cycle at the back-edge and emits this diagnostic.

A diamond (`a` imports `lib`; `b` imports `lib`; root imports
both `a` and `b`) is **not** a cycle and does not trigger this:
`lib` is loaded and merged exactly once via the first path, then
silently skipped on the second.

## Suggested fix

Break the cycle by removing the back-edge `import` statement.
Restructure shared definitions into a third file that both sides
import (turning the cycle into a diamond, which is fine).
