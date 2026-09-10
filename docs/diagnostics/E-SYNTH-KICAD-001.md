# E-SYNTH-KICAD-001 — KiCad export I/O failure

**Severity:** error
**Stage:** export (kicad)

## What this means

The KiCad exporter could not write one of its output files. Causes
include the destination directory being unwritable, a parent
directory missing and not creatable, or the disk being full.

The exporter is *all-or-nothing* at the file-writing layer: an
error on any single file aborts before the rest are written, so
you never end up with a half-written `.kicad_sch` paired with a
stale `.kicad_pro`.

## Minimal reproduction

Pass `--out` pointing at a path the user cannot write to, e.g.:

```
synth export-kicad fixtures/ir/single_mcu.synth --out /readonly/dir
```

## Suggested fix

Pick a writable output directory.

Phase 4 does not yet emit a structured `Diagnostic` for export
I/O — the CLI prints the underlying `std::io::Error` to stderr and
exits non-zero. Promotion to a full `synth_diagnostics::Diagnostic`
with patch suggestions is a Phase 4 follow-up.
