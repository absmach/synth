// SPDX-License-Identifier: Apache-2.0

//! MCP Tool handlers exposing Synth hardware design capabilities to LLMs and agents.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use synth_supply::Distributor;

/// Tool description metadata exposed in `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    /// Serialized as `inputSchema` to conform to the MCP JSON-RPC spec.
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// List all available MCP tools supported by Synth.
#[allow(clippy::too_many_lines)]
pub fn list_tools() -> Vec<McpToolInfo> {
    vec![
        McpToolInfo {
            name: "synth_language_reference".into(),
            description: "Get the SynthSpec (.synth) design-language grammar — every statement form, component-kind vocabulary, placement_hint attributes, engineering units — plus three complete worked example designs. Call this before drafting a .synth file from scratch; call synth_search_registry alongside it to find real (kind, part_id) pairs and pin names.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        McpToolInfo {
            name: "synth_search_registry".into(),
            description: "Search the Synth component registry by keyword, kind (mcu, sensor, regulator, connector, resistor, capacitor, ...), or MPN. Returns matching part IDs, descriptions, pin names, LCSC part numbers, and required decoupling — everything an agent needs to write correct SynthSpec component declarations.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Free-text search against part ID, description, or MPN" },
                    "kind": { "type": "string", "description": "Filter by component kind: mcu, sensor, regulator, connector, resistor, capacitor, switch, etc." },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_validate".into(),
            description: "Compile and validate a SynthSpec design file or string. Returns structured diagnostics with byte spans and patch suggestions.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_erc_report".into(),
            description: "Run Electrical Rules Checking (ERC) on a SynthSpec design. Returns structured ERC violations (power, i2c, spi, decoupling, etc.) with byte spans and suggested fixes.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_query_knowledge".into(),
            description: "Query the circuit-design knowledge graph: production support circuits (switch debounce, pull-ups, LED current limiting, IC decoupling, relay flyback, I2C pull-ups, crystal load caps, USB ESD) with rationale, severity, and — given a design — which ones are missing (E-SYNTH-KG-001 findings with insertion patches).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "Optional component kind (e.g. 'switch', 'led', 'relay') to list applicable templates for" },
                    "source": { "type": "string", "description": "Optional SynthSpec source code string to check against the knowledge graph" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk to check" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_drc_report".into(),
            description: "Run Physical Design Rule Checking (DRC) on a placed and routed PCB design. Validates trace clearances, widths, drill sizes, and courtyard overlaps against manufacturer profile.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "profile": { "type": "string", "description": "Manufacturer profile name ('jlcpcb_standard', 'jlcpcb_advanced') or path to custom .toml profile" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_fix".into(),
            description: "Apply the highest-confidence suggested_fix for each blocking diagnostic in one pass. Mirrors `synth fix [--smt]`. Returns the patched source code and a detailed per-diagnostic patch application report. Agents should call this in a loop until `is_clean` is true.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "enable_smt": { "type": "boolean", "description": "If true, solve SolveSmt quantitative constraint patches using the built-in SMT solver (default: false)" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_apply_patch".into(),
            description: "Apply structured patch primitives (replace_range, insert_at, delete_range, solve_smt) directly to source text.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Original source string" },
                    "patches": {
                        "type": "array",
                        "items": { "type": "object" },
                        "description": "List of Patch objects to apply"
                    },
                    "enable_smt": { "type": "boolean", "description": "Enable SMT constraint solving for SolveSmt patch kinds (default: false)" }
                },
                "required": ["source", "patches"]
            }),
        },
        McpToolInfo {
            name: "synth_route".into(),
            description: "Run deterministic PCB track routing on a SynthSpec design. Returns routed segments, through-hole vias, unrouted net diagnostics, and total wire length stats.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "log_routing_outcomes": { "type": "string", "description": "Optional directory path for logging Dataset 6 routing outcome pairs" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_preview_schematic".into(),
            description: "Generate auto-layout and visual schematic placement for a SynthSpec design.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code" },
                    "file_path": { "type": "string", "description": "Path to source file" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_mutate_layout".into(),
            description: "Apply one structured edit to a SynthSpec design's auto-generated schematic layout — move or rotate a component, group several components into a tidy column beside an anchor, force a net to render as a label instead of a wire, or re-route a net. Never changes connectivity, only visual placement. Returns the updated layout as JSON, or a structured error if the op references an unknown component/net id.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code" },
                    "file_path": { "type": "string", "description": "Path to source file" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" },
                    "op": {
                        "type": "object",
                        "description": "Layout edit operation: move_component, rotate_component, group_components, set_net_style, or reroute_net"
                    }
                },
                "required": ["op"]
            }),
        },
        McpToolInfo {
            name: "synth_export".into(),
            description: "Export a validated SynthSpec design to KiCad schematic (.kicad_sch), PCB (.kicad_pcb), BOM CSV, or Gerber files.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code" },
                    "file_path": { "type": "string", "description": "Path to source file" },
                    "out_dir": { "type": "string", "description": "Output directory path (accepts 'out' or 'out_dir')" },
                    "out": { "type": "string", "description": "Alias for out_dir" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root path for auto-resolving registry" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_predict_patch_consequence".into(),
            description: "Predict downstream diagnostic creation consequences for proposed patches using the learned Patch-Consequence MLP model.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "diagnostic_code": { "type": "string", "description": "Optional ERC diagnostic code filter (e.g. 'E-SYNTH-POWER-001')" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_cloud_mcp_endpoint".into(),
            description: "Cloud MCP Endpoint extension (under active local development).".into(),
            input_schema: serde_json::json!({ "type": "object" }),
        },
        McpToolInfo {
            name: "synth_live_supply_chain".into(),
            description: "Query real-time stock availability, MOQ, pricing, and lifecycle status for a single part number or an entire design BOM across LCSC and Nexar/Mouser/DigiKey APIs.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "part_number": { "type": "string", "description": "Single part number (LCSC C-number or MPN)" },
                    "mpn": { "type": "string", "description": "Alias for part_number" },
                    "lcsc_pn": { "type": "string", "description": "Alias for part_number" },
                    "source": { "type": "string", "description": "SynthSpec source code string to query entire BOM" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_multi_agent_synthesize".into(),
            description: "World Model multi-agent board synthesis engine (under active local development).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "High-level board synthesis prompt" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_place_with_hints".into(),
            description: "Run the PCB placer with explicit semantic placement hints (region, edge, near/side) for one or more components. Does not modify the source file. Returns placement positions, hint satisfaction, unrouted net count, and full DRC violation reports.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":           { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":        { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional path to .layout.toml sidecar overrides file" },
                    "profile":          { "type": "string", "description": "Optional manufacturer DRC profile ('jlcpcb_standard' or path to toml)" },
                    "registry_path":    { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root":   { "type": "string", "description": "Optional workspace root path" },
                    "hints": {
                        "type": "array",
                        "description": "List of placement hints",
                        "items": {
                            "type": "object",
                            "properties": {
                                "component":  { "type": "string", "description": "Component refdes e.g. 'U1'" },
                                "components": { "type": "array", "items": { "type": "string" }, "description": "List of component refdes" },
                                "region":     { "type": "string", "enum": ["top_left","top_right","bottom_left","bottom_right","centre","top_edge","bottom_edge","left_edge","right_edge"] },
                                "edge":       { "type": "string", "enum": ["top","bottom","left","right"] },
                                "near":       { "type": "string", "description": "Anchor component refdes" },
                                "side":       { "type": "string", "enum": ["above","below","left","right"] },
                                "priority":   { "type": "string", "enum": ["hard","soft"] }
                            }
                        }
                    }
                }
            }),
        },
        McpToolInfo {
            name: "synth_describe_placement".into(),
            description: "Get a human-readable semantic summary of the current PCB placement — which functional clusters are in which board regions, density warnings, and DRC status. Enables LLMs to reason about layout quality.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":        { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":     { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root":{ "type": "string", "description": "Optional workspace root path" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_read_layout_overrides".into(),
            description: "Read all persisted component placement overrides and forced net labels from sidecar file (<design>.synth.layout.toml). Includes provenance (human_drag vs agent), priority, and timestamp.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "layout_file_path": { "type": "string", "description": "Path to sidecar TOML file" }
                },
                "required": ["layout_file_path"]
            }),
        },
        McpToolInfo {
            name: "synth_write_layout_override".into(),
            description: "Write or update a component placement override or forced net label in sidecar file (<design>.synth.layout.toml). Preserves existing overrides.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "layout_file_path": { "type": "string", "description": "Path to sidecar TOML file" },
                    "refdes":           { "type": "string", "description": "Component refdes e.g. 'U1'" },
                    "x_mm":             { "type": "number", "description": "Component X position in mm" },
                    "y_mm":             { "type": "number", "description": "Component Y position in mm" },
                    "rotation":         { "type": "integer", "description": "Rotation in degrees (0, 90, 180, 270)" },
                    "source":           { "type": "string", "enum": ["human_drag", "agent"], "description": "Provenance tag" },
                    "priority":         { "type": "string", "enum": ["soft", "hard"], "description": "Override priority" }
                },
                "required": ["layout_file_path", "refdes", "x_mm", "y_mm"]
            }),
        },
        McpToolInfo {
            name: "synth_route_with_constraints".into(),
            description: "Run the PCB autorouter with explicit per-net routing constraints (trace width, clearance, preferred layer, differential pair) and optional layout sidecar overrides. Evaluates DRC and returns segments, vias, unrouted nets, and DRC violation reports.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":           { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":        { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional path to .layout.toml sidecar overrides file" },
                    "profile":          { "type": "string", "description": "Optional manufacturer DRC profile ('jlcpcb_standard' or path to toml)" },
                    "registry_path":    { "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root":   { "type": "string", "description": "Optional workspace root path" },
                    "net_constraints": {
                        "type": "array",
                        "description": "Per-net routing rules",
                        "items": {
                            "type": "object",
                            "properties": {
                                "net":             { "type": "string", "description": "Net name e.g. 'VBUS' or 'D+'" },
                                "width_mm":        { "type": "number", "description": "Trace width in mm" },
                                "clearance_mm":    { "type": "number", "description": "Clearance in mm" },
                                "preferred_layer": { "type": "string", "description": "Preferred layer e.g. 'top' or 'bottom'" },
                                "diff_pair":       { "type": "boolean", "description": "Differential pair flag" }
                            },
                            "required": ["net"]
                        }
                    }
                }
            }),
        },
        McpToolInfo {
            name: "synth_import_part".into(),
            description: "Import a component part into the Tier-2 (per-user) registry. Wraps the R15.4/R15.5 importers plus a zip-file importer: source 'lcsc' reads an EasyEDA CAD JSON (pass `from_file`; live fetch is best-effort) and emits a `.kicad_mod` + unverified `.synth.toml` (`provenance.source = generated`); source 'kicad' reads the physical pin inventory (number, name, electrical type) from an installed KiCad stock symbol and emits a pre-filled `.synth.toml` (`provenance.source = imported`); source 'kicad-zip' reads a SnapEDA/UltraLibrarian \"Export to KiCad\" zip already downloaded to disk (pass `zip_path`) — neither vendor has a public API and both prohibit automated site access in their ToS, so this never contacts snapeda.com/ultralibrarian.com, it only parses the zip the user already saved. All three land with `reviewed_by` empty until reviewed.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":        { "type": "string", "description": "Import source: 'lcsc', 'kicad', or 'kicad-zip'" },
                    "code":          { "type": "string", "description": "LCSC product code, e.g. 'C2040' (source='lcsc')" },
                    "from_file":     { "type": "string", "description": "Path to a cached EasyEDA CAD JSON (source='lcsc')" },
                    "lib_id":        { "type": "string", "description": "KiCad lib_id, e.g. 'Device:R' (source='kicad')" },
                    "zip_path":      { "type": "string", "description": "Path to a downloaded SnapEDA/UltraLibrarian KiCad-format export .zip (source='kicad-zip')" },
                    "id":            { "type": "string", "description": "Override the generated PartId (filename stem)" },
                    "footprint":     { "type": "string", "description": "Optional kicad_footprint reference (source='kicad')" },
                    "footprint_dir": { "type": "string", "description": "Directory to write generated .kicad_mod (source='lcsc')" },
                    "user_registry": { "type": "string", "description": "Tier-2 registry directory (defaults to SYNTH_USER_REGISTRY_DIR)" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_search_registry_vector".into(),
            description: "Search the Synth component registry using natural language intent via 384-D dense semantic vector embeddings. Returns similarity-ranked parts matching free-form natural language prompts.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query":        { "type": "string", "description": "Natural language intent query (e.g., '3.3V LDO regulator with enable pin in SOT-23')" },
                    "top_k":        { "type": "integer", "description": "Number of top matching results to return (default: 5)" },
                    "registry_path":{ "type": "string", "description": "Optional custom component registry path" },
                    "workspace_root":{ "type": "string", "description": "Optional workspace root path" }
                },
                "required": ["query"]
            }),
        },
        McpToolInfo {
            name: "synth_evaluate_thermal_si".into(),
            description: "Run sub-millisecond Signal Integrity (SI) transmission line characteristic impedance (Z0), propagation delay, and component thermal temperature rise physics calculations.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "trace_width_mm":       { "type": "number", "description": "Trace width in mm (default: 0.20)" },
                    "dielectric_height_mm": { "type": "number", "description": "Substrate height in mm (default: 0.16)" },
                    "trace_thickness_mm":  { "type": "number", "description": "Copper thickness in mm (default: 0.035)" },
                    "er":                   { "type": "number", "description": "Substrate relative permittivity (default: 4.3 for FR-4)" },
                    "power_watts":          { "type": "number", "description": "Component power dissipation in Watts (default: 0.5)" },
                    "r_theta_ja":           { "type": "number", "description": "Thermal resistance Junction-to-Ambient in deg C / W (default: 50.0)" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_author_part".into(),
            description: "Write a human/agent-authored `.synth.toml` part into the Tier-2 (per-user) registry after validating it. Accepts a TOML string in `part_toml` (the same format as shipped registry parts) and persists it as `<id>.synth.toml`. Marks `provenance.source = authored`; the part stays unverified until `reviewed_by` is set.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "part_toml":     { "type": "string", "description": "Part definition as TOML (must include id and pins[].name/number)" },
                    "user_registry": { "type": "string", "description": "Tier-2 registry directory (defaults to SYNTH_USER_REGISTRY_DIR)" }
                }
            }),
        },
        McpToolInfo {
            name: "synth_export_multiboard".into(),
            description: "Validate a multi-board hardware system (mainboard, daughtercards), verify inter-board header pin connections, and check cross-board signal continuity and voltage domain isolation.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_name": { "type": "string", "description": "Multi-board system project name (default: 'multi_board_system')" },
                    "interboard_mappings": {
                        "type": "array",
                        "description": "List of inter-board header pin mappings",
                        "items": {
                            "type": "object",
                            "properties": {
                                "from_board":  { "type": "string" },
                                "from_refdes": { "type": "string" },
                                "from_pin":    { "type": "string" },
                                "to_board":    { "type": "string" },
                                "to_refdes":   { "type": "string" },
                                "to_pin":      { "type": "string" }
                            }
                        }
                    }
                }
            }),
        },
        McpToolInfo {
            name: "synth_search_registry_web".into(),
            description: "Keyword search LCSC/EasyEDA for candidate parts and live stock/price. Best-effort network call; when the network is unavailable (or returns nothing) it falls back to searching the local Tier-1 + Tier-2 registries, so an agent always gets candidates. Returns `source: \"lcsc\"` with stock/price on a live hit, or `source: \"local_registry\"` with matched parts offline.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Keyword or part number to search, e.g. 'AMS1117' or 'C2040'" }
                }
            }),
        },
    ]
}

/// Execute a named tool with arguments.
pub fn call_tool(
    name: &str,
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    match name {
        "synth_language_reference" => Ok(execute_language_reference(args)),
        "synth_search_registry" => execute_search_registry(args, default_registry),
        "synth_validate" => execute_validate(args, default_registry),
        "synth_erc_report" => execute_erc_report(args, default_registry),
        "synth_query_knowledge" => execute_query_knowledge(args, default_registry),
        "synth_drc_report" => execute_drc_report(args, default_registry),
        "synth_fix" => execute_fix(args, default_registry),
        "synth_apply_patch" => execute_apply_patch(args, default_registry),
        "synth_route" => execute_route(args, default_registry),
        "synth_preview_schematic" => execute_preview_schematic(args, default_registry),
        "synth_mutate_layout" => execute_mutate_layout(args, default_registry),
        "synth_export" => execute_export(args, default_registry),
        "synth_predict_patch_consequence" => {
            execute_predict_patch_consequence(args, default_registry)
        }
        "synth_live_supply_chain" => execute_live_supply_chain(args, default_registry),
        "synth_place_with_hints" => execute_place_with_hints(args, default_registry),
        "synth_describe_placement" => execute_describe_placement(args, default_registry),
        "synth_read_layout_overrides" => execute_read_layout_overrides(args),
        "synth_write_layout_override" => execute_write_layout_override(args),
        "synth_route_with_constraints" => execute_route_with_constraints(args, default_registry),
        "synth_search_registry_vector" => execute_search_registry_vector(args, default_registry),
        "synth_evaluate_thermal_si" => execute_evaluate_thermal_si(args),
        "synth_cloud_mcp_endpoint" => execute_cloud_mcp_endpoint(args),
        "synth_import_part" => execute_import_part(args, default_registry),
        "synth_author_part" => execute_author_part(args, default_registry),
        "synth_search_registry_web" => execute_search_registry_web(args, default_registry),
        "synth_export_multiboard" => execute_export_multiboard(args),
        "synth_multi_agent_synthesize" => Ok(serde_json::json!({
            "status": "dev_stub",
            "message": format!("Tool '{name}' is under active local development.")
        })),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

/// Attempt to find workspace root containing `registry/parts`.
fn find_workspace_root() -> Option<PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
        let mut curr = exe_path.parent();
        while let Some(dir) = curr {
            if dir.join("registry").join("parts").is_dir() {
                return Some(dir.to_path_buf());
            }
            curr = dir.parent();
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(dir) = curr {
            if dir.join("registry").join("parts").is_dir() {
                return Some(dir.to_path_buf());
            }
            curr = dir.parent();
        }
    }

    None
}

/// Resolve the registry path from args, default_registry, or workspace_root.
fn resolve_registry(args: &Value, default_registry: Option<&Path>) -> PathBuf {
    if let Some(p) = args["registry_path"].as_str() {
        return PathBuf::from(p);
    }

    if let Some(p) = default_registry {
        return p.to_path_buf();
    }

    if let Some(ws_root) = args["workspace_root"].as_str() {
        let candidate = PathBuf::from(ws_root).join("registry").join("parts");
        if candidate.is_dir() {
            return candidate;
        }
    }

    if let Some(ws_root) = find_workspace_root() {
        let candidate = ws_root.join("registry").join("parts");
        if candidate.is_dir() {
            return candidate;
        }
    }

    PathBuf::from("registry").join("parts")
}

/// Resolve the Tier-2 (per-user) registry directory from args or
/// environment, without requiring one to be configured. Read-side
/// counterpart of `resolve_user_registry` (which errors when absent,
/// since the write tools need somewhere to write).
fn resolve_user_registry_opt(args: &Value) -> Option<PathBuf> {
    if let Some(p) = args["user_registry"].as_str() {
        return Some(PathBuf::from(p));
    }
    synth_registry::user_registry_dir()
}

/// Load the merged Tier-1 + Tier-2 registry, the same way `synth-cli`
/// does (`load_registry` in `main.rs`). Every tool that resolves parts
/// must go through this, not bare `load_dir` on Tier-1 alone — otherwise
/// a part just written by `synth_import_part` / `synth_author_part`
/// (Tier-2) is invisible to the very next `synth_validate` call, breaking
/// the unknown-part design loop (§18.8.4). Falls back to Tier-1-only when
/// no Tier-2 dir is configured or it doesn't exist yet.
fn load_registry_tiered(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<synth_registry::Registry, synth_registry::LoadError> {
    let tier1 = resolve_registry(args, default_registry);
    match resolve_user_registry_opt(args) {
        Some(tier2) if tier2.exists() => {
            synth_registry::load_tiered(&tier1, &tier2, false).map(|r| r.registry)
        }
        _ => synth_registry::load_dir(&tier1),
    }
}

/// Keyword search a single registry directory (Tier-1 or Tier-2) and return
/// the matching parts as JSON. Used by both `synth_search_registry` and the
/// offline fallback of `synth_search_registry_web`.
fn search_registry_dir(
    registry_dir: &Path,
    query: &str,
    kind_filter: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = synth_registry::load_dir(registry_dir).map_err(|e| {
        format!(
            "Could not load registry from '{}': {e}",
            registry_dir.display()
        )
    })?;
    Ok(filter_registry_matches(&registry, query, kind_filter))
}

/// Filter an already-loaded registry by keyword/kind. Split out of
/// `search_registry_dir` so `execute_search_registry` can search the
/// merged Tier-1 + Tier-2 registry (via `load_registry_tiered`) instead
/// of a single directory.
fn filter_registry_matches(
    registry: &synth_registry::Registry,
    query: &str,
    kind_filter: &str,
) -> Vec<serde_json::Value> {
    let query = query.to_lowercase();
    let kind_filter = kind_filter.to_lowercase();
    let mut matches = Vec::new();

    for (_id, part) in registry.iter() {
        if !kind_filter.is_empty() && part.kind.to_lowercase() != kind_filter {
            continue;
        }

        let matches_query = query.is_empty()
            || part.id.as_str().to_lowercase().contains(&query)
            || part.kind.to_lowercase().contains(&query)
            || part
                .description
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&query)
            || part
                .mpn
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&query);

        if matches_query {
            let pins: Vec<serde_json::Value> = part
                .pins
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "number": p.number.0,
                        "electrical_type": format!("{:?}", p.electrical_type),
                        "required": p.required
                    })
                })
                .collect();

            matches.push(serde_json::json!({
                "id": part.id.as_str(),
                "kind": part.kind,
                "description": part.description,
                "mpn": part.mpn,
                "lcsc_pn": part.lcsc_pn,
                "kicad_symbol": part.kicad_symbol,
                "kicad_footprint": part.kicad_footprint,
                "pins": pins,
                "required_decoupling": part.required_decoupling
            }));
        }
    }

    matches
}

/// Static SynthSpec grammar reference + worked examples, for
/// `synth_language_reference`. No filesystem/registry access, so it
/// needs no `default_registry` and can't fail.
fn execute_language_reference(_args: &Value) -> Value {
    serde_json::json!({
        "language": "SynthSpec (.synth)",
        "note": "This is the .synth design-input language grammar. It is distinct from the diagnostic JSON wire format documented in docs/protocol-v1.0.md (that covers synth_validate's output shape, not this input syntax).",
        "grammar": {
            "top_level": "[import \"relative/path.synth\"]*  board \"<name>\" { <statement>* }",
            "comments": "// line comment to end of line. No block comments.",
            "statements": [
                {
                    "form": "import \"<relative/path.synth>\"",
                    "where": "zero or more, before the `board { ... }` block",
                    "description": "Inlines another .synth file's statements at this point. Path must be relative with no `..` segments, resolved from the importing file's directory. Import cycles are rejected."
                },
                {
                    "form": "layers <integer>",
                    "description": "Copper layer count (commonly 2 or 4). At most one per board.",
                    "example": "layers 4"
                },
                {
                    "form": "manufacturer \"<name>\"",
                    "description": "Selects the DRC/manufacturing profile. At most one per board.",
                    "example": "manufacturer \"jlcpcb\""
                },
                {
                    "form": "revision \"<tag>\"",
                    "description": "Board revision tag (e.g. \"A\", \"1.2\") carried into the schematic title block's Revision field. At most one per board; optional.",
                    "example": "revision \"A\""
                },
                {
                    "form": "component <REFDES>: <kind> \"<part_id>\" [value \"<display>\"]",
                    "description": "Declares a component. <REFDES> is a unique designator (U1, C3, R12, ...). <kind> is a coarse category — see component_kinds_seen_in_registry below. <part_id> is a registry part id; call synth_search_registry to find valid (kind, part_id) pairs and the part's real pin names before writing `connect` lines. `value \"...\"` is an optional display value (e.g. \"10k\") for the schematic Value field and BOM.",
                    "example": "component R1: resistor \"r_generic_0603\" value \"10k\""
                },
                {
                    "form": "component <REFDES>: <kind> \"<part_id>\" { placement_hint { region: <region> edge: <edge> near: <REFDES> side: <side> priority: <hard|soft> } }",
                    "description": "Same as the plain component form, plus a placement_hint block. All attrs inside placement_hint are optional and independent; include only the ones you need.",
                    "example": "component U1: mcu \"stm32h743\" { placement_hint { region: top_left priority: hard } }"
                },
                {
                    "form": "connect <REFDES>.<pin> -> <REFDES>.<pin>",
                    "description": "Wires two component pins onto the same net. Pin names come from the part's registry entry. Repeat `connect` lines that share an endpoint to fan a net out to more than two pins.",
                    "example": "connect U1.vout -> C3.p1"
                },
                {
                    "form": "diff_pair <NET_P> <NET_N> { impedance <value><unit> }",
                    "description": "Marks two already-connected nets as a differential pair for routing, with a target characteristic impedance.",
                    "example": "diff_pair USB_DP USB_DN { impedance 90ohm }"
                },
                {
                    "form": "keepout <name> { radius <value><unit> }",
                    "description": "Declares a circular routing keepout zone (e.g. under an antenna).",
                    "example": "keepout antenna { radius 15mm }"
                }
            ],
            "component_kinds_seen_in_registry": [
                "antenna", "buzzer", "capacitor", "charger", "connector", "crystal", "diode",
                "display", "filter", "fuse", "ic", "inductor", "led", "level_shifter", "mcu",
                "memory", "modem", "opamp", "regulator", "resistor", "secure_element", "sensor",
                "switch", "transistor"
            ],
            "placement_hint_attrs": {
                "region": ["top_left", "top_right", "bottom_left", "bottom_right", "centre", "top_edge", "bottom_edge", "left_edge", "right_edge"],
                "edge": ["top", "bottom", "left", "right"],
                "near": "another component's REFDES",
                "side": ["above", "below", "left", "right"],
                "priority": ["hard", "soft"]
            },
            "units": ["mm", "mil", "ohm", "kohm", "mohm", "v", "mv", "a", "ma", "mhz", "ghz", "pf", "nf", "uf"]
        },
        "examples": [
            {
                "file": "examples/env_logger.synth",
                "description": "2-layer USB-C environmental sensor logger: STM32F103 MCU, BME680 sensor, AMS1117 LDO regulator, I2C bus with pull-ups, hardware reset circuit, UART debug header.",
                "source": include_str!("../../../examples/env_logger.synth")
            },
            {
                "file": "examples/sensor_logger.synth",
                "description": "4-layer sensor logger: ATmega328P MCU, USB-C with ESD protection diodes, two I2C environmental sensors, SPI NOR flash, status LEDs.",
                "source": include_str!("../../../examples/sensor_logger.synth")
            },
            {
                "file": "examples/placement_and_diff_pair.synth",
                "description": "Compact board isolating features the other two examples don't exercise: a hard placement_hint pinning the MCU to a board region, a diff_pair with a target impedance resolved via the <REFDES>_<pin> endpoint convention, and a keepout zone.",
                "source": include_str!("../../../examples/placement_and_diff_pair.synth")
            }
        ],
        "workflow_tip": "1) synth_search_registry to find real (kind, part_id) pairs and exact pin names. 2) Draft the .synth source using the grammar above. 3) synth_validate it. 4) synth_fix in a loop over diagnostics with suggested_fixes until clean. 5) synth_export."
    })
}

fn execute_search_registry(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let registry = load_registry_tiered(args, default_registry)
        .map_err(|e| format!("Could not load registry: {e}"))?;
    let mut matches = filter_registry_matches(
        &registry,
        args["query"].as_str().unwrap_or(""),
        args["kind"].as_str().unwrap_or(""),
    );

    let total_matches = matches.len();
    if matches.len() > 30 {
        matches.truncate(30);
    }

    Ok(serde_json::json!({
        "total_matches": total_matches,
        "returned_matches": matches.len(),
        "parts": matches
    }))
}

fn execute_validate(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    let mut diagnostics = parse.diagnostics;

    if let Some(ast) = parse.ast.as_ref() {
        let import_root = Path::new(file_name)
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let loader = synth_ir::FsImportLoader { root: import_root };
        let resolved = synth_ir::resolve_imports(ast, &loader, file_name);
        diagnostics.extend(resolved.diagnostics);

        if let Ok(registry) = load_registry_tiered(args, default_registry) {
            let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
            diagnostics.extend(lowered.diagnostics);
            if let Some(board) = lowered.board.as_ref() {
                diagnostics.extend(synth_validate::run_erc(board, file_name));
            }
        } else {
            let registry_dir = resolve_registry(args, default_registry);
            diagnostics.push(
                synth_diagnostics::DiagnosticBuilder::new(
                    "W-SYNTH-REGISTRY-001",
                    synth_diagnostics::Severity::Warning,
                    format!(
                        "Component registry not found at path '{}'",
                        registry_dir.display()
                    ),
                )
                .build(),
            );
        }
    }

    let is_clean = !diagnostics.iter().any(|d| d.severity.is_blocking());
    Ok(serde_json::json!({
        "status": if is_clean { "ok" } else { "error" },
        "diagnostics": diagnostics,
        "error_count": diagnostics.iter().filter(|d| d.severity.is_blocking()).count(),
        "warning_count": diagnostics.iter().filter(|d| d.severity == synth_diagnostics::Severity::Warning).count()
    }))
}

fn execute_erc_report(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    if parse.has_errors() {
        return Ok(serde_json::json!({
            "erc_clean": false,
            "error": "Parse phase produced blocking errors",
            "diagnostics": parse.diagnostics
        }));
    }

    let ast = parse.ast.ok_or("No AST produced")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;

    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let erc_diagnostics = synth_validate::run_erc(&board, file_name);
    let erc_clean = !erc_diagnostics.iter().any(|d| d.severity.is_blocking());

    Ok(serde_json::json!({
        "erc_clean": erc_clean,
        "violations": erc_diagnostics,
        "error_count": erc_diagnostics.iter().filter(|d| d.severity.is_blocking()).count(),
        "warning_count": erc_diagnostics.iter().filter(|d| d.severity == synth_diagnostics::Severity::Warning).count()
    }))
}

/// `synth_query_knowledge` — serve the circuit-design knowledge
/// graph: the template catalog (optionally filtered by component
/// kind), and — when a design is given — which enforced templates it
/// violates, including insertion patches.
fn execute_query_knowledge(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let kg = synth_knowledge::KnowledgeGraph::embedded();

    // Catalog (optionally filtered by kind).
    let templates: Vec<Value> = if let Some(kind) = args["kind"].as_str() {
        kg.templates_for_kind(kind)
    } else {
        kg.templates().iter().collect()
    }
    .into_iter()
    .map(|t| {
        serde_json::json!({
            "id": t.id,
            "description": t.description,
            "rationale": t.rationale,
            "severity": t.severity.to_string(),
            "applies_to_kinds": t.applies_to_kinds,
            "enforced_by": if t.enforced_by.is_empty() { "E-SYNTH-KG-001 (this server)" } else { t.enforced_by.as_str() },
        })
    })
    .collect();

    // Optional board check. A malformed design is a tool error, not
    // a catalog result.
    let check = if args["source"].as_str().is_some() || args["file_path"].as_str().is_some() {
        let violations = check_board_against_kg(args, default_registry, &kg)?;
        serde_json::json!({ "violations": violations })
    } else {
        serde_json::json!(null)
    };

    Ok(serde_json::json!({
        "templates": templates,
        "check": check,
    }))
}

/// Lower the design (if given) and run the knowledge-graph checker,
/// returning one JSON object per violation.
fn check_board_against_kg(
    args: &Value,
    default_registry: Option<&Path>,
    kg: &synth_knowledge::KnowledgeGraph,
) -> Result<Vec<Value>, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    if parse.has_errors() {
        return Err("Parse phase produced blocking errors".to_string());
    }
    let ast = parse.ast.ok_or("No AST produced")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    Ok(synth_knowledge::check_board(&board, kg)
        .into_iter()
        .map(|v| {
            serde_json::json!({
                "template": v.template_id,
                "severity": v.severity.to_string(),
                "component": v.refdes,
                "detail": v.detail,
                "patch_available": v.suggested_fix.is_some(),
            })
        })
        .collect())
}

fn execute_drc_report(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let profile_str = args["profile"].as_str().unwrap_or("jlcpcb_standard");
    let profile = match profile_str {
        "jlcpcb_standard" | "jlcpcb" => synth_drc::ManufacturerProfile::jlc_standard(),
        custom_path => {
            if let Ok(p) = synth_drc::ManufacturerProfile::from_toml_file(Path::new(custom_path)) {
                p
            } else {
                synth_drc::ManufacturerProfile::jlc_standard()
            }
        }
    };

    let parse = synth_parser::parse(&source, file_name.to_string());
    let ast = parse.ast.ok_or("Parse failed")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let placement = synth_place::place(&board).map_err(|e| format!("Placement failed: {e:?}"))?;
    let routing = synth_route::route(&board, &placement);
    let report = synth_drc::check(&board, &placement, &routing, &profile);

    Ok(serde_json::json!({
        "drc_clean": report.is_clean(),
        "violations": report.violations,
        "violation_count": report.violations.len()
    }))
}

fn execute_route(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    let ast = parse.ast.ok_or("Parse failed")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let placement = synth_place::place(&board).map_err(|e| format!("Placement failed: {e:?}"))?;
    let routing = synth_route::route(&board, &placement);

    if let Some(log_dir_str) = args["log_routing_outcomes"].as_str() {
        let log_dir = PathBuf::from(log_dir_str);
        if let Err(e) = synth_route::log_routing_outcome(&board, &placement, &routing, &log_dir) {
            eprintln!("synth_route MCP: failed to log outcome: {e}");
        }
    }

    let diags = routing.to_diagnostics(file_name);
    let total_wire_length_nm: i64 = routing
        .segments
        .iter()
        .map(|s| (s.end.x_nm - s.start.x_nm).abs() + (s.end.y_nm - s.start.y_nm).abs())
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let total_wire_length_mm = (total_wire_length_nm as f64) / 1_000_000.0;

    Ok(serde_json::json!({
        "status": if routing.unrouted_nets.is_empty() { "ok" } else { "unrouted_nets" },
        "segments_count": routing.segments.len(),
        "vias_count": routing.vias.len(),
        "unrouted_nets_count": routing.unrouted_nets.len(),
        "total_wire_length_mm": total_wire_length_mm,
        "routing": routing,
        "diagnostics": diags
    }))
}

fn execute_fix(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");
    let enable_smt = args["enable_smt"].as_bool().unwrap_or(false);

    let parse = synth_parser::parse(&source, file_name.to_string());
    let mut diagnostics = parse.diagnostics;

    if let Some(ast) = parse.ast.as_ref() {
        let import_root = Path::new(file_name)
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let loader = synth_ir::FsImportLoader { root: import_root };
        let resolved = synth_ir::resolve_imports(ast, &loader, file_name);
        diagnostics.extend(resolved.diagnostics);

        if let Ok(registry) = load_registry_tiered(args, default_registry) {
            let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
            diagnostics.extend(lowered.diagnostics);
            if let Some(board) = lowered.board.as_ref() {
                diagnostics.extend(synth_validate::run_erc(board, file_name));
            }
        }
    }

    let diag_patches: Vec<(&synth_diagnostics::Diagnostic, synth_diagnostics::Patch)> = diagnostics
        .iter()
        .filter(|d| d.severity.is_blocking())
        .filter_map(|d| d.suggested_fixes.first().cloned().map(|p| (d, p)))
        .collect();

    let mut current = source;
    let mut applied_count = 0;
    let mut skipped_count = 0;
    let mut report = Vec::new();

    let mut sorted_patches = diag_patches;
    sorted_patches.sort_by_key(|(d, p)| std::cmp::Reverse(diag_patch_anchor_offset(d, p)));

    for (diag, patch) in &sorted_patches {
        let res = match &patch.kind {
            synth_diagnostics::PatchKind::SolveSmt { .. } => {
                if enable_smt {
                    patch.apply(&current)
                } else {
                    Err(synth_diagnostics::PatchError::Unsupported(
                        "smt disabled".into(),
                    ))
                }
            }
            _ => patch.apply(&current),
        };

        match res {
            Ok(next) => {
                current = next;
                applied_count += 1;
                report.push(serde_json::json!({
                    "code": diag.code,
                    "status": "applied",
                    "kind": format!("{:?}", patch.kind)
                }));
            }
            Err(e) => {
                skipped_count += 1;
                report.push(serde_json::json!({
                    "code": diag.code,
                    "status": "skipped",
                    "reason": format!("{e}"),
                    "kind": format!("{:?}", patch.kind)
                }));
            }
        }
    }

    let re_parse = synth_parser::parse(&current, file_name.to_string());
    let mut re_diags = re_parse.diagnostics;
    if let Some(ast) = re_parse.ast.as_ref() {
        let loader = synth_ir::FsImportLoader {
            root: PathBuf::from("."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, file_name);
        re_diags.extend(resolved.diagnostics);
        if let Ok(registry) = load_registry_tiered(args, default_registry) {
            let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
            re_diags.extend(lowered.diagnostics);
            if let Some(board) = lowered.board.as_ref() {
                re_diags.extend(synth_validate::run_erc(board, file_name));
            }
        }
    }
    let is_clean = !re_diags.iter().any(|d| d.severity.is_blocking());

    Ok(serde_json::json!({
        "patched_source": current,
        "applied_count": applied_count,
        "skipped_count": skipped_count,
        "is_clean": is_clean,
        "remaining_blocking_count": re_diags.iter().filter(|d| d.severity.is_blocking()).count(),
        "report": report
    }))
}

fn execute_apply_patch(args: &Value, _default_registry: Option<&Path>) -> Result<Value, String> {
    let mut source = args["source"]
        .as_str()
        .ok_or_else(|| "Missing required parameter 'source'".to_string())?
        .to_string();

    let patches_val = args["patches"]
        .as_array()
        .ok_or_else(|| "Missing required array 'patches'".to_string())?;

    let enable_smt = args["enable_smt"].as_bool().unwrap_or(false);

    let mut patches = Vec::new();
    for val in patches_val {
        let p: synth_diagnostics::Patch = serde_json::from_value(val.clone())
            .map_err(|e| format!("Invalid patch object: {e}"))?;
        patches.push(p);
    }

    // Sort reverse byte order
    patches.sort_by_key(|p| std::cmp::Reverse(patch_anchor_offset(p)));

    let mut applied_count = 0;
    for patch in &patches {
        let res = match &patch.kind {
            synth_diagnostics::PatchKind::SolveSmt { .. } => {
                if enable_smt {
                    patch.apply(&source)
                } else {
                    Err(synth_diagnostics::PatchError::Unsupported(
                        "smt disabled".into(),
                    ))
                }
            }
            _ => patch.apply(&source),
        };
        if let Ok(next) = res {
            source = next;
            applied_count += 1;
        }
    }

    Ok(serde_json::json!({
        "patched_source": source,
        "applied_count": applied_count,
        "total_patches": patches.len()
    }))
}

fn execute_predict_patch_consequence(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");
    let filter_code = args["diagnostic_code"].as_str().unwrap_or("");

    let parse = synth_parser::parse(&source, file_name.to_string());
    let ast = parse.ast.ok_or("Parse failed")?;
    let loader = synth_ir::FsImportLoader {
        root: PathBuf::from("."),
    };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let erc_diagnostics = synth_validate::run_erc(&board, file_name);

    let mut predictions = Vec::new();
    for diag in &erc_diagnostics {
        if !filter_code.is_empty() && diag.code != filter_code {
            continue;
        }
        for patch in &diag.suggested_fixes {
            if let Some(preview) = &patch.patch_consequence_preview {
                predictions.push(serde_json::json!({
                    "diagnostic_code": diag.code,
                    "patch_kind": format!("{:?}", patch.kind),
                    "confidence": patch.confidence,
                    "preview": preview
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "predictions_count": predictions.len(),
        "predictions": predictions
    }))
}

fn execute_preview_schematic(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());

    if let Some(ast) = parse.ast.as_ref() {
        let loader = synth_ir::FsImportLoader {
            root: PathBuf::from("."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, file_name);
        let registry = load_registry_tiered(args, default_registry).ok();
        if let Some(reg) = registry {
            let lowered = synth_ir::lower(&resolved.program, &reg, file_name);
            if let Some(board) = lowered.board.as_ref() {
                let layout = synth_layout::layout(board);
                return serde_json::to_value(&layout).map_err(|e| e.to_string());
            }
        }
    }

    Err("Could not generate layout: AST or board lowering failed".into())
}

/// `synth_mutate_layout`: compile `source`/`file_path` to a `Board`,
/// compute the auto-layout, apply one `LayoutOp` from `args["op"]`,
/// and return the mutated layout as JSON. See
/// `synth_layout::ops::LayoutOp` for the op contract and
/// `crates/synth-layout/tests/ops.rs` for the exact wire shape.
fn execute_mutate_layout(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let op: synth_layout::ops::LayoutOp =
        serde_json::from_value(args["op"].clone()).map_err(|e| format!("Invalid 'op': {e}"))?;

    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    let ast = parse.ast.ok_or("Parse failed")?;
    let loader = synth_ir::FsImportLoader {
        root: PathBuf::from("."),
    };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
    let registry = load_registry_tiered(args, default_registry)
        .map_err(|e| format!("Could not load registry: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered
        .board
        .ok_or("Could not generate layout: board lowering failed")?;

    let mut layout = synth_layout::layout(&board);
    synth_layout::ops::apply_op(&mut layout, &board, op)
        .map_err(|e| format!("Could not apply layout op: {e}"))?;

    serde_json::to_value(&layout).map_err(|e| e.to_string())
}

fn execute_export(args: &Value, default_registry: Option<&Path>) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");
    let out_dir_str = args["out_dir"]
        .as_str()
        .or_else(|| args["out"].as_str())
        .ok_or_else(|| "Missing required parameter 'out_dir' (or 'out')".to_string())?;
    let out_dir = PathBuf::from(out_dir_str);

    let parse = synth_parser::parse(&source, file_name.to_string());
    let ast = parse.ast.ok_or("Parse failed")?;
    let loader = synth_ir::FsImportLoader {
        root: PathBuf::from("."),
    };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let res = synth_kicad::export(&board, &out_dir).map_err(|e| format!("Export failed: {e}"))?;

    // Aesthetic schematic ERC over the same layout the exporter used:
    // surfaced to the agent so it can repair readability regressions
    // in the same closed loop as electrical ERC findings.
    let aesthetic = {
        let layout = synth_layout::layout(&board);
        synth_kicad::check_schem_erc(&layout, &board)
            .into_iter()
            .map(|d| serde_json::json!({ "code": d.code, "title": d.title }))
            .collect::<Vec<_>>()
    };

    Ok(serde_json::json!({
        "status": "success",
        "capability_tier": "engineer-review-required",
        "compiler_version": "0.0.1",
        "registry_version": "0.0.1",
        "kicad_version": "10.0.5",
        "project_path": res.project_path,
        "schematic_path": res.schematic_path,
        "pcb_path": res.pcb_path,
        "bom_path": res.bom_path,
        "aesthetic_violations": aesthetic
    }))
}

fn execute_live_supply_chain(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let engine = synth_supply::SupplyEngine::new().map_err(|e| e.to_string())?;

    let single_pn = args["part_number"]
        .as_str()
        .or_else(|| args["mpn"].as_str())
        .or_else(|| args["lcsc_pn"].as_str());

    if let Some(pn) = single_pn {
        let handle = tokio::runtime::Handle::try_current();
        let results = if let Ok(h) = handle {
            tokio::task::block_in_place(|| h.block_on(engine.query_part(pn)))
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(engine.query_part(pn))
        }
        .map_err(|e| e.to_string())?;

        return Ok(serde_json::json!({
            "query_type": "single_part",
            "part_number": pn,
            "match_count": results.len(),
            "results": results
        }));
    }

    if let Ok(source) = get_source_from_args(args) {
        let file_name = args["file_path"].as_str().unwrap_or("board.synth");

        let parse = synth_parser::parse(&source, file_name.to_string());
        let ast = parse.ast.ok_or("Parse failed")?;
        let import_root = Path::new(file_name)
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let loader = synth_ir::FsImportLoader { root: import_root };
        let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);
        let registry = load_registry_tiered(args, default_registry)
            .map_err(|e| format!("Registry error: {e}"))?;
        let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
        let board = lowered.board.ok_or("Lowering failed")?;

        let bom_entries: Vec<synth_supply::BomQueryEntry> = board
            .components
            .iter()
            .map(|inst| synth_supply::BomQueryEntry {
                ref_des: inst.refdes.clone(),
                part_id: inst
                    .part
                    .as_ref()
                    .map_or_else(String::new, |p| p.id.to_string()),
                mpn: inst.part.as_ref().and_then(|p| p.mpn.clone()),
                lcsc_pn: inst.part.as_ref().and_then(|p| p.lcsc_pn.clone()),
            })
            .collect();

        let handle = tokio::runtime::Handle::try_current();
        let bom_map = if let Ok(h) = handle {
            tokio::task::block_in_place(|| h.block_on(engine.query_bom(&bom_entries)))
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(engine.query_bom(&bom_entries))
        }
        .map_err(|e| e.to_string())?;

        return Ok(serde_json::json!({
            "query_type": "bom",
            "component_count": bom_entries.len(),
            "bom_supply": bom_map
        }));
    }

    Err("Provide either 'part_number' (or 'mpn'/'lcsc_pn') or 'source'/'file_path' to query supply chain.".to_string())
}

fn get_source_from_args(args: &Value) -> Result<String, String> {
    if let Some(src) = args["source"].as_str() {
        return Ok(src.to_string());
    }
    if let Some(path_str) = args["file_path"].as_str() {
        return std::fs::read_to_string(path_str)
            .map_err(|e| format!("Could not read source file '{path_str}': {e}"));
    }
    Err("Either 'source' string or 'file_path' must be provided".to_string())
}

fn diag_patch_anchor_offset(
    d: &synth_diagnostics::Diagnostic,
    p: &synth_diagnostics::Patch,
) -> u32 {
    use synth_diagnostics::PatchKind;
    match &p.kind {
        PatchKind::ReplaceRange { range, .. } | PatchKind::DeleteRange { range } => {
            range.byte_start
        }
        PatchKind::InsertAt { at, .. } => *at,
        PatchKind::SolveSmt {
            target_range: Some(range),
            ..
        } => range.byte_start,
        PatchKind::AddStatement { .. }
        | PatchKind::RemoveStatement { .. }
        | PatchKind::SolveSmt { .. } => d
            .location
            .as_ref()
            .map_or(u32::MAX, |loc| loc.span.byte_start),
    }
}

fn patch_anchor_offset(p: &synth_diagnostics::Patch) -> u32 {
    use synth_diagnostics::PatchKind;
    match &p.kind {
        PatchKind::ReplaceRange { range, .. } | PatchKind::DeleteRange { range } => {
            range.byte_start
        }
        PatchKind::InsertAt { at, .. } => *at,
        PatchKind::SolveSmt {
            target_range: Some(range),
            ..
        } => range.byte_start,
        PatchKind::AddStatement { .. }
        | PatchKind::RemoveStatement { .. }
        | PatchKind::SolveSmt { .. } => u32::MAX,
    }
}

fn execute_place_with_hints(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    if parse.has_errors() {
        return Ok(serde_json::json!({
            "status": "error",
            "error": "Parse phase produced blocking errors",
            "diagnostics": parse.diagnostics
        }));
    }

    let ast = parse.ast.ok_or("No AST produced")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;

    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let hints: Vec<synth_place::ExternalHint> = if let Some(hints_val) = args.get("hints") {
        serde_json::from_value(hints_val.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };

    match synth_place::place_with_hints(&board, &hints) {
        Ok((mut placement, report)) => {
            let sidecar_opt: Option<PathBuf> = args
                .get("layout_file_path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .or_else(|| {
                    let sidecar_candidate = PathBuf::from(format!("{file_name}.layout.toml"));
                    if sidecar_candidate.exists() {
                        Some(sidecar_candidate)
                    } else {
                        None
                    }
                });
            if let Some(sc_path) = sidecar_opt.as_deref() {
                if let Some(sidecar) = synth_layout::sidecar::SidecarLayout::load_from_file(sc_path)
                {
                    synth_place::apply_sidecar_overrides(&board, &mut placement, &sidecar);
                }
            }

            #[allow(clippy::cast_precision_loss)]
            let board_w_mm = (placement.board_outline.width_nm() as f64) / 1_000_000.0;
            #[allow(clippy::cast_precision_loss)]
            let board_h_mm = (placement.board_outline.height_nm() as f64) / 1_000_000.0;

            let routing = synth_route::route(&board, &placement);
            let profile_str = args["profile"].as_str().unwrap_or("jlcpcb_standard");
            let profile = match profile_str {
                "jlcpcb_standard" | "jlcpcb" => synth_drc::ManufacturerProfile::jlc_standard(),
                custom_path => {
                    synth_drc::ManufacturerProfile::from_toml_file(Path::new(custom_path))
                        .unwrap_or_else(|_| synth_drc::ManufacturerProfile::jlc_standard())
                }
            };
            let drc_report = synth_drc::check(&board, &placement, &routing, &profile);

            Ok(serde_json::json!({
                "status": "ok",
                "board_size_mm": [board_w_mm, board_h_mm],
                "component_placements": placement.components,
                "hint_satisfaction": report,
                "drc_clean": drc_report.is_clean(),
                "violations": drc_report.violations,
                "violation_count": drc_report.violations.len(),
                "unrouted_nets": routing.unrouted_nets.len()
            }))
        }
        Err(e) => Ok(serde_json::json!({
            "status": "error",
            "error": format!("{e}"),
            "diagnostics": e.to_diagnostics(&board, file_name)
        })),
    }
}

fn execute_describe_placement(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    if parse.has_errors() {
        return Ok(serde_json::json!({
            "status": "error",
            "error": "Parse phase produced blocking errors",
            "diagnostics": parse.diagnostics
        }));
    }

    let ast = parse.ast.ok_or("No AST produced")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;

    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let placement = synth_place::place(&board).map_err(|e| format!("Placement failed: {e}"))?;

    let desc = synth_place::describe_placement(&board, &placement);
    serde_json::to_value(desc).map_err(|e| format!("Serialization error: {e}"))
}

fn execute_read_layout_overrides(args: &Value) -> Result<Value, String> {
    let file_path_str = args["layout_file_path"]
        .as_str()
        .ok_or("Missing required 'layout_file_path'")?;
    let path = Path::new(file_path_str);
    if !path.exists() {
        return Ok(serde_json::json!({
            "status": "ok",
            "exists": false,
            "overrides": {}
        }));
    }
    let sidecar = synth_layout::sidecar::SidecarLayout::load_from_file(path)
        .ok_or_else(|| format!("Failed to parse sidecar TOML at '{}'", path.display()))?;
    Ok(serde_json::json!({
        "status": "ok",
        "exists": true,
        "schema_version": sidecar.schema_version,
        "components": sidecar.components,
        "forced_net_labels": sidecar.forced_net_labels
    }))
}

fn execute_write_layout_override(args: &Value) -> Result<Value, String> {
    let file_path_str = args["layout_file_path"]
        .as_str()
        .ok_or("Missing required 'layout_file_path'")?;
    let refdes = args["refdes"]
        .as_str()
        .ok_or("Missing required 'refdes'")?
        .to_string();
    let x_mm = args["x_mm"]
        .as_f64()
        .ok_or("Missing required numeric 'x_mm'")?;
    let y_mm = args["y_mm"]
        .as_f64()
        .ok_or("Missing required numeric 'y_mm'")?;
    let rotation = args["rotation"].as_u64().unwrap_or(0) as u32;

    let source = match args["source"].as_str() {
        Some("human_drag") => synth_layout::sidecar::OverrideSource::HumanDrag,
        _ => synth_layout::sidecar::OverrideSource::Agent,
    };
    let priority = match args["priority"].as_str() {
        Some("hard") => synth_layout::sidecar::OverridePriority::Hard,
        _ => synth_layout::sidecar::OverridePriority::Soft,
    };

    let path = Path::new(file_path_str);
    let mut sidecar = if path.exists() {
        synth_layout::sidecar::SidecarLayout::load_from_file(path).unwrap_or_default()
    } else {
        synth_layout::sidecar::SidecarLayout::default()
    };

    let placement = synth_layout::sidecar::SidecarPlacement {
        x: x_mm,
        y: y_mm,
        rotation,
        source,
        priority,
        timestamp: None,
    };

    sidecar.merge_override(refdes.clone(), placement);
    sidecar
        .save_to_file(path)
        .map_err(|e| format!("Failed to save sidecar TOML: {e}"))?;

    Ok(serde_json::json!({
        "status": "ok",
        "saved_path": path.display().to_string(),
        "updated_refdes": refdes,
        "effective_override": sidecar.components.get(&refdes)
    }))
}

/// Parse `net_constraints` from the tool arguments: warns on unknown
/// net names and returns the tightest requested width/clearance in
/// nanometres (plus any warnings, in declaration order).
#[allow(clippy::cast_possible_truncation)]
fn collect_route_constraints(
    args: &Value,
    board: &synth_ir::Board,
) -> (Option<i64>, Option<i64>, Vec<synth_diagnostics::Diagnostic>) {
    let mut diagnostics = Vec::new();
    let valid_net_names: std::collections::HashSet<&str> =
        board.nets.iter().map(|n| n.name.as_str()).collect();

    let mut min_width_nm: Option<i64> = None;
    let mut min_clearance_nm: Option<i64> = None;

    let Some(net_constraints) = args["net_constraints"].as_array() else {
        return (min_width_nm, min_clearance_nm, diagnostics);
    };
    for constraint in net_constraints {
        let Some(net_name) = constraint["net"].as_str() else {
            continue;
        };
        if !valid_net_names.contains(net_name) {
            diagnostics.push(
                synth_diagnostics::DiagnosticBuilder::new(
                    "W-SYNTH-ROUTE-CONSTRAINT-001",
                    synth_diagnostics::Severity::Warning,
                    format!("Unknown net name '{net_name}' in routing constraints"),
                )
                .build(),
            );
        }
        if let Some(w_mm) = constraint["width_mm"].as_f64() {
            let w_nm = (w_mm * 1_000_000.0) as i64;
            min_width_nm = Some(min_width_nm.map_or(w_nm, |cur| cur.max(w_nm)));
        }
        if let Some(c_mm) = constraint["clearance_mm"].as_f64() {
            let c_nm = (c_mm * 1_000_000.0) as i64;
            min_clearance_nm = Some(min_clearance_nm.map_or(c_nm, |cur| cur.max(c_nm)));
        }
    }
    (min_width_nm, min_clearance_nm, diagnostics)
}

fn execute_route_with_constraints(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let source = get_source_from_args(args)?;
    let file_name = args["file_path"].as_str().unwrap_or("board.synth");

    let parse = synth_parser::parse(&source, file_name.to_string());
    if parse.has_errors() {
        return Ok(serde_json::json!({
            "status": "error",
            "error": "Parse phase produced blocking errors",
            "diagnostics": parse.diagnostics
        }));
    }

    let ast = parse.ast.ok_or("No AST produced")?;
    let import_root = Path::new(file_name)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file_name);

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;

    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    let board = lowered.board.ok_or("Lowering failed")?;

    let (min_width_nm, min_clearance_nm, diagnostics) = collect_route_constraints(args, &board);

    let sidecar_opt: Option<PathBuf> = args
        .get("layout_file_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| {
            let sidecar_candidate = PathBuf::from(format!("{file_name}.layout.toml"));
            if sidecar_candidate.exists() {
                Some(sidecar_candidate)
            } else {
                None
            }
        });

    let mut placement = synth_place::place(&board).map_err(|e| format!("Placement failed: {e}"))?;
    if let Some(sc_path) = sidecar_opt.as_deref() {
        if let Some(sidecar) = synth_layout::sidecar::SidecarLayout::load_from_file(sc_path) {
            synth_place::apply_sidecar_overrides(&board, &mut placement, &sidecar);
        }
    }

    let routing = match (min_width_nm, min_clearance_nm) {
        (Some(w), Some(c)) => synth_route::route_with_profile(&board, &placement, w, c),
        (Some(w), None) => synth_route::route_with_profile(&board, &placement, w, 127_000),
        (None, Some(c)) => synth_route::route_with_profile(&board, &placement, 127_000, c),
        (None, None) => synth_route::route(&board, &placement),
    };

    let total_trace_length_mm: f64 = routing
        .segments
        .iter()
        .map(|t| {
            #[allow(clippy::cast_precision_loss)]
            let dx = (t.end.x_nm - t.start.x_nm) as f64;
            #[allow(clippy::cast_precision_loss)]
            let dy = (t.end.y_nm - t.start.y_nm) as f64;
            (dx * dx + dy * dy).sqrt() / 1_000_000.0
        })
        .sum();
    let total_vias: usize = routing.vias.len();
    let unrouted_nets = routing.unrouted_nets.len();

    let profile_str = args["profile"].as_str().unwrap_or("jlcpcb_standard");
    let profile = match profile_str {
        "jlcpcb_standard" | "jlcpcb" => synth_drc::ManufacturerProfile::jlc_standard(),
        custom_path => synth_drc::ManufacturerProfile::from_toml_file(Path::new(custom_path))
            .unwrap_or_else(|_| synth_drc::ManufacturerProfile::jlc_standard()),
    };
    let drc_report = synth_drc::check(&board, &placement, &routing, &profile);

    Ok(serde_json::json!({
        "status": if unrouted_nets == 0 && drc_report.is_clean() { "ok" } else { "partial" },
        "segments": routing.segments,
        "vias": routing.vias,
        "unrouted_nets": routing.unrouted_nets,
        "total_trace_length_mm": total_trace_length_mm,
        "total_vias": total_vias,
        "drc_clean": drc_report.is_clean(),
        "violations": drc_report.violations,
        "violation_count": drc_report.violations.len(),
        "diagnostics": diagnostics
    }))
}

fn execute_search_registry_vector(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let query = args["query"]
        .as_str()
        .ok_or_else(|| "Missing required parameter 'query'".to_string())?;
    let top_k = args["top_k"].as_u64().unwrap_or(5) as usize;

    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;

    let index = synth_registry::VectorSearchIndex::build(&registry);
    let results = index.search(query, top_k);

    Ok(serde_json::json!({
        "status": "ok",
        "query": query,
        "results_count": results.len(),
        "parts": results
    }))
}

// Kept as `Result<Value, String>` to match every other tool handler's
// signature in the `call_tool` dispatch table, even though this one
// never fails.
#[allow(clippy::unnecessary_wraps)]
fn execute_evaluate_thermal_si(args: &Value) -> Result<Value, String> {
    let width_mm = args["trace_width_mm"].as_f64().unwrap_or(0.20);
    let height_mm = args["dielectric_height_mm"].as_f64().unwrap_or(0.16);
    let thickness_mm = args["trace_thickness_mm"].as_f64().unwrap_or(0.035);
    let er = args["er"].as_f64().unwrap_or(4.3);
    let power_watts = args["power_watts"].as_f64().unwrap_or(0.5);
    let r_theta_ja = args["r_theta_ja"].as_f64().unwrap_or(50.0);

    let params = synth_geometry::MicrostripParams {
        width_mm,
        height_mm,
        thickness_mm,
        er,
    };

    let microstrip_si = synth_geometry::calculate_microstrip_z0(&params);
    let stripline_si =
        synth_geometry::calculate_stripline_z0(width_mm, height_mm, thickness_mm, er);
    let thermal = synth_geometry::estimate_component_thermal("U1", power_watts, r_theta_ja, 25.0);

    Ok(serde_json::json!({
        "status": "ok",
        "microstrip": microstrip_si,
        "stripline": stripline_si,
        "thermal": thermal
    }))
}

// Kept as `Result<Value, String>` to match every other tool handler's
// signature in the `call_tool` dispatch table, even though this one
// never fails.
#[allow(dead_code, clippy::unnecessary_wraps)]
fn execute_export_multiboard(args: &Value) -> Result<Value, String> {
    let name = args["project_name"]
        .as_str()
        .unwrap_or("multi_board_system");
    let mut project = synth_ir::MultiBoardProject::new(name);

    if let Some(mappings) = args["interboard_mappings"].as_array() {
        for m in mappings {
            if let (Some(from_b), Some(from_r), Some(from_p), Some(to_b), Some(to_r), Some(to_p)) = (
                m["from_board"].as_str(),
                m["from_refdes"].as_str(),
                m["from_pin"].as_str(),
                m["to_board"].as_str(),
                m["to_refdes"].as_str(),
                m["to_pin"].as_str(),
            ) {
                project.add_mapping(synth_ir::InterBoardPinMapping {
                    from_board: from_b.to_string(),
                    from_refdes: from_r.to_string(),
                    from_pin: from_p.to_string(),
                    to_board: to_b.to_string(),
                    to_refdes: to_r.to_string(),
                    to_pin: to_p.to_string(),
                });
            }
        }
    }

    let validation = project.validate_multiboard_system();

    Ok(serde_json::json!({
        "status": if validation.is_clean { "ok" } else { "warning" },
        "project_name": name,
        "total_mappings": validation.total_mappings,
        "matched_pins": validation.matched_pins,
        "matching_percentage": validation.matching_percentage,
        "diagnostics": validation.diagnostics
    }))
}

fn execute_cloud_mcp_endpoint(args: &Value) -> Result<Value, String> {
    let client_id = args["client_id"].as_str().unwrap_or("cloud_mcp_session");
    let payload = args["request_payload"].as_str();

    if let Some(req_str) = payload {
        let res_json =
            crate::cloud_endpoint::CloudMcpServer::handle_jsonrpc_request(req_str, client_id)?;
        Ok(serde_json::from_str(&res_json).map_err(|e| e.to_string())?)
    } else {
        Ok(serde_json::json!({
            "status": "ok",
            "transport": "HTTP SSE / JSON-RPC 2.0",
            "rate_limiter": "Active (60 req/min)",
            "ipc_audit": "IPC Class 3 High Reliability",
            "client_id": client_id
        }))
    }
}

/// Resolve the Tier-2 (per-user) registry directory from args or environment.
fn resolve_user_registry(args: &Value) -> Result<PathBuf, String> {
    if let Some(p) = args["user_registry"].as_str() {
        return Ok(PathBuf::from(p));
    }
    synth_registry::user_registry_dir().ok_or_else(|| {
        "no Tier-2 user registry configured; pass 'user_registry' or set SYNTH_USER_REGISTRY_DIR".to_string()
    })
}

/// Import a component into the Tier-2 registry (R15.4 LCSC / R15.5 KiCad).
fn execute_import_part(args: &Value, _default_registry: Option<&Path>) -> Result<Value, String> {
    let source = args["source"].as_str().unwrap_or("lcsc");
    let user_dir = resolve_user_registry(args)?;
    std::fs::create_dir_all(&user_dir).map_err(|e| format!("cannot create user registry: {e}"))?;

    match source {
        "lcsc" => import_part_lcsc(args, &user_dir),
        "kicad" => import_part_kicad(args, &user_dir),
        "kicad-zip" => import_part_kicad_zip(args, &user_dir),
        other => Err(format!(
            "unknown import source '{other}'; expected 'lcsc', 'kicad', or 'kicad-zip'"
        )),
    }
}

fn import_part_lcsc(args: &Value, user_dir: &Path) -> Result<Value, String> {
    let code = args["code"].as_str().ok_or("lcsc import requires 'code'")?;
    let raw = match args["from_file"].as_str() {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("from_file read failed: {e}"))?,
        None => return Err(
            "live LCSC CAD fetch is unavailable via MCP; pass 'from_file' with cached EasyEDA JSON"
                .into(),
        ),
    };
    let comp = synth_registry::easyeda::parse_easyeda(&raw)
        .map_err(|e| format!("failed to parse EasyEDA JSON: {e}"))?;
    let id = code.to_lowercase();
    let pins = synth_registry::easyeda::extract_pins(&comp);
    let module = synth_registry::easyeda::to_kicad_mod(&comp, &id);

    let fp_dir = args["footprint_dir"]
        .as_str()
        .map_or_else(|| user_dir.join("footprints"), PathBuf::from);
    std::fs::create_dir_all(&fp_dir).map_err(|e| e.to_string())?;
    let pretty = fp_dir.join(format!("{id}.pretty"));
    std::fs::create_dir_all(&pretty).map_err(|e| e.to_string())?;
    let kmod_path = pretty.join(format!("{id}.kicad_mod"));
    std::fs::write(&kmod_path, &module).map_err(|e| e.to_string())?;

    let toml_str = synth_registry::easyeda::generate_part_toml(
        &id,
        code,
        "",
        "",
        &format!("https://lcsc.com/p/{code}.html"),
        &pins,
    );
    let part_path = user_dir.join(format!("{id}.synth.toml"));
    std::fs::write(&part_path, toml_str).map_err(|e| e.to_string())?;

    let pins_json: Vec<serde_json::Value> = pins
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p,
                "number": p,
                "electrical_type": "bidirectional"
            })
        })
        .collect();

    Ok(serde_json::json!({
        "status": "imported",
        "source": "lcsc",
        "part_id": id,
        "part_path": part_path.display().to_string(),
        "footprint_path": kmod_path.display().to_string(),
        "pin_count": pins.len(),
        "pins": pins_json
    }))
}

fn import_part_kicad(args: &Value, user_dir: &Path) -> Result<Value, String> {
    use std::fmt::Write as _;

    let lib_id = args["lib_id"]
        .as_str()
        .ok_or("kicad import requires 'lib_id'")?;
    let pins = synth_layout::kicad_lib_loader::physical_pins(lib_id).ok_or_else(|| {
        format!("could not read physical pins for '{lib_id}'; is KiCad installed and on the symbol path?")
    })?;

    let default_id = lib_id
        .split_once(':')
        .map_or(lib_id, |(_, s)| s)
        .to_lowercase()
        .replace([' ', '-', '.', '/'], "_");
    let part_id = args["id"].as_str().map_or(default_id, str::to_string);
    let kind = infer_kind(lib_id);
    let footprint = args["footprint"]
        .as_str()
        .map_or_else(|| lib_id.to_string(), str::to_string);

    let mut out = String::new();
    let _ = writeln!(out, "id = \"{part_id}\"");
    let _ = writeln!(out, "kind = \"{kind}\"");
    out.push_str("description = \"Imported from KiCad stock symbol\"\n");
    let _ = writeln!(out, "kicad_symbol = \"{lib_id}\"");
    let _ = writeln!(out, "kicad_footprint = \"{footprint}\"");
    out.push_str("\n[provenance]\n");
    out.push_str("source = \"imported\"\n");
    out.push_str("reviewed_by = \"\"\n");
    out.push_str("\n[[pins]]\n");
    for p in &pins {
        let _ = writeln!(out, "name = \"{}\"", p.name);
        let _ = writeln!(out, "number = \"{}\"", p.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{}\"",
            map_kicad_electrical_type(&p.electrical_type)
        );
        out.push_str("\n[[pins]]\n");
    }
    if !pins.is_empty() {
        out.truncate(out.len() - "\n[[pins]]\n".len());
    }

    let part_path = user_dir.join(format!("{part_id}.synth.toml"));
    std::fs::write(&part_path, out).map_err(|e| e.to_string())?;

    let pins_json: Vec<serde_json::Value> = pins
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "number": p.number,
                "electrical_type": map_kicad_electrical_type(&p.electrical_type)
            })
        })
        .collect();

    Ok(serde_json::json!({
        "status": "imported",
        "source": "kicad",
        "part_id": part_id,
        "part_path": part_path.display().to_string(),
        "kind": kind,
        "pin_count": pins.len(),
        "pins": pins_json
    }))
}

/// Import a part from a SnapEDA/UltraLibrarian "Export to KiCad" zip
/// (or any single-part KiCad-format export zip) already present on
/// disk. Neither vendor offers a public API and both prohibit
/// automated access to their sites in their Terms of Service, so this
/// never contacts snapeda.com/ultralibrarian.com — the agent (or the
/// human it's working with) downloads the zip through their own
/// browser session first, and this tool only reads that local file.
fn import_part_kicad_zip(args: &Value, user_dir: &Path) -> Result<Value, String> {
    use std::fmt::Write as _;

    let zip_path = args["zip_path"]
        .as_str()
        .ok_or("kicad-zip import requires 'zip_path' (path to a downloaded .zip)")?;
    let bytes = std::fs::read(zip_path).map_err(|e| format!("could not read {zip_path}: {e}"))?;
    let import = synth_layout::kicad_zip::parse_kicad_zip(&bytes)?;

    let default_id = import
        .symbol_name
        .to_lowercase()
        .replace([' ', '-', '.', '/'], "_");
    let part_id = args["id"].as_str().map_or(default_id, str::to_string);
    let kind = infer_kind(&import.symbol_name);

    let mut out = String::new();
    let _ = writeln!(out, "id = \"{part_id}\"");
    let _ = writeln!(out, "kind = \"{kind}\"");
    let _ = writeln!(
        out,
        "description = \"Imported from a KiCad-format export zip ({zip_path})\""
    );

    let mut footprint_path: Option<PathBuf> = None;
    if let Some(fp_text) = &import.footprint_text {
        let fp_dir = user_dir.join("footprints");
        let pretty = fp_dir.join(format!("{part_id}.pretty"));
        std::fs::create_dir_all(&pretty).map_err(|e| e.to_string())?;
        let kmod = pretty.join(format!("{part_id}.kicad_mod"));
        std::fs::write(&kmod, fp_text).map_err(|e| e.to_string())?;
        let _ = writeln!(out, "kicad_footprint = \"{part_id}:{part_id}\"");
        footprint_path = Some(kmod);
    }
    let _ = writeln!(out);

    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for pin in &import.pins {
        if !pin.name.is_empty() {
            *name_counts.entry(pin.name.as_str()).or_insert(0) += 1;
        }
    }
    for pin in &import.pins {
        let name = if pin.name.is_empty() {
            pin.number.clone()
        } else if name_counts.get(pin.name.as_str()).copied().unwrap_or(0) > 1 {
            format!("{}_{}", pin.name, pin.number)
        } else {
            pin.name.clone()
        };
        let _ = writeln!(out, "[[pins]]");
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "number = \"{}\"", pin.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{}\"",
            map_kicad_electrical_type(&pin.electrical_type)
        );
        let _ = writeln!(out, "required = false");
    }
    out.push_str("\n[provenance]\n");
    out.push_str("source = \"imported\"\n");
    out.push_str("generator = \"synth-part-import-zip 0.1\"\n");
    out.push_str("reviewed_by = \"\"\n");

    let part_path = user_dir.join(format!("{part_id}.synth.toml"));
    std::fs::write(&part_path, out).map_err(|e| e.to_string())?;

    let pins_json: Vec<serde_json::Value> = import
        .pins
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "number": p.number,
                "electrical_type": map_kicad_electrical_type(&p.electrical_type)
            })
        })
        .collect();

    Ok(serde_json::json!({
        "status": "imported",
        "source": "kicad-zip",
        "part_id": part_id,
        "part_path": part_path.display().to_string(),
        "footprint_path": footprint_path.map(|p| p.display().to_string()),
        "kind": kind,
        "pin_count": import.pins.len(),
        "pins": pins_json
    }))
}

/// Persist a human/agent-authored part into the Tier-2 registry after validation.
fn execute_author_part(args: &Value, _default_registry: Option<&Path>) -> Result<Value, String> {
    let toml_src = args["part_toml"]
        .as_str()
        .ok_or("author_part requires 'part_toml' (TOML string)")?;
    let part: synth_registry::Part =
        toml::from_str(toml_src).map_err(|e| format!("invalid part TOML: {e}"))?;

    if part.id.as_str().is_empty() {
        return Err("part TOML must include a non-empty 'id'".into());
    }
    if part.pins.is_empty() {
        return Err("part TOML must declare at least one [[pins]] entry".into());
    }
    for p in &part.pins {
        if p.name.is_empty() || p.number.0.is_empty() {
            return Err("each pin must have a non-empty name and number".into());
        }
    }

    let user_dir = resolve_user_registry(args)?;
    std::fs::create_dir_all(&user_dir).map_err(|e| format!("cannot create user registry: {e}"))?;

    let serialized = toml::to_string_pretty(&part).map_err(|e| e.to_string())?;
    let path = user_dir.join(format!("{}.synth.toml", part.id.as_str()));
    std::fs::write(&path, serialized).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "status": "authored",
        "part_id": part.id.as_str(),
        "path": path.display().to_string(),
        "pin_count": part.pins.len()
    }))
}

/// Best-effort keyword search of LCSC for candidate parts with live stock/price.
/// Degrades gracefully: if the network is unavailable (or returns nothing), it
/// falls back to searching the local Tier-1 + Tier-2 registries.
fn execute_search_registry_web(
    args: &Value,
    default_registry: Option<&Path>,
) -> Result<Value, String> {
    let query = args["query"].as_str().unwrap_or("").trim();
    if query.is_empty() {
        return Err("search requires a non-empty 'query'".into());
    }

    let network: Result<Option<synth_supply::types::SupplyStatus>, String> = (|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let dist = synth_supply::lcsc::LcscDistributor::new();
        rt.block_on(dist.query(query)).map_err(|e| e.to_string())
    })();

    let mut network_error: Option<String> = None;
    let mut network_result: Option<synth_supply::types::SupplyStatus> = None;
    match network {
        Ok(opt) => network_result = opt,
        Err(e) => network_error = Some(e),
    }

    if let Some(s) = network_result {
        return Ok(serde_json::json!({
            "status": "ok",
            "source": "lcsc",
            "query": query,
            "part_number": s.part_number,
            "distributor": s.distributor,
            "in_stock": s.in_stock,
            "stock_qty": s.stock_qty,
            "moq": s.moq,
            "unit_price_usd": s.unit_price_usd,
            "lifecycle": format!("{:?}", s.lifecycle)
        }));
    }

    // Offline fallback: search local Tier-1 + Tier-2 registries.
    let mut local = Vec::new();
    let tier1 = resolve_registry(args, default_registry);
    if let Ok(m) = search_registry_dir(&tier1, query, "") {
        local.extend(m);
    }
    if let Some(tier2) = synth_registry::user_registry_dir() {
        if tier2 != tier1 {
            if let Ok(m) = search_registry_dir(&tier2, query, "") {
                local.extend(m);
            }
        }
    }
    local.dedup_by(|a, b| a["id"] == b["id"]);
    let mut seen = std::collections::HashSet::new();
    local.retain(|p| seen.insert(p["id"].as_str().unwrap_or("").to_string()));
    if local.len() > 30 {
        local.truncate(30);
    }

    let mut resp = serde_json::json!({
        "status": if local.is_empty() { "no_results" } else { "ok" },
        "source": "local_registry",
        "query": query,
        "returned_matches": local.len(),
        "parts": local
    });
    if let Some(e) = network_error {
        resp["network_error"] = serde_json::json!(e);
    }

    Ok(resp)
}

/// Map a KiCad electrical type string to a synth `ElectricalType` variant name.
fn map_kicad_electrical_type(kt: &str) -> String {
    match kt.to_uppercase().as_str() {
        "INPUT" => "input",
        "OUTPUT" => "output",
        "BIDIR" | "BIDIRECTIONAL" => "bidirectional",
        "TRISTATE" => "tristate",
        "PASSIVE" => "passive",
        "FREE" => "free",
        "POWER_IN" | "PWRIN" => "power_input",
        "POWER_OUT" | "PWROUT" => "power_output",
        "OPENCOLLECTOR" => "open_collector",
        "OPENEMITTER" => "open_emitter",
        "NC" | "NOTCONNECTED" => "nc",
        _ => "unclassified",
    }
    .to_string()
}

/// Heuristically infer a synth `kind` from a KiCad library/symbol id.
fn infer_kind(lib_id: &str) -> String {
    let lower = lib_id.to_lowercase();
    if lower.contains("resistor") || lower.starts_with("device:r") {
        "resistor"
    } else if lower.contains("capacitor") || lower.starts_with("device:c") {
        "capacitor"
    } else if lower.contains("inductor") || lower.starts_with("device:l") {
        "inductor"
    } else if lower.contains("diode") || lower.contains("led") {
        "diode"
    } else if lower.contains("transistor") || lower.contains("fet") || lower.contains("mosfet") {
        "transistor"
    } else if lower.contains("connector") || lower.contains("conn") {
        "connector"
    } else if lower.contains("crystal") || lower.contains("oscillator") {
        "crystal"
    } else if lower.contains("regulator") || lower.contains("ldo") {
        "regulator"
    } else {
        // "74"/"ic"/"mcu"/"microcontroller" and anything else default to "ic".
        "ic"
    }
    .to_string()
}

#[cfg(test)]
mod import_tool_tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("synth_mcp_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn author_part_writes_to_user_registry() {
        let dir = scratch("author");
        let toml_src = r#"
id = "test_res_1k"
kind = "resistor"
description = "1k resistor"
[[pins]]
name = "1"
number = "1"
electrical_type = "passive"
[[pins]]
name = "2"
number = "2"
electrical_type = "passive"
"#;
        let res = call_tool(
            "synth_author_part",
            &json!({ "part_toml": toml_src, "user_registry": dir.to_str().unwrap() }),
            None,
        )
        .expect("author_part should succeed");
        assert_eq!(res["status"], "authored");
        assert_eq!(res["part_id"], "test_res_1k");
        assert!(dir.join("test_res_1k.synth.toml").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn author_part_rejects_missing_id() {
        let dir = scratch("author_bad");
        let toml_src = r#"
kind = "ic"
[[pins]]
name = "1"
number = "1"
electrical_type = "input"
"#;
        let res = call_tool(
            "synth_author_part",
            &json!({ "part_toml": toml_src, "user_registry": dir.to_str().unwrap() }),
            None,
        );
        assert!(res.is_err(), "missing id must be rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_part_kicad_writes_user_registry() {
        let symbol_dir = std::env::var("KICAD_SYMBOL_DIR").map_or_else(
            |_| std::path::PathBuf::from("/usr/share/kicad/symbols"),
            std::path::PathBuf::from,
        );
        if !symbol_dir.is_dir() {
            eprintln!(
                "skipping import_part_kicad_writes_user_registry: {symbol_dir:?} not found (requires KiCad stock symbols)"
            );
            return;
        }
        let dir = scratch("import_kicad");
        let () = std::env::set_var("KICAD_SYMBOL_DIR", &symbol_dir);
        let res = call_tool(
            "synth_import_part",
            &json!({ "source": "kicad", "lib_id": "Device:R", "user_registry": dir.to_str().unwrap() }),
            None,
        )
        .expect("kicad import should succeed");
        assert_eq!(res["status"], "imported");
        assert_eq!(res["source"], "kicad");
        assert!(res["pin_count"].as_u64().unwrap_or(0) >= 2, "R has 2 pins");
        assert!(dir.join("r.synth.toml").exists(), "expected r.synth.toml");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Build a minimal single-part `.kicad_sym` + `.kicad_mod` zip on
    /// disk, matching the shape SnapEDA/UltraLibrarian document
    /// (symbol at zip root, footprint under a `<lib>.pretty/` dir).
    fn write_sample_kicad_zip(dir: &Path) -> PathBuf {
        use std::io::Write as _;

        let sym = r#"(kicad_symbol_lib (version 20211014) (generator kicad_symbol_editor)
  (symbol "TestPart"
    (symbol "TestPart_1_1"
      (pin input line (at -5.08 0 0) (length 2.54)
        (name "A" (effects (font (size 1.27 1.27))))
        (number "1" (effects (font (size 1.27 1.27))))
      )
      (pin output line (at 5.08 0 180) (length 2.54)
        (name "B" (effects (font (size 1.27 1.27))))
        (number "2" (effects (font (size 1.27 1.27))))
      )
    )
  )
)
"#;
        let modu = r#"(footprint "TestPart" (version 20211014) (generator pcbnew) (layer "F.Cu")
  (pad "1" smd rect (at -1 0) (size 0.6 0.6) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 1 0) (size 0.6 0.6) (layers "F.Cu" "F.Paste" "F.Mask"))
)
"#;
        let zip_path = dir.join("testpart_export.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zw.start_file("TestPart.kicad_sym", opts).unwrap();
        zw.write_all(sym.as_bytes()).unwrap();
        zw.start_file("TestPart.pretty/TestPart.kicad_mod", opts)
            .unwrap();
        zw.write_all(modu.as_bytes()).unwrap();
        zw.finish().unwrap();
        zip_path
    }

    #[test]
    fn import_part_kicad_zip_writes_user_registry_and_footprint() {
        let dir = scratch("import_kicad_zip");
        let zip_path = write_sample_kicad_zip(&dir);
        let res = call_tool(
            "synth_import_part",
            &json!({
                "source": "kicad-zip",
                "zip_path": zip_path.to_str().unwrap(),
                "user_registry": dir.to_str().unwrap()
            }),
            None,
        )
        .expect("kicad-zip import should succeed");
        assert_eq!(res["status"], "imported");
        assert_eq!(res["source"], "kicad-zip");
        assert_eq!(res["pin_count"].as_u64(), Some(2));
        assert!(dir.join("testpart.synth.toml").exists());
        assert!(dir
            .join("footprints/testpart.pretty/testpart.kicad_mod")
            .exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_part_kicad_zip_rejects_missing_zip_path() {
        let dir = scratch("import_kicad_zip_bad");
        let res = call_tool(
            "synth_import_part",
            &json!({ "source": "kicad-zip", "user_registry": dir.to_str().unwrap() }),
            None,
        );
        assert!(res.is_err(), "missing zip_path must be rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_part_lcsc_rejects_missing_from_file() {
        let dir = scratch("import_lcsc");
        let res = call_tool(
            "synth_import_part",
            &json!({ "source": "lcsc", "code": "C2040", "user_registry": dir.to_str().unwrap() }),
            None,
        );
        assert!(res.is_err(), "live LCSC fetch must be rejected via MCP");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_registry_web_rejects_empty_query() {
        let res = call_tool("synth_search_registry_web", &json!({ "query": "" }), None);
        assert!(res.is_err(), "empty query must be rejected");
    }

    #[test]
    fn search_registry_web_degrades_to_local() {
        // Network is unavailable in the sandbox, so this must fall back to the
        // local registry and still return a well-formed response.
        let res = call_tool(
            "synth_search_registry_web",
            &json!({ "query": "esp32" }),
            None,
        )
        .expect("search should not hard-fail offline");
        let source = res["source"].as_str().unwrap_or("");
        assert!(
            source == "lcsc" || source == "local_registry",
            "expected a graceful source, got {source:?}"
        );
    }
}

#[cfg(test)]
mod language_reference_tests {
    use super::*;

    #[test]
    fn language_reference_is_listed() {
        let names: Vec<String> = list_tools().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"synth_language_reference".to_string()));
    }

    #[test]
    fn language_reference_returns_grammar_and_examples() {
        let result = execute_language_reference(&serde_json::json!({}));
        assert!(result["grammar"]["statements"].is_array());
        assert!(!result["grammar"]["statements"]
            .as_array()
            .unwrap()
            .is_empty());
        let examples = result["examples"].as_array().expect("examples array");
        assert_eq!(examples.len(), 3);
        for example in examples {
            assert!(example["source"]
                .as_str()
                .unwrap_or_default()
                .contains("board \""));
        }
    }

    /// Guards against the embedded examples drifting out of sync with
    /// what the parser actually accepts.
    #[test]
    fn embedded_examples_parse_cleanly() {
        let result = execute_language_reference(&serde_json::json!({}));
        for example in result["examples"].as_array().unwrap() {
            let file = example["file"].as_str().unwrap();
            let source = example["source"].as_str().unwrap();
            let parse = synth_parser::parse(source, file.to_string());
            assert!(
                parse.ast.is_some(),
                "{file} failed to parse: {:?}",
                parse.diagnostics
            );
            let errors: Vec<_> = parse
                .diagnostics
                .iter()
                .filter(|d| {
                    matches!(
                        d.severity,
                        synth_diagnostics::Severity::Error | synth_diagnostics::Severity::Fatal
                    )
                })
                .collect();
            assert!(errors.is_empty(), "{file} has parse errors: {errors:?}");
        }
    }
}

#[cfg(test)]
mod tool_registration_tests {
    use super::*;

    /// Every tool `tools/list` advertises must actually be reachable
    /// from `call_tool` — otherwise an agent that discovers the tool
    /// via `tools/list` gets "Unknown tool" on the first call. This
    /// caught `synth_export_multiboard` being listed but never wired
    /// into the dispatch `match`.
    #[test]
    fn every_listed_tool_is_dispatched() {
        for tool in list_tools() {
            let result = call_tool(&tool.name, &serde_json::json!({}), None);
            if let Err(msg) = result {
                assert!(
                    !msg.starts_with("Unknown tool:"),
                    "`{}` is listed in tools/list but not wired into call_tool's dispatch",
                    tool.name
                );
            }
        }
    }
}
