# Reference designs

Canonical SynthSpec fixtures used as golden tests across the workspace.

Each `*.synth` file is paired with expected outputs as the pipeline grows:

- `*.ast.json` — expected AST (added in Phase 1)
- `*.ir.json` — expected semantic IR (added in Phase 2)
- `*.erc.json` — expected ERC diagnostics (added in Phase 3)
- `*.kicad/` — expected KiCad export (added in Phase 4)

Phase 0 ships only the smallest possible fixture, [`hello.synth`](hello.synth),
which exercises the end-to-end CLI contract: parse, emit zero diagnostics, exit 0.
