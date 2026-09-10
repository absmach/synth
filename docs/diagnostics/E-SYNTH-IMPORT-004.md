# E-SYNTH-IMPORT-004 — import recursion limit exceeded

**Severity:** error
**Stage:** ir-lower (import resolution)

## What this means

The chain of `import` statements is `MAX_IMPORT_DEPTH = 16`
levels deep. Going deeper aborts the resolution.

## Suggested fix

Flatten the import structure. Most designs do not need more than
a handful of levels; 16 is a generous ceiling chosen to prevent
runaway resolution rather than to constrain legitimate designs.
