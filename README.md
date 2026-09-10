# Synth

Synth is an open-source EDA compiler for describing electronic designs in SynthSpec and producing KiCad artifacts. It provides a typed compiler pipeline, electrical and design-rule checks, deterministic placement and routing, a component registry, and tools for agent-assisted hardware design.

## Features

- SynthSpec parser, AST, and typed board IR
- Machine-readable diagnostics and structured repair suggestions
- Electrical-rule checking, design-rule checking, and SMT-backed fixes
- Deterministic placement and multilayer routing
- KiCad schematic, PCB, BOM, and fabrication export
- Versioned component registry with project and user overlays
- CLI, local web preview, and Model Context Protocol server

## Quick Start

Install a current Rust toolchain, then build the workspace:

```bash
cargo build --workspace
cargo run -p synth-cli -- --help
```

Compile a design from the included fixtures:

```bash
cargo run -p synth-cli -- check fixtures/designs/hello.synth
cargo run -p synth-cli -- export fixtures/designs/hello.synth --out-dir output
```

## Development

```bash
make verify
```

The component registry lives in `registry/parts`. Example designs and golden fixtures live in `fixtures/` and `examples/`.

## Documentation

- `docs/protocol-v1.0.md`: diagnostic wire format
- `docs/kicad-workflows.md`: KiCad review and fabrication workflow
- `docs/schematic-procedures.md`: schematic generation procedures
- `docs/diagnostics/`: diagnostic reference

## License

Licensed under Apache-2.0. See `LICENSE`.
