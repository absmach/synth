# E-SYNTH-IMPORT-002 — import path escapes sandbox

**Severity:** error
**Stage:** ir-lower (import resolution)

## What this means

Import paths are validated against a sandbox: they must be
relative paths with no `..` segments and no absolute prefix.
This prevents a malicious or accidental import from reading
arbitrary files on the host (e.g., `/etc/passwd` or
`../../home/user/.ssh/id_rsa`).

## Minimal reproductions

```synth
import "../somewhere/else.synth"   // `..` rejected
import "/etc/passwd"               // absolute rejected
```

## Suggested fix

Move the imported file under the project root and import it via a
relative path that doesn't traverse upward.
