# E-SYNTH-COMP-001 — unknown part

**Severity:** error
**Stage:** resolve

## What this means

The component declaration references a part id that does not exist in
the loaded registry (`registry/parts/**/*.synth.toml`).

## Minimal reproduction

```synth
board "x" {
  component U1: mcu "rp9999"
}
```

## Suggested fix

The resolver computes the Levenshtein distance between the requested
id and every part id in the registry and suggests the three closest
matches (within distance 3). Each suggestion ships as a
`replace_range` patch (`suggested_fixes`) that an agent can apply directly.

When the id genuinely isn't a typo — the part just doesn't exist in the
registry yet — text edits can't fix that, so the diagnostic also carries
`suggested_actions` (Phase 15, §18.8.4): non-textual next steps that call an
MCP tool / CLI command instead of patching `board.synth`:

| `kind` | Tool call | When |
| :--- | :--- | :--- |
| `search_registry_web` | `synth_search_registry_web` / `synth part search` | Always offered first — check LCSC for a matching part |
| `import_part_stub` | `synth_import_part` / `synth part import lcsc\|kicad` | Once search turns up a source id |
| `create_part_stub` | `create_part_stub` / `synth part stub <id> --pins <N>` | No source match — write a required-pins-relaxed skeleton and fill it in from a datasheet |

The full unknown-part design loop: search → import (if found) or stub (if
not) → fill in real pins from a datasheet via `synth_author_part` →
revalidate. Newly authored/imported parts land in the Tier-2 user registry
unreviewed, so they also carry `W-SYNTH-PART-UNVERIFIED`
(`docs/diagnostics/W-SYNTH-PART-UNVERIFIED.md`) until a human reviews them.
