# synth-mcp

Model Context Protocol (MCP) server exposing Synth's hardware-design
capabilities to AI agents and LLMs. It speaks JSON-RPC 2.0 over either
stdio (`synth mcp --stdio`) or SSE/HTTP (`synth mcp --sse --port 3000`,
endpoints `/rpc`, `/message`, `/sse`).

The server surfaces every compiler stage as a tool: validate, ERC/DRC,
fix, route, place, export, and — for growing the component library — the
**part import/author** tools described below.

## Running

```bash
# stdio transport (recommended for agent harnesses)
synth mcp --stdio

# HTTP + SSE transport
synth mcp --sse --port 3000 --registry registry/parts
```

All tools accept a `registry_path`, `workspace_root`, or fall back to the
default bundled registry. Import/author tools additionally honour
`SYNTH_USER_REGISTRY_DIR` (the Tier-2 per-user registry) or an explicit
`user_registry` argument.

## Design-language reference

| Tool                       | Purpose                                                                                                                                                                                                                                                                                                                                    |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `synth_language_reference` | Returns the SynthSpec (`.synth`) grammar — every statement form (`import`, `board`, `layers`, `manufacturer`, `component`, `connect`, `diff_pair`, `keepout`, `placement_hint`), the component-kind vocabulary, `placement_hint` attribute values, and engineering units — plus three complete worked example designs. Takes no arguments. |

Call `synth_language_reference` before drafting a `.synth` file from
scratch — an agent with no prior exposure to SynthSpec otherwise has to
guess the syntax from diagnostic messages alone. Pair it with
`synth_search_registry` to get real `(kind, part_id)` pairs and exact pin
names for the parts you reference in `component`/`connect` statements.

The three embedded examples (kept as real files under `examples/` so
they can't silently drift from what the parser accepts):

| Example                                  | Demonstrates                                                                                                                                                                                                      |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `examples/env_logger.synth`              | A realistic full board: MCU, LDO regulator, I2C sensor, reset circuit, debug header.                                                                                                                              |
| `examples/sensor_logger.synth`           | A larger full board: USB-C power, dual I2C sensors, SPI flash, status LEDs.                                                                                                                                       |
| `examples/placement_and_diff_pair.synth` | The syntax the other two don't use: a hard `placement_hint`, a `diff_pair` with `impedance` (resolved via the `<REFDES>_<pin>` endpoint convention — V1 has no separate net-naming syntax), and a `keepout` zone. |

```jsonrpc
--> { "jsonrpc": "2.0", "id": 0, "method": "tools/call",
      "params": { "name": "synth_language_reference", "arguments": {} } }

<-- { "jsonrpc": "2.0", "id": 0,
      "result": { "grammar": { "statements": [ ... ] },
                  "examples": [ { "file": "examples/env_logger.synth", "source": "..." },
                                { "file": "examples/sensor_logger.synth", "source": "..." },
                                { "file": "examples/placement_and_diff_pair.synth", "source": "..." } ] } }
```

## Part-import & authoring tools

| Tool                        | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `synth_search_registry`     | Keyword search the **local** Tier-1 + Tier-2 registries.                                                                                                                                                                                                                                                                                                                                                                                                   |
| `synth_search_registry_web` | Keyword search **LCSC/EasyEDA** for live stock/price. Falls back to the local registries when the network is unavailable (`source: "local_registry"`).                                                                                                                                                                                                                                                                                                     |
| `synth_import_part`         | Import a part into the Tier-2 registry. `source: "lcsc"` converts an EasyEDA CAD JSON (pass `from_file`; live fetch is best-effort) into a `.kicad_mod` + unverified `.synth.toml`; `source: "kicad"` reads the physical pin inventory of an installed KiCad stock symbol and emits a pre-filled `.synth.toml`; `source: "kicad-zip"` reads a SnapEDA/UltraLibrarian "Export to KiCad" zip already downloaded to disk (pass `zip_path`) and does the same. |
| `synth_author_part`         | Validate a hand/agent-authored TOML part and persist it to the Tier-2 registry.                                                                                                                                                                                                                                                                                                                                                                            |

Imported/authored parts are tagged `provenance.source = imported|authored`
with an empty `reviewed_by`, so the `UnverifiedPartRule`
(`W-SYNTH-PART-UNVERIFIED`) still flags them until a human confirms the
pinout.

### Component sources: what's live vs. file-based, and why

| Source                                  | Path                                                         | Why                                                                                                                                                                                                                                                                                                                                                                                                      |
| --------------------------------------- | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| LCSC / EasyEDA                          | Live fetch or `from_file`                                    | LCSC's product-detail endpoint is publicly reachable; used directly.                                                                                                                                                                                                                                                                                                                                     |
| JLCPCB Parts Library (jlcpcb.com/parts) | Same as LCSC — use `source: "lcsc"` with the listed C-number | JLCPCB is LCSC's sister company; every JLCPCB Parts Library entry uses the identical C-number ("JLCPCB Part #, formerly also LCSC Part #" per their own docs). No separate API or import path exists or is needed.                                                                                                                                                                                       |
| KiCad stock libraries                   | Local install (`source: "kicad"`)                            | Reads the symbol already on the machine.                                                                                                                                                                                                                                                                                                                                                                 |
| SnapEDA (snapeda.com)                   | `source: "kicad-zip"`, file only                             | SnapEDA has no public API without a signed partnership, and its Terms of Service explicitly prohibit "any automated means—including robots, scrapers, crawlers, spiders" against the site, plus building "services substantially similar to the Site." Synth never contacts snapeda.com; the user downloads and exports to KiCad format through their own browser, and `kicad-zip` only reads that file. |
| UltraLibrarian (ultralibrarian.com)     | `source: "kicad-zip"`, file only                             | No public API; ToS: "You may not use any robot or other automated means to access or gather content from the Website." Same file-only path as SnapEDA.                                                                                                                                                                                                                                                   |

## The agent design loop

An agent grows the component library without leaving the tool surface:

```
1. synth_search_registry_web   → candidate parts + live stock/price
        (offline: falls back to synth_search_registry)
2. synth_import_part           → LCSC JSON, KiCad symbol, or a SnapEDA/
                                  UltraLibrarian export zip → Tier-2 .synth.toml
        (or) synth_author_part → persist a hand-authored .synth.toml
3. synth_validate / registry doctor → confirm the part parses & pins are sane
4. synth_fix                   → apply any suggested patches
5. repeat until the board lowers clean, then synth_export
```

### Example: import an ESP32 module from its KiCad symbol

```jsonrpc
--> { "jsonrpc": "2.0", "id": 1, "method": "tools/call",
      "params": { "name": "synth_import_part",
                  "arguments": { "source": "kicad",
                                 "lib_id": "Device:R",
                                 "user_registry": "/home/me/.synth/registry" } } }

<-- { "jsonrpc": "2.0", "id": 1,
      "result": { "status": "imported", "source": "kicad",
                  "part_id": "r", "pin_count": 2,
                  "part_path": "/home/me/.synth/registry/r.synth.toml" } }
```

### Example: author a part and validate it

```jsonrpc
--> { "jsonrpc": "2.0", "id": 2, "method": "tools/call",
      "params": { "name": "synth_author_part",
                  "arguments": { "part_toml": "id = \"led_red\"\nkind = \"diode\"\n[[pins]]\nname = \"A\"\nnumber = \"1\"\nelectrical_type = \"input\"\n[[pins]]\nname = \"C\"\nnumber = \"2\"\nelectrical_type = \"output\"\n",
                                 "user_registry": "/home/me/.synth/registry" } } }

<-- { "jsonrpc": "2.0", "id": 2,
      "result": { "status": "authored", "part_id": "led_red",
                  "path": "/home/me/.synth/registry/led_red.synth.toml",
                  "pin_count": 2 } }
```

### Example: live search with offline fallback

```jsonrpc
--> { "jsonrpc": "2.0", "id": 3, "method": "tools/call",
      "params": { "name": "synth_search_registry_web",
                  "arguments": { "query": "AMS1117" } } }

<-- { "jsonrpc": "2.0", "id": 3,
      "result": { "status": "ok", "source": "lcsc",
                  "part_number": "C6186", "in_stock": true,
                  "stock_qty": 12345, "moq": 1,
                  "unit_price_usd": 0.08, "lifecycle": "Active" } }
```

When the network is unreachable the same call returns
`"source": "local_registry"` with the matching local parts instead of an
error.

