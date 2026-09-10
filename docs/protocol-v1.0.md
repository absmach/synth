# Synth Diagnostic Protocol — v1.0

**Status:** frozen for V1.
**Schema:** [`schemas/diagnostic.schema.json`](../schemas/diagnostic.schema.json).
**Schema version field:** `"schema_version": "1.0"`.

This is the agent-facing contract for the Synth EDA compiler. Every
diagnostic emitted by any stage (lexer, parser, semantic analysis,
ERC, placement, routing, DRC, manufacturing export) conforms to the
[`Diagnostic`](#diagnostic-object) shape below.

## Scope

The protocol covers:

- The JSON shape of a single diagnostic record.
- The wire format of `synth validate --format json` output.
- The patch primitives that diagnostics may attach as
  `suggested_fixes`.
- Stable diagnostic codes (`E-SYNTH-<CATEGORY>-NNN`).
- Exit codes from CLI subcommands.

The protocol does **not** cover:

- The `Board` IR JSON (intentionally unstable through V1).
- The `.synth` source-language grammar (covered by the SynthSpec
  language reference).
- The KiCad export file shapes (those follow KiCad 8 conventions).

## Compatibility

- **Major bump (v2.0):** a field is removed or renamed; a previously
  optional field becomes required; a `PatchKind` variant is removed.
- **Minor bump (v1.1, v1.2, …):** a new optional field is added; a
  new `PatchKind` variant is added; a new diagnostic code is
  introduced. Agents pinned to v1.0 keep working.
- **Patch release (v1.0.1, …):** wording-only changes to
  `title` / `message` / `expected` / `found`. No structural change.

## Wire format

### Top-level (`synth validate --format json`)

```json
{
  "schema_version": "1.0",
  "diagnostics": [<Diagnostic>, <Diagnostic>, ...]
}
```

`diagnostics` is sorted in emission order: parse-stage diagnostics
first (in source order), then import resolution, then lowering,
then ERC rules in their registration order. Within an ERC rule,
diagnostics are emitted in net-then-component order so the output is
byte-deterministic across runs.

### Diagnostic object

```json
{
  "schema_version": "1.0",
  "code": "E-SYNTH-USB-001",
  "severity": "error",
  "title": "USB differential connection invalid",
  "message": "DP wire connected to non-DP-capable pin",
  "location": {
    "file": "board.synth",
    "byte_start": 412,
    "byte_end": 421,
    "line_start": 14,
    "col_start": 9,
    "line_end": 14,
    "col_end": 18
  },
  "entities": [
    { "kind": "component", "id": "U1" },
    { "kind": "pin", "component": "U1", "pin": "GP0" }
  ],
  "expected": "USB_DP capable target",
  "found": "SPI MOSI pin",
  "candidates": [
    { "kind": "pin", "component": "U1", "pin": "USB_DP", "confidence": 0.92 }
  ],
  "suggested_fixes": [
    {
      "confidence": 0.92,
      "rationale": "Only USB_DP-capable pin on U1.",
      "kind": "replace_range",
      "range": { "byte_start": 412, "byte_end": 421 },
      "replacement": "U1.usb_dp"
    }
  ],
  "explanation_url": "synth.docs/diagnostics/E-SYNTH-USB-001"
}
```

#### Required fields

| Field            | Type                                        | Notes                                               |
| ---------------- | ------------------------------------------- | --------------------------------------------------- |
| `schema_version` | string                                      | Always `"1.0"` for this protocol revision.          |
| `code`           | string                                      | Stable, documented at `docs/diagnostics/<code>.md`. |
| `severity`       | `"info" \| "warning" \| "error" \| "fatal"` | `"error"` and `"fatal"` cause non-zero exit.        |
| `title`          | string                                      | Short, one-line, stable across patch releases.      |

#### Optional fields

| Field             | Type          | Notes                                                        |
| ----------------- | ------------- | ------------------------------------------------------------ |
| `message`         | string        | Longer human prose. **Not** stable across patch releases.    |
| `location`        | `Location`    | Anchors the diagnostic to a byte range in a source file.     |
| `entities`        | `[EntityRef]` | Domain entities the diagnostic concerns. May be empty.       |
| `expected`        | string        | Free-form description of what the rule wanted.               |
| `found`           | string        | Free-form description of what the rule got.                  |
| `candidates`      | `[Candidate]` | Informational alternatives, sorted by descending confidence. |
| `suggested_fixes` | `[Patch]`     | Machine-applicable edits, sorted by descending confidence.   |
| `explanation_url` | string        | Conventionally `synth.docs/diagnostics/<code>`.              |

Absent optional fields are omitted from the JSON (not serialized as
`null`). Agents may pass-through any field they don't recognise.

### `Location`

```json
{
  "file": "board.synth",
  "byte_start": 412,
  "byte_end": 421,
  "line_start": 14,
  "col_start": 9,
  "line_end": 14,
  "col_end": 18
}
```

- **Authoritative:** `(file, byte_start, byte_end)`. Byte offsets are
  UTF-8 byte indices, half-open `[start, end)`.
- **Derived:** `line_start`, `col_start`, `line_end`, `col_end`. 1-indexed.
  If a stage has not computed them, they default to `0` — agents must
  treat `0` as "unknown" and fall back to byte offsets.

### `EntityRef`

Tagged on `kind`:

```json
{"kind": "component",  "id": "U1"}
{"kind": "pin",        "component": "U1", "pin": "GP0"}
{"kind": "net",        "name": "vdd"}
{"kind": "constraint", "id": "diff_pair_usb"}
{"kind": "module",     "name": "power_section"}
```

### `Candidate`

```json
{ "kind": "pin", "component": "U1", "pin": "USB_DP", "confidence": 0.92 }
```

Candidates are **informational**. They describe alternatives the
agent might consider; they are not edits. For edits, see
`suggested_fixes`.

### `Patch`

A `Patch` is a machine-applicable edit. The host applies it by
calling `synth fix` or by invoking the equivalent function in
`synth-diagnostics`.

```json
{
  "confidence": 0.92,
  "rationale": "Only USB_DP-capable pin on U1.",
  "kind": "replace_range",
  "range": { "byte_start": 412, "byte_end": 421 },
  "replacement": "U1.usb_dp"
}
```

| Field        | Type                      | Notes                                                    |
| ------------ | ------------------------- | -------------------------------------------------------- |
| `confidence` | float in `[0.0, 1.0]`     | Sort order within a diagnostic's `suggested_fixes`.      |
| `rationale`  | string \| absent          | Free-form human-readable explanation; agents may ignore. |
| `kind`       | one of the variants below | Tagged union; new variants are minor-version-compatible. |

#### Patch primitives

##### `replace_range`

```json
{
  "kind": "replace_range",
  "range": { "byte_start": 12, "byte_end": 18 },
  "replacement": "p1"
}
```

Replace the half-open byte range `[byte_start, byte_end)` with
`replacement`. Out-of-bounds ranges return `PatchError::OutOfBounds`.

##### `insert_at`

```json
{ "kind": "insert_at", "at": 42, "text": " -> " }
```

Insert `text` at byte offset `at`. `at` may equal the source length
(end-of-file insertion is valid).

##### `delete_range`

```json
{ "kind": "delete_range", "range": { "byte_start": 100, "byte_end": 120 } }
```

Delete the half-open byte range. Equivalent to
`replace_range` with `replacement: ""`.

##### `add_statement`

```json
{ "kind": "add_statement", "scope": "main_board", "statement": "layers 4" }
```

Insert a SynthSpec statement at the given scope. **Semantic
primitive — not yet implemented in v1.0.** Returns
`PatchError::Unsupported` from the textual apply entry point. A
future `synth-patch` crate (introduced post-V1) will resolve this.

##### `remove_statement`

```json
{ "kind": "remove_statement", "id": "U7" }
```

Remove a statement identified by `id`. **Same status as
`add_statement`** — semantic, deferred.

#### Patch application order

Agents that apply multiple patches in one pass **must** sort by
descending `byte_start` so earlier patches do not shift later byte
offsets. The reference implementation in `synth fix` does this; it is
also the algorithm documented in the agent harness
(`crates/synth-validate/tests/agent_harness.rs`).

## CLI subcommands relevant to agents

### `synth validate --format json <input>`

Run the full pipeline. Print diagnostics as a single JSON document on
stdout. Exit `0` on clean, `1` on any error- or fatal-severity
diagnostic, `2` on usage errors.

### `synth fix [--dry-run] <input>`

Run validate, collect the top `suggested_fix` per diagnostic, apply
in reverse byte order, write the result back to the input file.
`--dry-run` prints the patched source to stdout instead. Exit `0`
even if patches fail to apply — the workflow is "apply what we can
and re-validate."

### `synth schema diagnostic`

Print the JSON Schema (draft-07) for the diagnostic protocol to
stdout. Useful for tooling that validates third-party diagnostic
producers or consumers.

### `synth dump-ast --pretty <input>`

Dump the parsed AST as JSON. The AST shape is **not stable** — it is
exposed for debugging only; not part of this protocol.

### `synth dump-ir --pretty <input>`

Dump the lowered IR (`Board`) as JSON. Same stability caveat as
`dump-ast`.

## Exit codes

| Code | Meaning                                                                                    |
| ---- | ------------------------------------------------------------------------------------------ |
| `0`  | Success — no error- or fatal-severity diagnostics emitted.                                 |
| `1`  | Validation errors — at least one error- or fatal-severity diagnostic emitted.              |
| `2`  | Usage error — invalid arguments, missing input file, registry load failure.                |
| `3`  | Internal compiler error — a panic or invariant violation. Should never happen; file a bug. |

## Stability tests

The protocol is enforced by the following checks in CI:

- `crates/synth-diagnostics/tests/schema_freshness.rs` — the checked-in
  `schemas/diagnostic.schema.json` must match the schema derived from
  the in-source types. Regenerate via
  `cargo run -p synth-cli -- schema diagnostic > schemas/diagnostic.schema.json`.
- `crates/synth-validate/tests/agent_harness.rs` — the reference
  agent harness must converge on ≥80% of the seeded corpus
  (`fixtures/agent/*.synth`) in ≤5 iterations, and the loop must be
  deterministic across runs.
- Existing parser- and ERC-fixture tests round-trip diagnostic JSON
  via `insta` snapshots, so accidental shape changes show up in
  pre-merge review.
