# E-SYNTH-DRC-022 — the routed board could not be read

**Severity:** error
**Stage:** drc

## What this means

The signal/power-integrity pass reads the routed board back from disk to check
the copper `kicad-cli` DRC cannot see — impedance, crosstalk, differential and
bus skew, ampacity, return path, IR drop and net length. The delivered
`.kicad_pcb` was missing, unreadable, or malformed, so none of those rules ran.

This is a **blocking** finding rather than an empty (clean) result on purpose.
A gate that swallows the read error cannot tell "checked, nothing found" from
"never checked". It downgrades the release manifest and stops a release export
exactly as a real violation would; in the MCP route path it clears `drc_clean`.

## Minimal reproduction

A run where the routed board is not on disk when the pass reads it — a router
that reports a delivered path it did not write, or an export tree removed
between routing and validation.

## Suggested fix

Ensure the router's delivered `.kicad_pcb` exists and is well-formed before the
SI/PI pass runs. A path mismatch is a router-delivery bug to fix at the source;
a corrupt board is re-routed. `--force` (CLI) or `allow_incomplete=true` (MCP)
export an explicitly labelled draft without clearing the finding.
