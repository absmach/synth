# synth-web

Leptos + WASM browser viewer for the Synth EDA compiler.

This crate is **not** an editor — it's a read-only viewer. You keep
editing `.synth` files in your normal IDE; the [`synth preview`
CLI command](../synth-cli/src/preview.rs) watches the file and
pushes recompiled state to the browser via Server-Sent Events.

## Architecture

```
your IDE  →  .synth file  →  synth preview file.synth  →  browser shows schematic
              ↑ you edit                ↑ watches file
              ↑ you save                ↑ pushes updates (SSE)
```

The browser receives `BoardView { source_path, board, diagnostics }`
JSON on every save. The Leptos app deserializes it through the same
serde types used server-side (no two-language drift), then renders
SVG reactively.

## Build

Prerequisites:

- `rustup target add wasm32-unknown-unknown`
- `cargo install trunk` (and `wasm-bindgen-cli` + `wasm-opt` —
  Trunk fetches both on first run)

Build the bundle:

```bash
cd crates/synth-web
trunk build --release
```

That produces `crates/synth-web/dist/` containing
`index.html`, `*.wasm` (~350 KB), `*.js` (wasm-bindgen shim),
and the CSS asset.

## Run

From the workspace root:

```bash
synth preview fixtures/ir/two_components_with_net.synth
```

Defaults: binds to `127.0.0.1:8080`, registry at `registry/parts`,
assets at `crates/synth-web/dist`. Override with `--port`,
`--registry`, `--assets-dir`.

Open the printed URL in a browser. Save the source file in your
editor; the browser updates within ~150 ms.

## V1 scope and follow-ups

In: schematic SVG, diagnostics list, SSE live updates, A4 layout
mirroring `synth-kicad`.

Not in V1 (explicit follow-ups):

- Hover/click inspectors for components and nets.
- Editor jump (`vscode://`, `cursor://`) from diagnostic locations.
- Single-binary deploy (currently `synth preview` reads
  `dist/` off disk; embedding via `include_dir!` is a one-line
  change once the bundle layout stabilises).
- Multi-page schematics.
- Pin-orientation grouping by capability (USB / power / GPIO).
