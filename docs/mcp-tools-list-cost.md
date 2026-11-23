# MCP tools/list context cost

Status: step 1 of #481 done. On-demand tool groups are deferred.

## Why it matters

Every `tools/list` sends all tool schemas to the agent. That context
is not available for the design or the datasheets.

## How it is measured

The test `tools_list_cost_tests` in `crates/synth-mcp/src/tools.rs`
serialises the `tools/list` result and prints the byte size, a token
estimate and the size of each tool. It fails when the payload grows
past `TOOLS_LIST_BYTES_CEILING`.

The token estimate is bytes divided by 4. It is a rule of thumb, not
a tokenizer count.

Run it with:

    cargo test -p synth-mcp --lib tools_list_cost -- --nocapture

## Ceiling policy

The ceiling is a ratchet. Lower it when descriptions shrink. Raise
it only on purpose, in its own commit, with the reason in the message.

## Measured sizes

| State | Tools | Bytes | Tokens (bytes/4) |
|-------|-------|-------|------------------|
| Before | 36 | 45926 | 11482 |
| After | 36 | 44208 | 11052 |

Largest tools after the change: `synth_export` (4255 bytes),
`synth_place_with_hints` (3153), `synth_compact_layout` (2741),
`synth_mutate_layout` (2601), `synth_route` (2584).

## Deferred: on-demand tool groups

Issue #481 proposes a default `core` listing and a
`synth_load_tools(group)` call that sends `tools/list_changed`.
This is not built. Proposed groups:

- `core`: validate, ERC, DRC, export, render, layout, route, registry search.
- `sourcing`: cost, supply chain, web and vector search.
- `advanced`: simulation, thermal and SI, multiboard, multi-agent, cloud.

Pick the groups from data first, for example `audit.jsonl` call
counts, before building the router.
