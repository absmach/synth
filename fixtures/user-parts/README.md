# Fixture: User-Imported Test Parts

This directory contains component definitions used to simulate user-imported and custom parts in unit and integration tests.

## Purpose

1. **Simulate User Import:** Test `synth part import lcsc` and `synth part import kicad` workflows without modifying the core compiler registry.
2. **Tiered Registry Testing:** Test `synth_registry::load_tiered()` to verify that project-local (`./parts/`) and user-tier parts overlay cleanly onto the core `registry/parts/` corpus.
3. **Shadowing & Precedence:** Verify that user parts can extend or override core library definitions with appropriate warnings.

These files are test fixtures and are intentionally separate from `registry/parts/`.
