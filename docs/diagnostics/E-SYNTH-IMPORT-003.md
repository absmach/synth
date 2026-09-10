# E-SYNTH-IMPORT-003 — imported file exceeds the size cap

**Severity:** error
**Stage:** ir-lower (import resolution)

## What this means

Per plan §12.2 the import resolver caps individual files at
`MAX_IMPORT_SIZE = 1,000,000` bytes (1 MB). Larger files are
rejected without being read.

This bound prevents denial-of-service via huge files or parser
bombs in imported content.

## Suggested fix

Split the large file into smaller modules, or pre-process its
content before importing.
