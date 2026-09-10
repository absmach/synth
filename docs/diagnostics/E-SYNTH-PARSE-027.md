# E-SYNTH-PARSE-027 — invalid JSON or AST schema error

**Severity:** error
**Stage:** parse

## What this means

The JSON AST ingestion endpoint (`parse_json`) received JSON input that was either malformed JSON syntax or did not conform to the expected `ProgramAst` schema.

## Minimal reproduction

```json
{
  "invalid_ast": true
}
```

## Suggested fix

Ensure the input JSON matches the canonical `ProgramAst` schema, or serialize a valid `ProgramAst` structure using `serde_json`.
