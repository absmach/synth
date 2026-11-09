# Reviewing a schematic by looking at it

Synth can tell you a schematic is electrically correct. It cannot tell you the drawing is readable — for that, something has to look at the picture. This guide walks you through that loop: render the sheet, look at it, fix what you see, confirm you did not break anything else, then export.

It is written for an agent, or for a human driving Synth through an MCP client. Two companion guides sit either side of it: [`schematic-procedures.md`](schematic-procedures.md) explains the pipeline this loop drives, and [`kicad-workflows.md`](kicad-workflows.md) covers the review gates after export.

## Why you have to look

The auto-layout is deterministic, and it already passes a battery of readability rules (`E-SYNTH-SCHEM-001..015`): inverted power symbols, wire-crossing density, decoupling distance, content outside the page, text-run overlap, group-region contiguity.

Those rules catch mechanical failures. They cannot catch the ones that need eyes:

- labels that overlap _in this particular_ arrangement,
- a wire that crosses a component body in a way that reads as connected,
- a sub-circuit that is logically grouped but visually scattered,
- signal flow that runs right to left.

So treat the rules as your first pass and the rendered image as your second.

> [!IMPORTANT]
> Look at the image you produced. A success status from a render command tells you the renderer ran, not that the drawing is legible.

## The tools you will use

| Tool                          | What it does                                                                                                    |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `synth_review_schematic`      | One call: compile → ERC + readability rules → render → layout summary → optional baseline diff. **Start here.** |
| `synth_render_schematic`      | Render only, for when you just want another look after a targeted edit.                                         |
| `synth_schematic_baseline`    | `set` captures a known-good render, `compare` reports drift, `clear` removes it.                                |
| `synth_mutate_layout`         | Makes one structured layout edit; `persist=true` writes it through to the sidecar.                              |
| `synth_write_layout_override` | Writes an absolute or relative placement override directly.                                                     |

You do not have to export first in order to render. The tools call `synth_kicad::export_schematic_only` into a temporary directory, plot that with `kicad-cli sch export svg`, and rasterize the result with the pinned `synth-render` stack (`RENDERER_ID = "synth-render/resvg-0.48.1"`).

That schematic-only export is byte-identical to the schematic-side files a full export writes for the same board — pinned by `crates/synth-kicad/tests/schematic_only_parity.rs` — so what you review is what ships.

> [!NOTE]
> The images come back as MCP `image` content blocks, not as base64 inside a JSON string. Inspect them directly.

## The loop at a glance

```
1. synth_validate           → zero blocking diagnostics
2. synth_review_schematic   → diagnostics + rendered sheets (look at them)
3. fix                      → synth_fix for ERC, synth_mutate_layout for layout
4. synth_review_schematic   → re-check, and compare against the baseline if you set one
5. synth_export             → release export
6. kicad-cli sch erc        → zero errors
```

### Step 1 — validate first

```
synth_validate
```

This returns both the electrical ERC findings and the `E-SYNTH-SCHEM-*` readability rules.

Resolve the electrical ones on the source. Make a note of the readability ones but do not chase them yet — step 2 is where you can actually judge them.

### Step 2 — review, and open the image

```
synth_review_schematic
```

One call gives you four things:

- `diagnostics` — ERC plus readability, with counts,
- `readability_findings` — just the `E-SYNTH-SCHEM-*` subset, so you can focus,
- `layout` — sheet size, component/wire/label/flag/junction counts, and group names,
- `render.sheets[]` — one image per sheet, with real pixel dimensions.

Now open the image. Accept the render only when, in the picture itself:

- related components read as one visual block,
- no two text runs collide,
- wires take short, plausible paths and do not cross unrelated bodies,
- signal flow runs left to right,
- nothing sits past the page edge.

### Step 3 — fix what you saw

There are two kinds of fix, and picking the wrong one is the usual mistake.

**Source fixes** — connectivity, missing values, missing decoupling. Use `synth_fix`, or edit the `.synth` file and re-validate.

**Layout fixes** — visual only. Use `synth_mutate_layout` with `persist=true`, or `synth_write_layout_override`. The supported operations are:

| Operation          | What it does                                                                        |
| ------------------ | ----------------------------------------------------------------------------------- |
| `move_component`   | Moves one component.                                                                |
| `rotate_component` | Rotates one component.                                                              |
| `group_components` | `GroupBlock` — pulls scattered parts into a tidy column beside an anchor.           |
| `set_net_style`    | `ReplaceWireWithLabel` — forces a net to render as labels instead of one long wire. |
| `distribute_row`   | Lays parts out left to right in signal order.                                       |
| `fit_sheet`        | Shrinks the page to the content.                                                    |
| `reroute_net`      | Reroutes a net.                                                                     |

#### Let the repair hints do the work

`synth_review_schematic` does not stop at naming a defect. For failures it can mechanise, it returns a `repair_hints[]` entry with a `problem` string and a ready-to-apply `suggested_op`. Feed that op straight back into `synth_mutate_layout`.

Two hints exist today:

| Finding                                                                                                     | Suggested op                                                       |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `E-SYNTH-SCHEM-012` (content covers only a corner of the page)                                              | `fit_sheet` — shrink the page, including to a custom size below A4 |
| Signal flow (3–8 parts, and no row holds more than half of them: a column, a diagonal staircase, a scatter) | `distribute_row` — a left-to-right row in signal order             |

> [!NOTE]
> The fill-ratio repair deliberately **shrinks the sheet rather than spreading the parts.** The standard A4/A3/A2 ladder has nothing below A4, so a three-part design on A4 is always mostly empty; stretching the parts to fill the page only draws long wires across blank paper. A human drawing three parts reaches for a small sheet instead.

So `fit_sheet` returns a rounded `Custom` page — 130 × 115 mm for the canonical LED indicator — and the diagnostic clears. The page is sized around the drawing _including_ its Reference and Value fields, is never narrower than KiCad's 110 mm title block plus frame, and is exported as `(paper "User" W H)`. It then centres the drawing in the area above the title block with one rigid, grid-snapped translation, so the slack is not all dumped on the right and bottom.

#### Where persisted edits go

Persisted sheet positions go to `<design>.schematic.layout.toml`. The `.synth` source stays the source of truth for connectivity.

> [!IMPORTANT]
> Do not confuse this with `<design>.placement.layout.toml`, which holds _board_ millimetres for the PCB. Same schema, different coordinate space — passing one where the other is expected silently misplaces parts. The older shared `<design>.synth.layout.toml` is deprecated and is never written to.

> [!NOTE]
> `fit_sheet` persists as `fit_sheet = true` and is re-applied — both size and centring — after the auto-layout, because the pipeline would otherwise re-derive the sheet size from scratch. Component overrides in the sidecar stay in the auto-layout's un-centred coordinates; the centring shift moves them along with everything else.

### Step 4 — confirm you did not break anything

Before a batch of edits, capture a known-good render:

```
synth_schematic_baseline action=set
```

After the edits, compare against it — either `synth_schematic_baseline action=compare` or `synth_review_schematic compare_baseline=true`. You get `pass` or `drift`, with the bounding box of the changed pixels.

Drift is measured against the **drawing** (content pixels), not the blank page, so a small real change is not diluted into nothing by a large empty sheet.

> [!NOTE]
> A baseline recorded by a different renderer is flagged rather than silently trusted.

### Steps 5 and 6 — export, then ERC

```
synth_export
kicad-cli sch erc
```

`synth_export` reruns placement and routing from the exact board and sidecar, and rejects unresolved diagnostics, incomplete routing, or DRC violations. Then run `kicad-cli sch erc` and require zero errors.

## When is it actually done?

Do not claim the schematic is finished until all four of these hold:

1. `synth_validate` reports zero blocking diagnostics.
2. The rendered image shows coherent grouping, no label or symbol overlap, and no content outside the page.
3. Every finding you chose not to fix has a stated reason — an intentional waiver, or a design decision awaiting user input.
4. The export and `kicad-cli sch erc` gates are both green.

> [!WARNING]
> If a required check could not run — `kicad-cli` is not installed, so no render is possible — report the result as **incomplete** and name the blocked evidence. Do not present a partial review as a clean one.

## Why the renders are reproducible

Two details, in case you are wondering whether an image you got yesterday is comparable to one from today:

- **The rasterizer consults no system fonts.** KiCad draws schematic text as stroke-font paths, so in practice this costs you nothing. If a visible `<text>` element does appear, it is reported in `warnings[]` rather than rendered inconsistently from one machine to the next.
- **The renderer version is pinned exactly** in the workspace manifest and recorded in every baseline. A library bump therefore shows up as a renderer change, not as design drift.
