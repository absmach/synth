# MCP tool groups and context budget

`tools/list` is bounded by a test: the serialised catalogue must stay under
120,000 bytes (about 30,000 tokens at four bytes per token). The ceiling only
moves down deliberately.

Tools are classified into three groups:

- `core` — the default design workflow: validate, ERC, DRC, export, render,
  layout, route and registry search.
- `sourcing` — cost, supply chain, and web/vector registry search.
- `advanced` — simulation, thermal/SI, multiboard, multi-agent synthesis and
  the cloud endpoint.

Call `synth_load_tools({ "group": "sourcing" })` to fetch a group's schemas,
then list tools again to refresh the client catalogue. `synth_recent_calls`
and `synth_server_stats` read the audit log so an agent can see which call
failed and why before retrying; error kinds are `parse_error`, `unknown_part`,
`missing_engine`, `invalid_argument` and `io_error`.
