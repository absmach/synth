# MCP transport: progress, cancellation and label control

Status: accepted, 2026-11-13. Covers #359 and #374.

## Transports

- **stdio** runs each `tools/call` on a worker and supports
  `notifications/progress` and `notifications/cancelled` (see
  `crates/synth-mcp/src/server.rs`).
- **HTTP/SSE** runs each `tools/call` on the same shared worker stdio uses
  (the `Dispatcher` in `crates/synth-mcp/src/server.rs`), so a long
  `synth_export` no longer blocks the server: `ping`, `tools/list` and
  `notifications/cancelled` posted to `/rpc` or `/message` are served while
  the export runs. A call that carries `_meta.progressToken` is answered
  with an SSE stream (`text/event-stream`): `notifications/progress`
  heartbeat events while the tool runs, and the final JSON-RPC response as
  the last event. A call without a token answers with plain JSON as before,
  but still honours cancellation.

A cancelled call answers with the `-32800` (`Request cancelled`) JSON-RPC
error, where stdio sends no response at all — an HTTP request must return
something, and the client is still waiting on its connection.

Remaining HTTP/SSE limits:

- There is no standalone `GET /sse` notification channel. Progress flows in
  the streaming response to the POST that carried the `progressToken`, so a
  client that wants progress reads that POST's response as SSE rather than
  opening a separate event connection.
- Progress is still the fixed-interval heartbeat ("synth is still working"),
  not tool-reported percentages.

## Silkscreen label control

A board can already hide one reference label through the placement sidecar:

```toml
[labels.R7]
hidden = true
```

That is per-part and survives re-export. A board-wide `labels off` statement is
not implemented; the working alternative is a sidecar entry per designator. The
exporter never post-edits a generated board to remove labels, because the next
export would overwrite the edit.

## USB-C 3D model

The stock `Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12` footprint names a
STEP file that is not in KiCad's 3D model package. The qualification packages
ship the model under `qualification/*/models/` and the exported board points at
the local copy. A registry-authored part may name a bundled model the same way;
this is preferred to a network download during export.
