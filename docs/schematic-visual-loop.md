# Schematic Visual-Feedback Loop

**Scope:** how an agent (or a human through an MCP client) closes the loop
on schematic *readability* in Synth — render the sheet, look at it, refine
the layout, confirm nothing regressed, then export.

**Companion docs:** [`schematic-procedures.md`](schematic-procedures.md)
(the pipeline this loop drives) and
[`kicad-workflows.md`](kicad-workflows.md) (the post-export review gates).

---

## 1. Why a rendered loop

Synth's auto-layout is deterministic and already passes a battery of
readability rules (`E-SYNTH-SCHEM-001..015`): inverted power symbols,
wire-crossing density, decoupling distance, content outside the page,
text-run overlap, group-region contiguity. Those rules catch the
mechanical failures. They cannot catch the ones that need eyes:

- labels that overlap *in this particular* arrangement,
- a wire that crosses a body in a way that reads as connected,
- a sub-circuit that is logically grouped but visually scattered,
- signal flow that runs right-to-left.

So the loop is: **the rules give the agent a first pass, and the rendered
image gives it the second.** The agent is expected to look at the image it
produced, not just at a success status from a render command.

---

## 2. The tools

| Tool                       | What it does                                                                                                 |
| -------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `synth_review_schematic`   | One call: compile → ERC + readability rules → render → layout summary → optional baseline diff. Start here. |
| `synth_render_schematic`   | Render only, when you just want to look after a targeted edit.                                              |
| `synth_schematic_baseline` | `set` captures a known-good render; `compare` reports drift; `clear` removes it.                            |
| `synth_mutate_layout`      | One structured layout edit; `persist=true` writes it through to the sidecar.                                 |
| `synth_write_layout_override` | Directly write an absolute/relative placement override.                                                |

Rendering does **not** require you to export first: the tools call
`synth_kicad::export_schematic_only` into a temp directory, plot that with
`kicad-cli sch export svg`, and rasterize the result with the pinned
`synth-render` stack (`RENDERER_ID = "synth-render/resvg-0.48.1"`). The
schematic-only export is **byte-identical** to the schematic-side files a
full export writes for the same board (pinned by
`crates/synth-kicad/tests/schematic_only_parity.rs`), so what you review is
what ships.

The images arrive as MCP `image` content blocks, not base64 inside a JSON
string — inspect them directly.

---

## 3. The workflow

```
1. synth_validate              → zero blocking diagnostics
2. synth_review_schematic      → diagnostics + rendered sheets (look at them)
3. fix                       → synth_fix for ERC, synth_mutate_layout for layout
4. synth_review_schematic      → re-check; compare against the baseline if set
5. synth_export                → release export
6. kicad-cli sch erc           → zero errors
```

### Step 1 — validate

`synth_validate` returns both the electrical ERC findings and the
`E-SYNTH-SCHEM-*` readability rules. Resolve the electical ones on the
source; note the readability ones, because step 2 will let you judge them
visually.

### Step 2 — review and look

`synth_review_schematic` gives you, in one call:

- `diagnostics` (ERC + readability) with counts,
- `readability_findings` (the `E-SYNTH-SCHEM-*` subset, for focus),
- `layout` — sheet size, component/wire/label/flag/junction counts, group
  names,
- `render.sheets[]` — one image per sheet with real pixel dimensions.

**Open the image.** Accept the render only when, in the picture:

- related components read as one visual block,
- no two text runs collide,
- wires take short, plausible paths and do not cross unrelated bodies,
- signal flow runs left to right,
- nothing sits past the page edge.

### Step 3 — refine

Two kinds of fix:

- **Source fixes** (connectivity, missing values, missing decoupling):
  `synth_fix` or edit the `.synth` and re-validate.
- **Layout fixes** (visual only): `synth_mutate_layout` with
  `persist=true`, or `synth_write_layout_override`. Supported ops:
  `move_component`, `rotate_component`, `group_components`
  (`GroupBlock` — pull scattered parts into a tidy column beside an
  anchor), `set_net_style` (`ReplaceWireWithLabel` — force a net to
  render as labels instead of a long wire), and `reroute_net`.

Persisted placement goes to `<design>.synth.layout.toml`; the `.synth`
source stays the source of truth for connectivity.

### Step 4 — confirm no regression

Before a batch of edits, `synth_schematic_baseline action=set`. After the
edits, `compare` (or `synth_review_schematic compare_baseline=true`)
reports `pass` or `drift` with the changed-pixel bounding box. Drift is
measured against the **drawing** (content pixels), not the blank page, so
a small real change is not diluted to nothing. A baseline recorded by a
different renderer is flagged rather than silently trusted.

### Steps 5–6 — export and ERC

`synth_export` reruns placement/routing from the exact board and sidecar,
and rejects unresolved diagnostics, incomplete routing, or DRC violations.
Then run `kicad-cli sch erc` and require zero errors.

---

## 4. Completion bar

Do not claim the schematic is done until:

- `synth_validate` reports zero blocking diagnostics;
- the rendered image shows coherent grouping, no label/symbol overlap, and
  no content outside the page;
- every finding you chose not to fix has a stated reason (an intentional
  waiver, or a design decision awaiting user input);
- the export and `kicad-cli sch erc` gates are green.

If a required check could not run (for example `kicad-cli` is not
installed, so no render is possible), report the result as **incomplete**
and name the blocked evidence — do not present a partial review as a clean
one.

---

## 5. Determinism notes

- The rasterizer consults **no system fonts**. KiCad draws schematic text
  as stroke-font paths, so this is not a practical limitation; a visible
  `<text>` element is reported in `warnings[]` rather than rendered
  inconsistently across machines.
- The renderer version is pinned exactly in the workspace manifest and
  recorded in every baseline, so a library bump is detected as a renderer
  change, not design drift.
