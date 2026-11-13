# MCP transport: progress, cancellation and label control

Status: accepted, 2026-11-13. Covers #359 and #374.

## Transports

- **stdio** runs each `tools/call` on a worker and supports
  `notifications/progress` and `notifications/cancelled` (see
  `crates/synth-mcp/src/server.rs`).
- **HTTP/SSE** currently runs `tools/call` synchronously. A long
  `synth_export` sends no progress, so a client can time it out.

The HTTP fix is a transport-neutral dispatcher: `/rpc` and `/message` enqueue
the call, `/sse` streams `notifications/progress` and the final response, and
`notifications/cancelled` drops the in-flight token. Until that lands, a client
should use the stdio transport or raise its request timeout.

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
