# Bundled agent assets

`synth init` installs the same agent guidance the binary was built with, so a
project does not start by pasting prompts:

```sh
synth init --client claude   # ~/.claude/agents and ~/.claude/skills
synth init --client codex    # ~/.agents/skills
synth status [--client claude|codex] [--json]
synth uninstall --client claude
```

Installation is idempotent: a file whose content already matches is not
rewritten. `synth status` reports `missing`, `current` or `modified` for every
owned file and exits `3` when anything is missing or has drifted.

## `synth-design-review`

Runs the deterministic gates first — validate/ERC, placement visual review,
schematic render review, DRC/SI — then reviews what they cannot see:
protection on exposed interfaces, unused pins, thermal paths and mechanical
fit. Findings are CRITICAL/WARNING/SUGGESTION with evidence. The report ends
with a PASS/FAIL/BLOCKED/N/A checklist and one verdict: `READY`, `NOT_READY`
or `INCOMPLETE`.

A finding is CRITICAL only when a gate, a datasheet or connectivity backs it;
heuristics are warnings. A required check that could not run makes the verdict
`INCOMPLETE`, never a pass.

## `synth-design-build`

Registry-first design from requirements: copy a reference module 1:1 and
record every deviation, validate/fix to zero blocking diagnostics, review
placement and schematic, route and DRC, then report the unified verdict and
date each decision in project notes.
