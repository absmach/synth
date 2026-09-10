# E-SYNTH-IMPORT-001 — imported file not found

**Severity:** error
**Stage:** ir-lower (import resolution)

## What this means

An `import "<path>"` statement referenced a file the loader could
not read. Either the file does not exist under the sandbox root,
or the I/O failed for some other reason (permissions, etc.).

## Minimal reproduction

```synth
import "missing.synth"
board "x" {}
```

## Suggested fix

Check the spelling and make sure the file exists relative to the
project root (the loader does not allow `../` segments — see
`E-SYNTH-IMPORT-002`).
