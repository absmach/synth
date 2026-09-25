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
            description: "Run Physical Design Rule Checking (DRC) on a placed and routed PCB design. It first checks the sidecar-adjusted placement review and returns placement_requires_revision without rerouting an invalid layout; set allow_placement_warnings=true only for deliberate manual/debug validation. Validates trace clearances, widths, drill sizes, and courtyard overlaps against manufacturer profile.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional agent or human placement sidecar" },
                    "allow_placement_warnings": { "type": "boolean", "description": "Allow DRC routing despite visual-review findings; use only for deliberate manual/debug validation (default false)" },
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
            description: "Run deterministic PCB track routing on a SynthSpec design. It first checks the sidecar-adjusted placement visual review and returns placement_requires_revision without entering the expensive router when the layout is structurally poor. Set allow_placement_warnings=true only for intentional manual/debug routing. Agents may provide routing_order as an advisory list of net names to attempt first after inspecting congestion diagnostics; omitted nets retain the deterministic electrical-priority order. Returns routed segments, through-hole vias, unrouted net diagnostics, and total wire length stats.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code string" },
                    "file_path": { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional agent or human placement sidecar" },
                    "allow_placement_warnings": { "type": "boolean", "description": "Allow routing despite visual-review findings; use only for deliberate manual/debug routing (default false)" },
                    "routing_order": { "type": "array", "items": { "type": "string" }, "description": "Optional advisory net-name order, e.g. [\"net_14\", \"net_20\"]. Listed nets are attempted first; DRC and placement rules still apply." },
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
            description: "Export a validated SynthSpec design to KiCad schematic (.kicad_sch), PCB (.kicad_pcb), BOM CSV, or Gerber files. By default this is a release export and rejects incomplete routing or DRC violations. Set allow_incomplete=true only to create explicitly draft artifacts for review/manual routing; draft artifacts are never release-ready.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "SynthSpec source code" },
                    "file_path": { "type": "string", "description": "Path to source file" },
                    "out_dir": { "type": "string", "description": "Output directory path (accepts 'out' or 'out_dir')" },
                    "out": { "type": "string", "description": "Alias for out_dir" },
                    "layout_file_path": { "type": "string", "description": "Optional agent or human placement sidecar; exact component positions and rotations are applied to routing and export" },
                    "allow_incomplete": { "type": "boolean", "description": "Export a clearly labelled draft even when routing is incomplete or DRC has violations. Defaults to false; never use this output for fabrication." },
                    "routing_order": { "type": "array", "items": { "type": "string" }, "description": "Optional net order from synth_route routing_feedback; preserves the ordered recovery route during export." },
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
            description: "Run the PCB placer with explicit semantic placement hints (region, edge, near/side) for one or more components. Does not modify the source file. Returns placement positions, hint satisfaction, structural visual-review findings, functional-cluster warnings, compactness warnings, unrouted net count, and full DRC violation reports. Call with run_routing=false first; if placement_quality.visual_review.requires_revision is true, revise the hints or sidecar before routing. A combined run also stops before the router when review requires revision; use synth_route with allow_placement_warnings=true only for deliberate manual/debug routing. For production layouts, keep MCU/flash/decouplers and interface passives close, keep connectors edge-oriented, then use a sidecar for relative or exact refinements.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":           { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":        { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional path to .layout.toml sidecar overrides file" },
                    "profile":          { "type": "string", "description": "Optional manufacturer DRC profile ('jlcpcb_standard' or path to toml)" },
                    "board_width_mm":   { "type": "number", "description": "Optional explicit board width in millimetres; must be provided with board_height_mm" },
                    "board_height_mm":  { "type": "number", "description": "Optional explicit board height in millimetres; must be provided with board_width_mm" },
                    "allow_placement_warnings": { "type": "boolean", "description": "Allow combined routing despite visual-review findings; use only for deliberate manual/debug routing (default false)" },
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
            description: "Get a human-readable semantic summary of the current PCB placement — which functional clusters are in which board regions, density warnings, structural visual-review findings, and DRC status. Pass layout_file_path so the review evaluates the agent/human sidecar arrangement rather than only the automatic placement.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":        { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":     { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional placement sidecar; review the overridden arrangement when provided" },
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
            description: "Write or update a component placement override or forced net label in sidecar file (<design>.synth.layout.toml). Preserves existing overrides. Prefer relative_to with dx_mm/dy_mm for agent revisions so the arrangement remains robust when the board is resized; rerun synth_place_with_hints or synth_describe_placement afterward.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "layout_file_path": { "type": "string", "description": "Path to sidecar TOML file" },
                    "refdes":           { "type": "string", "description": "Component refdes e.g. 'U1'" },
                    "x_mm":             { "type": "number", "description": "Absolute X position in mm; omit when using relative_to" },
                    "y_mm":             { "type": "number", "description": "Absolute Y position in mm; omit when using relative_to" },
                    "relative_to":     { "type": "string", "description": "Optional anchor refdes; position becomes anchor center plus dx_mm/dy_mm" },
                    "dx_mm":            { "type": "number", "description": "Relative X offset from relative_to in mm" },
                    "dy_mm":            { "type": "number", "description": "Relative Y offset from relative_to in mm" },
                    "rotation":         { "type": "integer", "description": "Rotation in degrees (0, 90, 180, 270)" },
                    "sheet":            { "type": "string", "description": "Optional sheet the component was placed on; recorded so a later sheet move invalidates the stale override" },
                    "source":           { "type": "string", "enum": ["human_drag", "agent"], "description": "Provenance tag" },
                    "priority":         { "type": "string", "enum": ["soft", "hard"], "description": "Override priority" }
                },
                "required": ["layout_file_path", "refdes"]
            }),
        },
        McpToolInfo {
            name: "synth_route_with_constraints".into(),
            description: "Run the PCB autorouter with explicit per-net routing constraints (trace width, clearance, preferred layer, differential pair) and optional layout sidecar overrides. It applies the same placement visual gate as synth_route before starting the expensive router; set allow_placement_warnings=true only for intentional manual/debug routing. Evaluates DRC and returns segments, vias, unrouted nets, and DRC violation reports.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source":           { "type": "string", "description": "SynthSpec source code string" },
                    "file_path":        { "type": "string", "description": "Optional path to .synth source file on disk" },
                    "layout_file_path": { "type": "string", "description": "Optional path to .layout.toml sidecar overrides file" },
                    "allow_placement_warnings": { "type": "boolean", "description": "Allow routing despite visual-review findings; use only for deliberate manual/debug routing (default false)" },
                    "profile":          { "type": "string", "description": "Optional manufacturer DRC profile ('jlcpcb_standard' or path to toml)" },
                    "routing_order":    { "type": "array", "items": { "type": "string" }, "description": "Optional net order from routing feedback; preserved with the requested width/clearance profile." },
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
                    "query": { "type": "string", "description": "Keyword or part number to search, e.g. 'AMS1117' or 'C2040'" },
                    "registry_path": { "type": "string", "description": "Optional custom component registry path for offline fallback" },
                    "workspace_root": { "type": "string", "description": "Optional workspace root for offline registry fallback" }
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
// The body is a single static JSON grammar + example document; splitting
// it would scatter the documentation without simplifying anything.
#[allow(clippy::too_many_lines)]
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
                },
                {
                    "form": "group \"<title>\" [color \"#rrggbb\"] [title \"<display>\"] [region <region>] { <statement>* }",
                    "description": "Declares a sub-circuit region. THE most important statement for schematic readability: a group becomes a titled, coloured, dashed box on the sheet, its members are placed together inside it, and nets that stay inside it are drawn as wires while nets crossing out of it become labels. Components keep board-unique refdes and may be connected across groups. Prefer 3-6 groups naming the functional blocks (power input, MCU, sensor, ...). `color` overrides the deterministic palette hue; `title` sets a display title different from the identifier; `region` steers which page quadrant it packs toward.",
                    "example": "group \"3V3 LDO\" color \"#c2410c\" { component U2: regulator \"ap2112k_3v3\" value \"AP2112K-3.3\" }"
                },
                {
                    "form": "notes \"<title>\" { \"<line>\"* }",
                    "description": "Free prose rendered on the sheet. Inside a `group` the block is drawn at the bottom of that group's box; at board level it stacks below all content. Use it for the intent the netlist cannot carry: I2C addresses, strap choices, current budgets, assembly notes. Pass an empty title (\"\") for an untitled block.",
                    "example": "notes \"\" { \"I2C address 0x76 (SDO tied low).\" \"4.7k pull-ups sized for 400 kHz at 3.3 V.\" }"
                },
                {
                    "form": "power \"<RAIL>\" <value><unit> [class \"<netclass>\"] [{ <REFDES>.<pin>* }]",
                    "description": "Declares a named power rail with a nominal voltage. Rails become power symbols on the schematic instead of drawn wires, and the declared voltage feeds the voltage-domain ERC rules rather than being guessed from pin names. A bare declaration still materialises the net so later `connect ... as \"<RAIL>\"` lines join it.",
                    "example": "power \"+3V3\" 3.3v"
                },
                {
                    "form": "net \"<NAME>\" [class \"<netclass>\"] [{ <REFDES>.<pin>* }]",
                    "description": "Names a net explicitly. A named net renders its name as the schematic label; an unnamed net is auto-named (net_7) and `E-SYNTH-SCHEM-014` flags it if it reaches the sheet. Names should be UPPERCASE and <= 16 characters (`E-SYNTH-SCHEM-008/009`).",
                    "example": "net \"I2C_SCL\" { U1.scl U2.pb6 }"
                },
                {
                    "form": "netclass \"<name>\" { [trace_width <value><unit>] [clearance <value><unit>] [color \"#rrggbb\"] }",
                    "description": "Routing and colour class. Nets are auto-classified (Power, Ground, I2C, SPI, UART, USB, Clock, Reset) and coloured from a fixed palette; a declared class joined with `class \"<name>\"` overrides both. `color` sets the schematic wire/label hue and the PCB net colour.",
                    "example": "netclass \"PWR\" { trace_width 0.4mm clearance 0.25mm color \"#d55e00\" }"
                },
                {
                    "form": "connect <REFDES>.<pin> -> <REFDES>.<pin> as \"<NET_NAME>\"",
                    "description": "Same as plain `connect`, but joins the named net instead of an auto-named one. This is the usual way to put a connection on a declared `power` rail or a semantic signal name.",
                    "example": "connect U1.vout -> U2.vdd as \"+3V3\""
                },
                {
                    "form": "legends <on|off>",
                    "description": "Connector pin legends, default off. When on, each external connector gets a compact pin:net list beside it. Off is usually right: a one-line `notes` block reads better than a generated table, and no-connect crosses already mark unused pins.",
                    "example": "legends on"
                },
                {
                    "form": "company \"<name>\"",
                    "description": "Design-authority name for the schematic title block's Company field. Distinct from `manufacturer`, which names who builds the board.",
                    "example": "company \"Acme Robotics\""
                },
                {
                    "form": "sheet \"<name>\" { <statement>* }",
                    "description": "A hierarchical-sheet boundary. Statements lower exactly as in a `group` (refdes stay board-unique, cross-sheet `connect` is allowed), but the name is also a split point: a board whose single-sheet content overflows A2 and which has two or more populated boundaries exports as a KiCad hierarchy, one file per sheet. Small boards stay on one page.",
                    "example": "sheet \"Power\" { component U1: regulator \"ams1117_3v3\" }"
                },
                {
                    "form": "variant \"<name>\" [description \"<text>\"] { dnp <REFDES>* }",
                    "description": "A build variant sharing one schematic and layout but leaving the listed refdes unpopulated. Exports as KiCad 10 native variants plus one bom.<variant>.csv per variant. ERC still checks DNP parts.",
                    "example": "variant \"lite\" description \"No radio\" { dnp U3 U4 }"
                },
                {
                    "form": "module <Name> [( <param> = <default>, ... )] { port <Port>: <interface> ... <statement>* }",
                    "description": "A reusable parameterised sub-circuit definition. Instantiate with `use`. Ports are typed by a declared `interface`; bind them at the instantiation site with `bind`.",
                    "example": "module LedBank(count = 3) { port ctrl: Gpio component R1: resistor \"r_generic_0603\" value \"1k\" }"
                },
                {
                    "form": "interface <Name> { <signal>* }   |   bus <Name> { <member>* }   |   use <Module> as <Prefix> [( <param> = <value> )]   |   bind <Prefix>.<Port> -> <target>",
                    "description": "Module plumbing. `interface` names a signal bundle a port can carry, `bus` groups member nets under one name, `use` instantiates a module under a refdes prefix, and `bind` wires an instance port to a net or interface.",
                    "example": "use LedBank as LB1 ( count = 4 )"
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
        "workflow_tip": "1) synth_search_registry to find real (kind, part_id) pairs and exact pin names. 2) Draft the .synth source using the grammar above. Put every component inside a `group` and give each group a `notes` block - grouping is what makes the generated sheet readable, and an ungrouped board lays out as one flat band. Give every generic passive a `value` (a missing one is an error, E-SYNTH-VALUE-001). 3) synth_validate it: this returns electrical ERC *and* the readability rules (E-SYNTH-SCHEM-*). 4) synth_fix in a loop over diagnostics with suggested_fixes until clean. 5) synth_preview_schematic to inspect group_boxes/annotations before exporting. 6) synth_export."
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
                // Readability rules (E-SYNTH-SCHEM-*) alongside the
                // electrical ones, matching what `synth validate` does
                // on the CLI. Without these an agent iterating on
                // `synth_validate` gets no feedback on layout quality
                // until it exports, by which point the sheet is built.
                let global = synth_layout::layout(board);
                let sheets = synth_layout::sheets::layout_sheets(board, global);
                let mut schem = synth_kicad::check_schem_erc_sheets(board, &sheets);
                synth_kicad::attach_schem_erc_locations(&mut schem, board, file_name);
                diagnostics.extend(schem);
            }
        } else {
            let registry_dir = resolve_registry(args, default_registry);
            diagnostics.push(
                synth_diagnostics::DiagnosticBuilder::new(
                    "E-SYNTH-REGISTRY-001",
                    synth_diagnostics::Severity::Error,
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
            "violations": parse.diagnostics,
            "diagnostics": parse.diagnostics.clone()
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
    let mut diagnostics = resolved.diagnostics;
    diagnostics.extend(lowered.diagnostics);
    if diagnostics.iter().any(|d| d.severity.is_blocking()) {
        return Ok(serde_json::json!({
            "erc_clean": false,
            "error": "Compilation produced blocking diagnostics; ERC was not run",
            "violations": diagnostics,
            "diagnostics": diagnostics.clone()
        }));
    }
    let board = lowered.board.ok_or("Lowering failed")?;

    let erc_diagnostics = synth_validate::run_erc(&board, file_name);
    let erc_clean = !erc_diagnostics.iter().any(|d| d.severity.is_blocking());

    let error_count = erc_diagnostics
        .iter()
        .filter(|d| d.severity.is_blocking())
        .count();
    let warning_count = erc_diagnostics
        .iter()
        .filter(|d| d.severity == synth_diagnostics::Severity::Warning)
        .count();
    Ok(serde_json::json!({
        "erc_clean": erc_clean,
        "violations": erc_diagnostics,
        "diagnostics": erc_diagnostics.clone(),
        "error_count": error_count,
        "warning_count": warning_count
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
    if resolved
        .diagnostics
        .iter()
        .any(|d| d.severity.is_blocking())
    {
        return Err("Compilation failed during import resolution; DRC aborted".into());
    }
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    if lowered.diagnostics.iter().any(|d| d.severity.is_blocking()) {
        return Err("Compilation produced blocking diagnostics; DRC aborted".into());
    }
    let board = lowered.board.ok_or("Lowering failed")?;

    let sidecar_opt: Option<PathBuf> = args
        .get("layout_file_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| {
            let candidate = PathBuf::from(format!("{file_name}.layout.toml"));
            candidate.exists().then_some(candidate)
        });
    let placement = synth_place::place_with_sidecar(&board, sidecar_opt.as_deref())
        .map_err(|e| format!("Placement failed: {e:?}"))?;

    let placement_description = synth_place::describe_placement(&board, &placement);
    if placement_description.visual_review.requires_revision
        && !args["allow_placement_warnings"].as_bool().unwrap_or(false)
    {
        return Ok(serde_json::json!({
            "status": "placement_requires_revision",
            "routing_status": "not_run",
            "drc_status": "not_run",
            "reason": "Placement visual review requires revision before DRC",
            "placement_quality": {
                "visual_review": placement_description.visual_review,
                "functional_warnings": placement_description.functional_warnings,
                "dense_regions": placement_description.dense_regions
            },
            "hint": "Revise the placement sidecar or hints, rerun placement review, then retry DRC. Use allow_placement_warnings only for deliberate debug validation."
        }));
    }

    let routing = synth_route::route(&board, &placement);
    let report = synth_drc::check(&board, &placement, &routing, &profile);

    let route_complete = routing.unrouted_nets.is_empty();
    let drc_clean = report.is_clean() && route_complete;

    Ok(serde_json::json!({
        "status": if drc_clean { "ok" } else { "blocked" },
        "route_complete": route_complete,
        "unrouted_nets": routing.unrouted_nets,
        "drc_clean": drc_clean,
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

    let sidecar_opt: Option<PathBuf> = args
        .get("layout_file_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| {
            let candidate = PathBuf::from(format!("{file_name}.layout.toml"));
            candidate.exists().then_some(candidate)
        });
    let placement = synth_place::place_with_sidecar(&board, sidecar_opt.as_deref())
        .map_err(|e| format!("Placement failed: {e:?}"))?;

    let placement_description = synth_place::describe_placement(&board, &placement);
    if placement_description.visual_review.requires_revision
        && !args["allow_placement_warnings"].as_bool().unwrap_or(false)
    {
        return Ok(serde_json::json!({
            "status": "placement_requires_revision",
            "routing_status": "not_run",
            "reason": "Placement visual review requires revision before routing",
            "placement_quality": {
                "visual_review": placement_description.visual_review,
                "functional_warnings": placement_description.functional_warnings,
                "dense_regions": placement_description.dense_regions
            },
            "hint": "Revise the SynthSpec placement_hint or layout sidecar using relative_to/dx_mm/dy_mm, then rerun synth_place_with_hints or synth_describe_placement."
        }));
    }
    let routing_order: Vec<String> = args
        .get("routing_order")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let routing = if routing_order.is_empty() {
        synth_route::route(&board, &placement)
    } else {
        synth_route::route_with_order(&board, &placement, &routing_order)
    };

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
    let recommended_routing_order: Vec<String> = routing
        .unrouted_nets
        .iter()
        .take(8)
        .map(|net| net.net_name.clone())
        .collect();

    Ok(serde_json::json!({
        "status": if routing.unrouted_nets.is_empty() { "ok" } else { "unrouted_nets" },
        "segments_count": routing.segments.len(),
        "vias_count": routing.vias.len(),
        "unrouted_nets_count": routing.unrouted_nets.len(),
        "total_wire_length_mm": total_wire_length_mm,
        "routing_feedback": if recommended_routing_order.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!({
                "recommended_routing_order": recommended_routing_order,
                "reason": "Retry at most this bounded set first; preserve the electrical priority classes and inspect the result before expanding the order."
            })
        },
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
    // Long by design: one linear arg-parse + compile + export
    // pipeline per tool keeps the MCP surface reviewable in one
    // place (same rationale as `list_tools` below).
    #![allow(clippy::too_many_lines)]
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
    if resolved
        .diagnostics
        .iter()
        .any(|d| d.severity.is_blocking())
    {
        return Err("Compilation failed during import resolution; export aborted".into());
    }
    let registry =
        load_registry_tiered(args, default_registry).map_err(|e| format!("Registry error: {e}"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, file_name);
    if lowered.diagnostics.iter().any(|d| d.severity.is_blocking()) {
        return Err("Compilation produced blocking diagnostics; export aborted".into());
    }
    let board = lowered.board.ok_or("Lowering failed")?;

    // Recompute placement, routing and DRC from the exact board being exported.
    // A partial route may be exported only as an explicitly requested draft;
    // the default remains a release gate.
    let allow_incomplete = args["allow_incomplete"].as_bool().unwrap_or(false);
    let routing_order: Vec<String> = args
        .get("routing_order")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let sidecar_opt: Option<PathBuf> = args
        .get("layout_file_path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| {
            let candidate = PathBuf::from(format!("{file_name}.layout.toml"));
            candidate.exists().then_some(candidate)
        });
    let placement = synth_place::place_with_sidecar(&board, sidecar_opt.as_deref())
        .map_err(|e| format!("Placement failed: {e}"))?;
    let routing = if routing_order.is_empty() {
        synth_route::route(&board, &placement)
    } else {
        synth_route::route_with_order(&board, &placement, &routing_order)
    };
    let route_complete = routing.unrouted_nets.is_empty();
    if !route_complete && !allow_incomplete {
        return Err(format!(
            "Routing produced {} unrouted net(s); export aborted",
            routing.unrouted_nets.len()
        ));
    }
    let drc = synth_drc::check(
        &board,
        &placement,
        &routing,
        &synth_drc::ManufacturerProfile::jlc_standard(),
    );
    let drc_clean = drc.is_clean() && route_complete;
    if !drc.is_clean() && !allow_incomplete {
        return Err(format!(
            "DRC produced {} violation(s); export aborted",
            drc.violations.len()
        ));
    }

    let res = synth_kicad::export_with_sidecar_and_routing_order(
        &board,
        &out_dir,
        sidecar_opt.as_deref(),
        (!routing_order.is_empty()).then_some(routing_order.as_slice()),
    )
    .map_err(|e| format!("Export failed: {e}"))?;

    // Aesthetic schematic ERC over the same layout the exporter used:
    // surfaced to the agent so it can repair readability regressions
    // in the same closed loop as electrical ERC findings. Per-sheet
    // on §P26 split boards.
    let aesthetic = {
        let global = synth_layout::layout(&board);
        let sheets = synth_layout::sheets::layout_sheets(&board, global);
        let mut found = synth_kicad::check_schem_erc_sheets(&board, &sheets);
        // Whole diagnostics, not just `{code, title}`: a title alone
        // ("decoupling capacitor separation") does not say *which*
        // capacitor, so an agent cannot act on it. `attach_locations`
        // resolves a source span through the entity each rule names.
        synth_kicad::attach_schem_erc_locations(&mut found, &board, file_name);
        found
    };

    Ok(serde_json::json!({
        "status": if drc_clean { "success" } else { "draft_incomplete" },
        "release_ready": drc_clean,
        "route_complete": route_complete,
        "unrouted_nets": routing.unrouted_nets,
        "drc_clean": drc_clean,
        "warning": if drc_clean { serde_json::Value::Null } else { serde_json::json!("DRAFT ONLY: routing and/or DRC is incomplete. Review and manually complete the PCB before any fabrication use.") },
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
    // Long by design: see `execute_export`.
    #![allow(clippy::too_many_lines)]
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

    let requested_dimensions = match (
        args.get("board_width_mm").and_then(Value::as_f64),
        args.get("board_height_mm").and_then(Value::as_f64),
    ) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => {
            return Ok(serde_json::json!({
                "status": "error",
                "error": "board_width_mm and board_height_mm must be supplied together"
            }));
        }
    };

    let placement_result = requested_dimensions.map_or_else(
        || synth_place::place_with_hints(&board, &hints),
        |(width, height)| {
            synth_place::place_with_hints_and_dimensions(&board, &hints, width, height)
        },
    );

    match placement_result {
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
            let placement_description = synth_place::describe_placement(&board, &placement);

            // Placement is an independently useful stage. Routing and DRC
            // can be expensive and are exposed through their own gates; do
            // not make a placement request synchronously run the full router
            // unless the caller explicitly asks for the combined check.
            if !args["run_routing"].as_bool().unwrap_or(false) {
                #[allow(clippy::cast_precision_loss)]
                let board_w_mm = (placement.board_outline.width_nm() as f64) / 1_000_000.0;
                #[allow(clippy::cast_precision_loss)]
                let board_h_mm = (placement.board_outline.height_nm() as f64) / 1_000_000.0;
                return Ok(serde_json::json!({
                    "status": "placed",
                    "board_size_mm": [board_w_mm, board_h_mm],
                    "component_placements": placement.components,
                    "hint_satisfaction": report,
                    "placement_quality": {
                        "functional_warnings": placement_description.functional_warnings,
                        "dense_regions": placement_description.dense_regions,
                        "visual_review": placement_description.visual_review
                    },
                    "routing_status": "not_run",
                    "drc_status": "not_run",
                    "drc_clean": null,
                    "unrouted_nets": null
                }));
            }

            #[allow(clippy::cast_precision_loss)]
            let board_w_mm = (placement.board_outline.width_nm() as f64) / 1_000_000.0;
            #[allow(clippy::cast_precision_loss)]
            let board_h_mm = (placement.board_outline.height_nm() as f64) / 1_000_000.0;

            if placement_description.visual_review.requires_revision
                && !args["allow_placement_warnings"].as_bool().unwrap_or(false)
            {
                return Ok(serde_json::json!({
                    "status": "placement_requires_revision",
                    "board_size_mm": [board_w_mm, board_h_mm],
                    "component_placements": placement.components,
                    "hint_satisfaction": report,
                    "placement_quality": {
                        "functional_warnings": placement_description.functional_warnings,
                        "dense_regions": placement_description.dense_regions,
                        "visual_review": placement_description.visual_review
                    },
                    "routing_status": "not_run",
                    "drc_status": "not_run",
                    "reason": "Placement visual review requires revision before combined routing"
                }));
            }

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
                "placement_quality": {
                    "functional_warnings": placement_description.functional_warnings,
                    "dense_regions": placement_description.dense_regions,
                    "visual_review": placement_description.visual_review
                },
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

    let sidecar_path = args["layout_file_path"].as_str().map(Path::new);
    let placement = synth_place::place_with_sidecar(&board, sidecar_path)
        .map_err(|e| format!("Placement failed: {e}"))?;

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
    let relative_to = args["relative_to"].as_str().map(str::to_string);
    let x_mm = args["x_mm"].as_f64().unwrap_or(0.0);
    let y_mm = args["y_mm"].as_f64().unwrap_or(0.0);
    if relative_to.is_none() && (!args["x_mm"].is_number() || !args["y_mm"].is_number()) {
        return Err(
            "Provide x_mm/y_mm for an absolute override, or relative_to for a relative override"
                .into(),
        );
    }
    let rotation = args["rotation"].as_u64().unwrap_or(0) as u32;
    let sheet = args["sheet"].as_str().map(str::to_string);

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
        sheet,
        source,
        priority,
        timestamp: None,
        relative_to,
        dx: args["dx_mm"].as_f64().unwrap_or(0.0),
        dy: args["dy_mm"].as_f64().unwrap_or(0.0),
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
    // Long by design: see `execute_export`.
    #![allow(clippy::too_many_lines)]
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

    let placement_description = synth_place::describe_placement(&board, &placement);
    if placement_description.visual_review.requires_revision
        && !args["allow_placement_warnings"].as_bool().unwrap_or(false)
    {
        return Ok(serde_json::json!({
            "status": "placement_requires_revision",
            "routing_status": "not_run",
            "reason": "Placement visual review requires revision before constrained routing",
            "placement_quality": {
                "visual_review": placement_description.visual_review,
                "functional_warnings": placement_description.functional_warnings,
                "dense_regions": placement_description.dense_regions
            },
            "hint": "Revise the SynthSpec placement_hint or layout sidecar, rerun placement review, then retry constrained routing."
        }));
    }

    let routing_order: Vec<String> = args
        .get("routing_order")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let width_nm = min_width_nm.unwrap_or(127_000);
    let clearance_nm = min_clearance_nm.unwrap_or(127_000);
    let routing = if routing_order.is_empty() {
        match (min_width_nm, min_clearance_nm) {
            (Some(w), Some(c)) => synth_route::route_with_profile(&board, &placement, w, c),
            (Some(w), None) => synth_route::route_with_profile(&board, &placement, w, 127_000),
            (None, Some(c)) => synth_route::route_with_profile(&board, &placement, 127_000, c),
            (None, None) => synth_route::route(&board, &placement),
        }
    } else {
        synth_route::route_with_profile_and_order(
            &board,
            &placement,
            width_nm,
            clearance_nm,
            &routing_order,
        )
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
    let (resolved_lib_id, pins) = resolve_kicad_symbol(lib_id).ok_or_else(|| {
        format!("could not read physical pins for '{lib_id}'; is KiCad installed and on the symbol path?")
    })?;

    let default_id = lib_id
        .split_once(':')
        .map_or(lib_id, |(_, s)| s)
        .to_lowercase()
        .replace([' ', '-', '.', '/'], "_");
    let part_id = args["id"].as_str().map_or(default_id, str::to_string);
    let kind = infer_kind(&resolved_lib_id);
    let footprint = args["footprint"].as_str().map(str::to_string);

    let mut out = String::new();
    let _ = writeln!(out, "id = \"{part_id}\"");
    let _ = writeln!(out, "kind = \"{kind}\"");
    out.push_str("description = \"Imported from KiCad stock symbol\"\n");
    let _ = writeln!(out, "kicad_symbol = \"{resolved_lib_id}\"");
    if let Some(footprint) = &footprint {
        let _ = writeln!(out, "kicad_footprint = \"{footprint}\"");
    }
    out.push_str("\n[provenance]\n");
    out.push_str("source = \"imported\"\n");
    out.push_str("reviewed_by = \"\"\n");
    out.push_str("\n[[pins]]\n");
    let names = unique_imported_pin_names(&pins);
    for (p, name) in pins.iter().zip(&names) {
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "number = \"{}\"", p.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{}\"",
            imported_electrical_type(&p.name, &p.electrical_type)
        );
        let capabilities = inferred_imported_pin_capabilities(&p.name);
        if !capabilities.is_empty() {
            let _ = writeln!(
                out,
                "capabilities = [{}]",
                capabilities
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        out.push_str("\n[[pins]]\n");
    }
    if !pins.is_empty() {
        out.truncate(out.len() - "\n[[pins]]\n".len());
    }

    let part_path = user_dir.join(format!("{part_id}.synth.toml"));
    std::fs::write(&part_path, out).map_err(|e| e.to_string())?;

    let pins_json: Vec<serde_json::Value> = pins
        .iter()
        .zip(&names)
        .map(|(p, name)| {
            serde_json::json!({
                "name": name,
                "original_name": p.name,
                "number": p.number,
                "electrical_type": imported_electrical_type(&p.name, &p.electrical_type),
                "capabilities": inferred_imported_pin_capabilities(&p.name)
            })
        })
        .collect();

    Ok(serde_json::json!({
        "status": "imported",
        "source": "kicad",
        "part_id": part_id,
        "part_path": part_path.display().to_string(),
        "kind": kind,
        "kicad_symbol": resolved_lib_id,
        "pin_count": pins.len(),
        "footprint_supplied": footprint.is_some(),
        "warning": footprint.is_none().then_some(
            "No footprint was supplied; verify one before fabrication"
        ),
        "pins": pins_json
    }))
}

/// KiCad's stock connector library uses zero-padded unit names, while agents
/// commonly request the human spelling (`Conn_01x3`). Resolve that harmless
/// spelling variation before declaring an import unavailable.
fn resolve_kicad_symbol(
    lib_id: &str,
) -> Option<(String, Vec<synth_layout::kicad_lib_loader::PhysicalPin>)> {
    let mut candidates = vec![lib_id.to_string()];
    if let Some((library, symbol)) = lib_id.split_once(':') {
        let mut aliases = Vec::new();
        if let Some(x_pos) = symbol.rfind('x') {
            let (prefix, count) = symbol.split_at(x_pos + 1);
            if count.len() == 1 && count.chars().all(|c| c.is_ascii_digit()) {
                aliases.push(format!("{library}:{prefix}0{count}"));
            }
        }
        candidates.extend(aliases);
    }
    candidates.into_iter().find_map(|candidate| {
        synth_layout::kicad_lib_loader::physical_pins(&candidate).map(|pins| (candidate, pins))
    })
}

fn canonical_imported_pin_name(name: &str, number: &str) -> String {
    let mut result: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while result.starts_with('_') {
        result.remove(0);
    }
    while result.ends_with('_') {
        result.pop();
    }
    if result.is_empty() {
        result = format!("pin_{number}");
    }
    if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        result = format!("pin_{result}");
    }
    result
}

fn unique_imported_pin_names(pins: &[synth_layout::kicad_lib_loader::PhysicalPin]) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    pins.iter()
        .map(|pin| {
            let base = canonical_imported_pin_name(&pin.name, &pin.number);
            let mut candidate = base.clone();
            if !used.insert(candidate.clone()) {
                let suffix = canonical_imported_pin_name("", &pin.number);
                candidate = format!("{base}_{suffix}");
                let mut index = 2;
                while !used.insert(candidate.clone()) {
                    candidate = format!("{base}_{suffix}_{index}");
                    index += 1;
                }
            }
            candidate
        })
        .collect()
}

/// Infer only unambiguous Synth protocol capabilities from standard KiCad
/// signal names. KiCad symbols carry electrical direction but commonly omit
/// the semantic tags consumed by Synth ERC.
fn inferred_imported_pin_capabilities(name: &str) -> Vec<&'static str> {
    let upper = name.to_ascii_uppercase();
    let mut caps = Vec::new();
    if upper.contains("USB_DP") || upper == "D+" {
        caps.push("usb_dp");
    }
    if upper.contains("USB_DM") || upper == "D-" {
        caps.push("usb_dn");
    }
    if upper.contains("QSPI_SS") || upper.ends_with("_CS") || upper == "CS" {
        caps.push("spi_cs");
    }
    if upper.contains("QSPI_SCLK") || upper.ends_with("_SCK") || upper == "SCK" {
        caps.push("spi_sck");
    }
    if upper.contains("QSPI_SD0") || upper.ends_with("_MOSI") || upper == "MOSI" {
        caps.push("spi_mosi");
    }
    if upper.contains("QSPI_SD1") || upper.ends_with("_MISO") || upper == "MISO" {
        caps.push("spi_miso");
    }
    if upper.starts_with("GPIO") {
        caps.push("gpio");
    }
    if upper == "RUN" || upper.contains("RESET") {
        caps.push("reset");
    }
    caps
}

fn imported_electrical_type(name: &str, raw: &str) -> String {
    let upper = name.to_ascii_uppercase();
    if upper.contains("USB_DP") {
        "differential_positive".into()
    } else if upper.contains("USB_DM") {
        "differential_negative".into()
    } else {
        map_kicad_electrical_type(raw)
    }
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

    let zip_pins: Vec<synth_layout::kicad_lib_loader::PhysicalPin> = import
        .pins
        .iter()
        .map(|pin| synth_layout::kicad_lib_loader::PhysicalPin {
            number: pin.number.clone(),
            name: pin.name.clone(),
            electrical_type: pin.electrical_type.clone(),
            x: pin.x,
            y: pin.y,
        })
        .collect();
    let names = unique_imported_pin_names(&zip_pins);
    for (pin, name) in import.pins.iter().zip(&names) {
        let _ = writeln!(out, "[[pins]]");
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "number = \"{}\"", pin.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{}\"",
            imported_electrical_type(&pin.name, &pin.electrical_type)
        );
        let capabilities = inferred_imported_pin_capabilities(&pin.name);
        if !capabilities.is_empty() {
            let _ = writeln!(
                out,
                "capabilities = [{}]",
                capabilities
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
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
        .zip(&names)
        .map(|(p, name)| {
            serde_json::json!({
                "name": name,
                "original_name": p.name,
                "number": p.number,
                "electrical_type": imported_electrical_type(&p.name, &p.electrical_type),
                "capabilities": inferred_imported_pin_capabilities(&p.name)
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
    let mut part: synth_registry::Part =
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
    if let Some(provenance) = &part.provenance {
        if provenance.source == synth_registry::ProvenanceSource::Seed {
            return Err("authored parts cannot claim seed provenance".into());
        }
    } else {
        part.provenance = Some(synth_registry::Provenance {
            source: synth_registry::ProvenanceSource::Authored,
            generator: Some("synth_author_part".into()),
            ..Default::default()
        });
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

    let network: Result<Option<synth_supply::types::SupplyStatus>, String> =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<Option<synth_supply::types::SupplyStatus>, String> {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                let dist = synth_supply::lcsc::LcscDistributor::new();
                rt.block_on(dist.query(query)).map_err(|e| e.to_string())
            },
        ))
        .map_err(|_| "LCSC search panicked; using local registry fallback".to_string())
        .and_then(std::convert::identity);

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
    fn validation_rejects_unavailable_registry() {
        let source = r#"board "registry_failure" {
  layers 2
  component U1: mcu "definitely_missing_mcu"
}
"#;
        let result = call_tool(
            "synth_validate",
            &json!({
                "source": source,
                "registry_path": "/path/that/does/not/exist"
            }),
            None,
        )
        .expect("validation should return structured diagnostics");
        assert_eq!(result["status"], "error");
        assert_eq!(result["error_count"], 1);
        assert_eq!(result["diagnostics"][0]["code"], "E-SYNTH-REGISTRY-001");
    }

    #[test]
    fn erc_does_not_report_clean_for_unresolved_parts() {
        let source = r#"board "unresolved_part" {
  layers 2
  component U1: mcu "definitely_missing_mcu"
}
"#;
        let result = call_tool("synth_erc_report", &json!({ "source": source }), None)
            .expect("ERC should return a structured failure");
        assert_eq!(result["erc_clean"], false);
        assert!(result["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "E-SYNTH-COMP-001"));
    }

    #[test]
    fn export_rejects_unresolved_parts_before_writing() {
        let dir = scratch("export_unresolved");
        let source = r#"board "unresolved_export" {
  layers 2
  component U1: mcu "definitely_missing_mcu"
}
"#;
        let result = call_tool(
            "synth_export",
            &json!({
                "source": source,
                "out_dir": dir.to_str().unwrap()
            }),
            None,
        );
        assert!(result.is_err(), "export must reject unresolved parts");
        assert!(!dir.join("unresolved_export.kicad_pcb").exists());
        let _ = std::fs::remove_dir_all(&dir);
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
        let written = std::fs::read_to_string(dir.join("test_res_1k.synth.toml")).unwrap();
        assert!(written.contains("source = \"authored\""));
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
