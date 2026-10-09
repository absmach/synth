// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::too_many_lines)]
#![allow(clippy::too_many_arguments)]

//! `synth` — command-line entry point.
//!
//! Exit codes are part of the agent-facing contract (PRD Chapter 43):
//!
//! - `0` — success
//! - `1` — validation errors (parse, semantic, ERC, ...)
//! - `2` — usage error (bad arguments, file not found)
//! - `3` — internal compiler error (a bug — should never happen)

mod release;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use synth_diagnostics::NativeCheckStatus;

const EXIT_SUCCESS: u8 = 0;
const EXIT_VALIDATION_ERRORS: u8 = 1;
const EXIT_USAGE: u8 = 2;

#[derive(Debug, Parser)]
#[command(
    name = "synth",
    version,
    about = "Synth — agent-native EDA compiler",
    long_about = None,
)]
struct Cli {
    /// Worker threads for the parallel compiler stages (the
    /// placer-router repair search). Defaults to the number of
    /// available cores; `RAYON_NUM_THREADS` is honoured when this is
    /// not given. Thread count never changes the generated artifacts,
    /// only how long they take.
    #[arg(long, global = true, value_name = "N")]
    jobs: Option<usize>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse, resolve, and lower a SynthSpec file to the IR. Emits
    /// diagnostics from every stage; exits 0 if clean, 1 if any
    /// error-level diagnostic was emitted.
    Validate {
        /// Path to a `.synth` source file.
        input: PathBuf,

        /// Output format for diagnostics.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,

        /// Path to the component registry. Auto-discovered: `--registry`, else `$SYNTH_REGISTRY`, else `./registry/parts` (or an ancestor directory), else the embedded seed registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Path to the Tier-2 (per-user) registry directory.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,

        /// Promote user-part shadowing of shipped parts to a load error.
        #[arg(long)]
        strict_registry: bool,

        /// Skip resolution and lowering; run parsing only.
        #[arg(long)]
        parse_only: bool,
    },

    /// Parse a SynthSpec file and dump the AST as JSON to stdout.
    /// Diagnostics, if any, are written to stderr.
    DumpAst {
        input: PathBuf,
        #[arg(long)]
        pretty: bool,
    },

    /// Parse + lower a SynthSpec file and dump the canonical IR
    /// (`Board`) as JSON to stdout. Diagnostics go to stderr.
    DumpIr {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
    },

    /// Export a SynthSpec file to a KiCad 8 project directory.
    /// Writes `<name>.kicad_pro`, `<name>.kicad_sch`, `<name>.kicad_sym`,
    /// and `bom.csv` into `--out`. Optional `--gerbers` / `--drill` /
    /// `--step` flags shell out to `kicad-cli pcb export` to generate
    /// manufacturing artifacts alongside the project (Phase 9 slice 3).
    ExportKicad {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        /// Output directory for the KiCad project. Created if absent.
        #[arg(long = "out", alias = "out-dir", value_name = "DIR")]
        out: PathBuf,
        /// Also emit RS-274X Gerbers via `kicad-cli pcb export gerbers`.
        /// Writes into `<out>/gerbers/`.
        #[arg(long)]
        gerbers: bool,
        /// Also emit Excellon drill files (PTH + NPTH split, with PDF
        /// drill map) via `kicad-cli pcb export drill`. Writes into
        /// `<out>/drill/`.
        #[arg(long)]
        drill: bool,
        /// Also emit a 3D STEP model via `kicad-cli pcb export step`.
        /// Writes `<out>/<name>.step`.
        #[arg(long)]
        step: bool,
        /// Also emit Pick-and-Place (PnP) position file (pnp.csv).
        #[arg(long)]
        pnp: bool,
        /// Target manufacturer profile name (e.g. jlc, pcbway, oshpark).
        #[arg(long)]
        profile: Option<String>,
        /// Run KiCad schematic ERC (`kicad-cli sch erc`) on the exported schematic.
        #[arg(long)]
        validate_erc: bool,
        #[arg(long, value_name = "FILE")]
        verification_report: Option<PathBuf>,
        /// Force export even if Synth ERC validation produces error diagnostics.
        #[arg(long)]
        force: bool,
        /// Release mode: refuse to accept any override flag. Suitable for CI
        /// and release gates, where a package that needed an override is not
        /// a package anyone should be able to produce by accident.
        #[arg(long, conflicts_with_all = ["force", "allow_unverified_parts"])]
        safe: bool,
        /// Allow manufacturing-artifact export (--gerbers/--drill/--step)
        /// to proceed even when the board uses a part flagged
        /// `W-SYNTH-PART-UNVERIFIED` (Phase 15, R15.3/§18.8.2). Without
        /// this flag, a fab submission refuses to include unreviewed
        /// parts; the schematic/PCB/BOM files still export normally.
        #[arg(long)]
        allow_unverified_parts: bool,
        /// Path to the Tier-2 (per-user) registry directory.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,
        /// Promote user-part shadowing of shipped parts to a load error.
        #[arg(long)]
        strict_registry: bool,
        /// Deprecated. Every export now routes through the selected
        /// external router; there is no Synth-native route to enable.
        #[arg(long, hide = true)]
        autoroute: bool,
        /// External router that generates the copper. FreeRouting is the
        /// managed default; `kicad-routing-tools` requires an installed
        /// checkout. A missing engine is a capability error, never a
        /// silent fallback.
        #[arg(long, value_enum, default_value_t = ExternalRouter::FreeRouting)]
        router: ExternalRouter,
        /// FreeRouting JAR. Defaults to tools/freerouting/freerouting-2.4.1.jar.
        #[arg(long, value_name = "JAR")]
        freerouting_jar: Option<PathBuf>,
        /// Java executable used for FreeRouting. Defaults to the bundled
        /// tools/jre25/bin/java when present, otherwise `java`.
        #[arg(long, value_name = "JAVA")]
        freerouting_java: Option<PathBuf>,
        /// KiCadRoutingTools checkout, required with `--router kicad-routing-tools`.
        #[arg(long, value_name = "DIR")]
        kicad_routing_tools_repo: Option<PathBuf>,
        /// Python interpreter containing KiCadRoutingTools dependencies.
        #[arg(long, value_name = "PYTHON")]
        kicad_routing_tools_python: Option<PathBuf>,
        /// KiCadRoutingTools rule-relaxation policy. `board` preserves the
        /// board's declared minimums; `fab` may use the selected fab floor.
        #[arg(long, value_enum, default_value_t = KrtEscalation::Board)]
        krt_escalation: KrtEscalation,
        /// KiCadRoutingTools fabrication capability floor.
        #[arg(long, value_enum, default_value_t = KrtFabTier::Auto)]
        krt_fab_tier: KrtFabTier,
        /// Optional KRT fab-floor override file (`key = value` lines).
        #[arg(long, value_name = "FILE")]
        krt_fab_overrides: Option<PathBuf>,
        /// Minimum same-net pad clearance for KRT vias, in millimetres.
        /// The default keeps vias out of SMD pads and paste openings.
        #[arg(long, default_value_t = 0.1, value_name = "MM")]
        krt_same_net_pad_clearance: f64,
        /// Permit via-in-pad in the KRT production gate.
        #[arg(long)]
        krt_allow_via_in_pad: bool,
        /// Try every installed engine and keep the best attempt, judged by
        /// the independent checks rather than by either engine's own report.
        #[arg(long)]
        best_of: bool,
        /// Export a clearly labelled draft even when routing is incomplete,
        /// the router is unavailable, or the board is not fabrication-ready.
        /// The un-routed `<name>.synth.kicad_pcb` is preserved for review or
        /// for an external router to complete. Never fabricate this output.
        #[arg(long)]
        allow_incomplete: bool,
    },

    /// Apply the highest-confidence `suggested_fix` from every
    /// diagnostic emitted by `validate` to the source file, in
    /// reverse byte order so earlier patches don't shift later
    /// offsets. Writes the result back to the input file unless
    /// `--dry-run` is set, in which case the proposed new source
    /// is written to stdout.
    Fix {
        /// Path to a `.synth` source file.
        input: PathBuf,

        /// Path to the component registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Enable SMT quantitative constraint solving during patch application.
        #[arg(long)]
        smt: bool,

        /// Print the patched source to stdout instead of writing
        /// it back to the input file.
        #[arg(long)]
        dry_run: bool,
    },

    /// Emit the JSON Schema (draft-07) for the diagnostic protocol
    /// to stdout. Use this to validate tooling that consumes
    /// `synth validate --format json`.
    Schema {
        /// Schema to emit. Only `diagnostic` is supported in v1.0.
        #[arg(value_enum, default_value_t = SchemaKind::Diagnostic)]
        kind: SchemaKind,
    },

    /// Parse + lower a SynthSpec file, run schematic auto-layout,
    /// and dump the `Layout` (component placements, wires, power
    /// flags, net labels) as JSON to stdout. Useful for debugging
    /// the layouter and for tooling that wants the placement
    /// without going through the KiCad export.
    Layout {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
        /// Also compute deterministic layout quality metrics
        /// (§7.8.7) and include them as a `score` field in the
        /// output JSON.
        #[arg(long)]
        score: bool,
    },

    /// Parse + lower + run PCB placement (Phase 7). Dumps the
    /// `Placement` IR (board outline + per-component nanometer
    /// coordinates + layer + rotation) as JSON to stdout.
    /// Slice 1A emits a deterministic grid; later slices run the
    /// constraint solver and refinement.
    Place {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        /// Explicit board width in millimetres. Must be supplied with height.
        #[arg(long)]
        width: Option<f64>,
        /// Explicit board height in millimetres. Must be supplied with width.
        #[arg(long)]
        height: Option<f64>,
        /// Compiler-owned fixed-form-factor board family. Cannot be combined
        /// with explicit width/height.
        #[arg(long)]
        board_family: Option<String>,
        #[arg(long)]
        pretty: bool,
    },

    /// Report which external routers are installed and usable on this
    /// machine, with the reason each is or is not.
    ///
    /// Discovery is read-only and cheap: it locates the FreeRouting JAR and
    /// Java runtime, checks a KiCadRoutingTools checkout for its entry
    /// point, and records the version each would run at. Run this first when
    /// a routing run reports the engine as unavailable — the output names
    /// the exact path that was searched.
    Routers {
        #[arg(long)]
        pretty: bool,
    },

    /// Route a design with an external router and dump the routing run
    /// record — router identity and version, settings, input hash, retained
    /// artifacts, and the terminal state — as JSON to stdout.
    ///
    /// Synth does not generate copper. The run record is the answer, and
    /// exit code 0 means the board was independently validated rather than
    /// that a router exited successfully. See `synth routers` for what is
    /// installed.
    Route {
        /// Path to a `.synth` source file.
        input: PathBuf,

        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// External router to use. FreeRouting is the managed default;
        /// `kicad-routing-tools` requires an installed checkout.
        #[arg(long, value_enum, default_value_t = ExternalRouter::FreeRouting)]
        router: ExternalRouter,
        /// FreeRouting JAR. Defaults to tools/freerouting/freerouting-2.4.1.jar.
        #[arg(long, value_name = "JAR")]
        freerouting_jar: Option<PathBuf>,
        /// Java executable used for FreeRouting. Defaults to the bundled
        /// tools/jre25/bin/java when present, otherwise `java`.
        #[arg(long, value_name = "JAVA")]
        freerouting_java: Option<PathBuf>,
        /// KiCadRoutingTools checkout, required with
        /// `--router kicad-routing-tools`.
        #[arg(long, value_name = "DIR")]
        kicad_routing_tools_repo: Option<PathBuf>,
        /// Python interpreter containing KiCadRoutingTools dependencies.
        #[arg(long, value_name = "PYTHON")]
        kicad_routing_tools_python: Option<PathBuf>,
        /// KiCadRoutingTools rule-relaxation policy. `board` preserves the
        /// board's declared minimums; `fab` may use the selected fab floor.
        #[arg(long, value_enum, default_value_t = KrtEscalation::Board)]
        krt_escalation: KrtEscalation,
        /// KiCadRoutingTools fabrication capability floor.
        #[arg(long, value_enum, default_value_t = KrtFabTier::Auto)]
        krt_fab_tier: KrtFabTier,
        /// Optional KRT fab-floor override file (`key = value` lines).
        #[arg(long, value_name = "FILE")]
        krt_fab_overrides: Option<PathBuf>,
        /// Minimum same-net pad clearance for KRT vias, in millimetres.
        #[arg(long, default_value_t = 0.1, value_name = "MM")]
        krt_same_net_pad_clearance: f64,
        /// Permit via-in-pad in the KRT production gate.
        #[arg(long)]
        krt_allow_via_in_pad: bool,
        /// Wall-clock budget for the whole external run, in seconds.
        #[arg(long, value_name = "SECS")]
        router_timeout: Option<u64>,
        /// Try every installed engine and keep the best attempt, judged by
        /// the independent checks rather than by either engine's own report.
        #[arg(long)]
        best_of: bool,

        #[arg(long)]
        pretty: bool,
    },

    /// Route a design externally and report the independent physical checks:
    /// topology against the baseline the router was given, connectivity
    /// re-derived from copper, and `kicad-cli pcb drc` with zones refilled.
    ///
    /// Dumps the routing run record as JSON to stdout, and exits with the
    /// validation error code unless the board is fabrication-ready.
    Drc {
        /// Path to a `.synth` source file.
        input: PathBuf,

        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// External router to use. FreeRouting is the managed default;
        /// `kicad-routing-tools` requires an installed checkout.
        #[arg(long, value_enum, default_value_t = ExternalRouter::FreeRouting)]
        router: ExternalRouter,
        /// FreeRouting JAR. Defaults to tools/freerouting/freerouting-2.4.1.jar.
        #[arg(long, value_name = "JAR")]
        freerouting_jar: Option<PathBuf>,
        /// Java executable used for FreeRouting. Defaults to the bundled
        /// tools/jre25/bin/java when present, otherwise `java`.
        #[arg(long, value_name = "JAVA")]
        freerouting_java: Option<PathBuf>,
        /// KiCadRoutingTools checkout, required with
        /// `--router kicad-routing-tools`.
        #[arg(long, value_name = "DIR")]
        kicad_routing_tools_repo: Option<PathBuf>,
        /// Python interpreter containing KiCadRoutingTools dependencies.
        #[arg(long, value_name = "PYTHON")]
        kicad_routing_tools_python: Option<PathBuf>,
        /// KiCadRoutingTools rule-relaxation policy. `board` preserves the
        /// board's declared minimums; `fab` may use the selected fab floor.
        #[arg(long, value_enum, default_value_t = KrtEscalation::Board)]
        krt_escalation: KrtEscalation,
        /// KiCadRoutingTools fabrication capability floor.
        #[arg(long, value_enum, default_value_t = KrtFabTier::Auto)]
        krt_fab_tier: KrtFabTier,
        /// Optional KRT fab-floor override file (`key = value` lines).
        #[arg(long, value_name = "FILE")]
        krt_fab_overrides: Option<PathBuf>,
        /// Minimum same-net pad clearance for KRT vias, in millimetres.
        #[arg(long, default_value_t = 0.1, value_name = "MM")]
        krt_same_net_pad_clearance: f64,
        /// Permit via-in-pad in the KRT production gate.
        #[arg(long)]
        krt_allow_via_in_pad: bool,
        /// Wall-clock budget for the whole external run, in seconds.
        #[arg(long, value_name = "SECS")]
        router_timeout: Option<u64>,

        #[arg(long)]
        pretty: bool,
    },

    /// Run the deterministic, network-free release/check contract. The
    /// command composes compiler validation, DRC, and (with --fab) a complete
    /// KiCad manufacturing export into one stable JSON result.
    Check {
        /// Path to a `.synth` source file.
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        /// Run the manufacturing export gate as well as source/DRC checks.
        #[arg(long)]
        fab: bool,
        /// Allow manufacturing output containing unverified registry parts.
        #[arg(long)]
        allow_unverified_parts: bool,
        /// Accept a package that carries export overrides, recording why.
        /// Requires --authorized-by. The package stays non-production; this
        /// authorizes the run, it does not clean the package.
        #[arg(long, value_name = "REASON", requires = "authorized_by")]
        override_exception: Option<String>,
        /// Who authorized --override-exception.
        #[arg(long, value_name = "NAME", requires = "override_exception")]
        authorized_by: Option<String>,
        /// Emit machine-readable output. Without this flag a concise summary
        /// is printed while the JSON shape remains available to CI via
        /// `--json`.
        #[arg(long)]
        json: bool,
    },

    /// Start the Synth Model Context Protocol (MCP) Server for native AI tool integration.
    Mcp {
        /// Run over stdio (stdin/stdout). Default mode for AI extensions.
        #[arg(long)]
        stdio: bool,

        /// Run over HTTP SSE stream on 127.0.0.1.
        #[arg(long)]
        sse: bool,

        /// Port for HTTP SSE mode.
        #[arg(long, default_value_t = 8081)]
        port: u16,

        /// Path to component registry directory. Auto-discovered (see `synth registry path`).
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
    },

    /// Query real-time distributor stock and pricing for a design's BOM (Phase 9.3).
    SupplyChain {
        /// Path to a `.synth` source file.
        input: PathBuf,

        /// Path to the component registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Output format for supply status (human table or json).
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
    },

    /// Compile, place, route, and render 3D PNG (+ optional 2D SVG) of the PCB via `kicad-cli`.
    Render {
        /// Path to a `.synth` source file.
        input: PathBuf,

        /// Output image file path (.png).
        #[arg(long = "out", value_name = "FILE", default_value = "board.png")]
        out: PathBuf,

        /// Render quality (basic or high).
        #[arg(long, value_enum, default_value_t = RenderQuality::Basic)]
        quality: RenderQuality,

        /// Board side to render (top, bottom).
        #[arg(long, default_value = "top")]
        side: String,

        /// Image width in pixels.
        #[arg(long, default_value_t = 2400)]
        width: u32,

        /// Image height in pixels.
        #[arg(long, default_value_t = 1600)]
        height: u32,

        /// Path to the component registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Export 2D top-copper SVG alongside 3D render.
        #[arg(long)]
        svg: bool,
    },

    /// Emit the compiler-owned capability descriptor used by agents and
    /// cloud/UI integrations. This is read-only and never inspects or
    /// mutates a design.
    Capability {
        #[command(subcommand)]
        cmd: CapabilityCommand,
    },

    /// Inspect the component registry (Phase 15, R15.2): resolve tier
    /// directories, list merged parts, and run a health check.
    Registry {
        #[command(subcommand)]
        cmd: RegistryCommand,

        /// Path to the Tier-1 (shipped) registry. Auto-discovered: `--registry`, else `$SYNTH_REGISTRY`, else `./registry/parts` (or an ancestor directory), else the embedded seed registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Path to the Tier-2 (per-user) registry directory.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,

        /// Promote shadowing (user part overrides shipped) to a load error.
        #[arg(long)]
        strict: bool,
    },

    /// Import component parts into the Tier-2 (per-user) registry (Phase 15, R15.4).
    Part {
        #[command(subcommand)]
        cmd: PartCommand,

        /// Path to the Tier-1 registry `parts` directory. Defaults to the
        /// checkout's `registry/parts`, then the embedded seed registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Path to the Tier-2 (per-user) registry directory.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,
    },
}

/// Subcommands of `synth capability`.
#[derive(Debug, Subcommand)]
enum CapabilityCommand {
    /// List the actual CLI/compiler surface and supported physical stages.
    List {
        /// Emit a stable JSON descriptor instead of the human table.
        #[arg(long)]
        json: bool,
    },
}

/// Subcommands of `synth registry` (Phase 15, R15.2).
#[derive(Debug, Subcommand)]
enum RegistryCommand {
    /// Print the resolved Tier-1 and Tier-2 registry directories.
    Path,
    /// Materialize the embedded seed registry into the XDG shipped
    /// directory (`$XDG_DATA_HOME/synth/registry/shipped/parts`) so
    /// it is inspectable and every project resolves it without a
    /// checkout. Shipped files are overwritten in place; your own
    /// parts belong in the Tier-2 overlay (`--user-registry` /
    /// `SYNTH_USER_REGISTRY_DIR`).
    Install {
        /// Install into DIR instead of the XDG shipped directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// List every merged part id, annotated with its tier.
    List,
    /// Health check: report shadowed parts and unverified parts.
    Doctor,
    /// Qualify every part against its KiCad footprint and symbol:
    /// pin-to-pad coverage both ways, mirrored footprints, package
    /// dimensions, pin classification, and provenance. A check that
    /// cannot run is reported as `unknown`, never as a pass.
    Qualify {
        /// Write the review artifact as JSON to FILE instead of a
        /// human summary on stdout.
        #[arg(long, value_name = "FILE")]
        report: Option<PathBuf>,
        /// Emit the review artifact as JSON on stdout.
        #[arg(long, conflicts_with = "report")]
        json: bool,
        /// Qualify only this part id.
        #[arg(long, value_name = "ID")]
        part: Option<String>,
        /// Exit 0 even when parts are blocked or unproven.
        #[arg(long)]
        no_fail: bool,
    },
    /// Generate or verify a SHA256 manifest of the Tier-1 registry
    /// (R15.10). The manifest is the deterministic artifact the release
    /// pipeline signs: `synth registry manifest --output manifest.sha256`
    /// then sign that file; CI verifies with `--verify manifest.sha256`.
    Manifest {
        /// Write the manifest to FILE instead of stdout.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Verify the tree against a previously generated manifest FILE
        /// instead of generating one.
        #[arg(long, value_name = "FILE", conflicts_with = "output")]
        verify: Option<PathBuf>,
    },
}

/// Subcommands of `synth part` (Phase 15, R15.4).
#[derive(Debug, Subcommand)]
enum PartCommand {
    /// Import a part from LCSC/EasyEDA by its C-prefix product code.
    ///
    /// This is also the correct command for parts found on the
    /// JLCPCB Parts Library (<https://jlcpcb.com/parts>): JLCPCB is
    /// LCSC's sister company and its SMT assembly catalog uses the
    /// exact same C-number ("JLCPCB Part #, formerly also LCSC Part
    /// #" per their own docs) — there's no separate JLCPCB API or
    /// import path, the same `C2040`-style code works here directly.
    ImportLcsc {
        /// LCSC product code, e.g. `C2040`. Also the code shown on a
        /// JLCPCB Parts Library listing (jlcpcb.com/parts) — same
        /// numbering, same part.
        code: String,

        /// Read the EasyEDA CAD JSON from a local file instead of fetching live.
        #[arg(long, value_name = "FILE")]
        from_file: Option<PathBuf>,

        /// Directory to write generated `.kicad_mod` footprints into.
        /// Defaults to `<user_registry>/../footprints`.
        #[arg(long, value_name = "DIR")]
        footprint_dir: Option<PathBuf>,
    },

    /// Import a part from an installed KiCad stock symbol library.
    ///
    /// Reads the physical pin inventory (number, name, electrical type) from
    /// the bundled KiCad symbol via `kicad_lib_loader`, then writes a
    /// `<id>.synth.toml` skeleton into the Tier-2 registry with the pins
    /// pre-filled. Electrical types are carried over from KiCad; capabilities
    /// are left empty for a human/agent to complete from the datasheet.
    ImportKicad {
        /// KiCad `lib_id`, e.g. `Device:R`, `Regulator_Linear:AMS1117-3.3`,
        /// or `MCU_Microchip_SAMD:ATSAMD21E18A`.
        #[arg(default_value = "Device:R")]
        lib_id: String,

        /// Override the generated `PartId` (filename stem). Defaults to the
        /// symbol name (after `:`) lowercased and sanitized.
        #[arg(long, value_name = "ID")]
        id: Option<String>,

        /// Optional `kicad_footprint` reference (e.g. `Resistor_SMD:R_0603`)
        /// to record alongside the symbol.
        #[arg(long, value_name = "LIB_ID")]
        footprint: Option<String>,

        /// Bulk mode (R15.12 seed growth): import every `lib_id` listed in
        /// FILE (one per line; `#` comments and blank lines ignored).
        /// Each result lands unverified in the Tier-2 registry for the
        /// human review queue — see `registry/CREDITS.md`.
        #[arg(long, value_name = "FILE", conflicts_with = "lib_id")]
        batch: Option<PathBuf>,

        /// With `--batch`: stop after N successful imports.
        #[arg(long, value_name = "N", requires = "batch")]
        limit: Option<usize>,
    },

    /// Import a part from a SnapEDA or UltraLibrarian "Export to
    /// KiCad" zip file the user downloaded through their own browser.
    ///
    /// Neither site offers a public API, and both Terms of Service
    /// prohibit automated/scripted access to the site itself — Synth
    /// never contacts snapeda.com or ultralibrarian.com. This reads
    /// only the zip already on disk: it finds the first `.kicad_sym`
    /// and `.kicad_mod` entries, extracts the pin inventory the same
    /// way `import-kicad` does for a bundled stock symbol, copies the
    /// footprint into the Tier-2 registry, and writes a `.synth.toml`
    /// skeleton. Works for any single-part KiCad-format export zip,
    /// not just these two vendors (e.g. KiCad's own "Save symbol as
    /// new library" + "Export footprint" also produce compatible
    /// files).
    ImportZip {
        /// Path to the downloaded `.zip` file.
        zip_path: PathBuf,

        /// Override the generated `PartId` (filename stem). Defaults
        /// to the `.kicad_sym`'s symbol name, lowercased and
        /// sanitized.
        #[arg(long, value_name = "ID")]
        id: Option<String>,
    },

    /// Find a real `.kicad_mod` for parts whose `kicad_footprint` names one
    /// that is not installed.
    ///
    /// A wrong footprint name is the common failure, and the right footprint
    /// is usually already in the local KiCad libraries — so this searches
    /// them by name, reports what it finds, and with `--apply` writes the
    /// correction into the Tier-2 user registry. `synth export-kicad` runs
    /// the same resolver automatically, so this command is for inspecting
    /// and forcing the repair ahead of an export.
    ///
    /// A match is only applied when it is both confident and clearly ahead
    /// of the runner-up; an ambiguous result is reported, never guessed.
    /// Pin numbers are never touched — see `synth validate`'s
    /// `E-SYNTH-PIN-001` for why that has to stay a human decision.
    ResolveFootprint {
        /// Write the matched footprint into the Tier-2 user registry
        /// instead of only reporting it.
        #[arg(long)]
        apply: bool,

        /// Limit to a single part id. Default: every part in the registry.
        #[arg(value_name = "PART_ID")]
        part_id: Option<String>,
    },

    /// Write a `<id>.synth.toml` skeleton into the Tier-2 registry
    /// (R15.8 authoring flow, step 1). All pins are emitted with
    /// `required = false` and `electrical_type = "bidirectional"` so the
    /// stub never blocks lowering; fill in the real pinout from the
    /// datasheet, then set `reviewed_by` to clear
    /// `W-SYNTH-PART-UNVERIFIED`.
    Stub {
        /// Part id for the new entry (filename stem).
        id: String,

        /// Pin count for the skeleton (pins are numbered `1..=N`).
        #[arg(long, value_name = "N", default_value_t = 8)]
        pins: usize,

        /// Tier-2 registry directory. Defaults to
        /// `SYNTH_USER_REGISTRY_DIR` / XDG user registry.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,
    },
}

#[derive(Debug, Copy, Clone, ValueEnum)]
enum Format {
    Human,
    Json,
}

/// The external router an operator selected.
///
/// FreeRouting is the default because it is the managed path; the other is
/// an external checkout and therefore always an explicit choice. There is
/// no third variant, and no built-in router: a missing engine is a
/// capability error, never a silent fallback to something else.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ExternalRouter {
    FreeRouting,
    KicadRoutingTools,
}

impl From<ExternalRouter> for synth_router::RouterEngine {
    fn from(value: ExternalRouter) -> Self {
        match value {
            ExternalRouter::FreeRouting => Self::Freerouting,
            ExternalRouter::KicadRoutingTools => Self::KiCadRoutingTools,
        }
    }
}

impl ExternalRouter {
    fn as_str(self) -> &'static str {
        synth_router::RouterEngine::from(self).as_str()
    }
}

impl From<KrtEscalation> for synth_router::KrtEscalation {
    fn from(value: KrtEscalation) -> Self {
        // One conversion point so the CLI's spelling and the engine's
        // spelling cannot drift into two different meanings.
        match value.as_str() {
            "off" => Self::Off,
            "fab" => Self::Fab,
            _ => Self::Board,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum KrtEscalation {
    Off,
    Board,
    Fab,
}

impl KrtEscalation {
    /// The value forwarded to the engine.
    ///
    /// Kept alongside the adapter's own `KrtEscalation` rather than
    /// replaced by it so the CLI surface stays a plain clap enum, and the
    /// two are converted explicitly in one place.
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Board => "board",
            Self::Fab => "fab",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum KrtFabTier {
    Standard,
    Advanced,
    Auto,
}

impl KrtFabTier {
    fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Advanced => "advanced",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Copy, Clone, ValueEnum)]
enum RenderQuality {
    Basic,
    High,
}

impl RenderQuality {
    fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Copy, Clone, ValueEnum)]
enum SchemaKind {
    Diagnostic,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let jobs = match cli.jobs {
        Some(0) => {
            // A zero-thread pool is a usage error, not a request for the
            // default: silently running on every core would hide the typo.
            eprintln!("error: --jobs must be at least 1");
            return ExitCode::from(EXIT_USAGE);
        }
        Some(n) => {
            if let Err(err) = rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build_global()
            {
                eprintln!("error: could not configure {n} worker threads: {err}");
                return ExitCode::from(EXIT_USAGE);
            }
            n
        }
        // Default: Rayon's own default, i.e. the available parallelism.
        None => std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get),
    };
    // Size the repair waves identically to the pool so every core gets a
    // distinct candidate instead of queueing (or idling).
    synth_kicad::set_worker_threads(jobs);
    eprintln!("synth: using {jobs} worker threads");
    let result = match cli.command {
        Command::Validate {
            input,
            format,
            registry,
            user_registry,
            strict_registry,
            parse_only,
        } => validate(
            &input,
            format,
            registry.as_deref(),
            user_registry.as_deref(),
            strict_registry,
            parse_only,
        ),
        Command::DumpAst { input, pretty } => dump_ast(&input, pretty),
        Command::DumpIr {
            input,
            registry,
            pretty,
        } => dump_ir(&input, registry.as_deref(), pretty),
        Command::ExportKicad {
            input,
            registry,
            out,
            gerbers,
            drill,
            step,
            pnp: _,
            profile: _,
            validate_erc,
            verification_report,
            force,
            safe,
            allow_unverified_parts,
            user_registry,
            strict_registry,
            autoroute,
            router,
            freerouting_jar,
            freerouting_java,
            kicad_routing_tools_repo,
            kicad_routing_tools_python,
            krt_escalation,
            krt_fab_tier,
            krt_fab_overrides,
            krt_same_net_pad_clearance,
            krt_allow_via_in_pad,
            best_of,
            allow_incomplete,
        } => export_kicad(
            &input,
            registry.as_deref(),
            user_registry.as_deref(),
            strict_registry,
            &out,
            synth_kicad::FabRequest {
                gerbers,
                drill,
                step,
            },
            validate_erc,
            verification_report.as_deref(),
            force,
            safe,
            allow_unverified_parts,
            autoroute,
            router,
            freerouting_jar.as_deref(),
            freerouting_java.as_deref(),
            kicad_routing_tools_repo.as_deref(),
            kicad_routing_tools_python.as_deref(),
            krt_escalation,
            krt_fab_tier,
            krt_fab_overrides.as_deref(),
            krt_same_net_pad_clearance,
            krt_allow_via_in_pad,
            best_of,
            allow_incomplete,
        ),
        Command::Fix {
            input,
            registry,
            smt,
            dry_run,
        } => fix(&input, registry.as_deref(), smt, dry_run),
        Command::Schema { kind } => schema(kind),
        Command::Layout {
            input,
            registry,
            pretty,
            score,
        } => dump_layout(&input, registry.as_deref(), pretty, score),
        Command::Place {
            input,
            registry,
            width,
            height,
            board_family,
            pretty,
        } => dump_place(
            &input,
            registry.as_deref(),
            width,
            height,
            board_family.as_deref(),
            pretty,
        ),
        Command::Route {
            input,
            registry,
            router,
            freerouting_jar,
            freerouting_java,
            kicad_routing_tools_repo,
            kicad_routing_tools_python,
            krt_escalation,
            krt_fab_tier,
            krt_fab_overrides,
            krt_same_net_pad_clearance,
            krt_allow_via_in_pad,
            router_timeout,
            best_of,
            pretty,
        } => dump_route(
            &input,
            registry.as_deref(),
            router,
            &router_options(
                router,
                freerouting_jar.as_deref(),
                freerouting_java.as_deref(),
                kicad_routing_tools_repo.as_deref(),
                kicad_routing_tools_python.as_deref(),
                krt_escalation,
                krt_fab_tier,
                krt_fab_overrides.as_deref(),
                krt_same_net_pad_clearance,
                krt_allow_via_in_pad,
            )
            .with_timeout(router_timeout),
            best_of,
            pretty,
        ),
        Command::Drc {
            input,
            registry,
            router,
            freerouting_jar,
            freerouting_java,
            kicad_routing_tools_repo,
            kicad_routing_tools_python,
            krt_escalation,
            krt_fab_tier,
            krt_fab_overrides,
            krt_same_net_pad_clearance,
            krt_allow_via_in_pad,
            router_timeout,
            pretty,
        } => dump_drc(
            &input,
            registry.as_deref(),
            router,
            &router_options(
                router,
                freerouting_jar.as_deref(),
                freerouting_java.as_deref(),
                kicad_routing_tools_repo.as_deref(),
                kicad_routing_tools_python.as_deref(),
                krt_escalation,
                krt_fab_tier,
                krt_fab_overrides.as_deref(),
                krt_same_net_pad_clearance,
                krt_allow_via_in_pad,
            )
            .with_timeout(router_timeout),
            pretty,
        ),
        Command::Routers { pretty } => dump_routers(pretty),
        Command::Check {
            input,
            registry,
            fab,
            allow_unverified_parts,
            override_exception,
            authorized_by,
            json,
        } => match (authorized_by.as_deref(), override_exception.as_deref()) {
            (Some(who), Some(why)) => match release::Exception::new(who, why) {
                Ok(exception) => check(
                    &input,
                    registry.as_deref(),
                    fab,
                    allow_unverified_parts,
                    Some(&exception),
                    json,
                ),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            },
            _ => check(
                &input,
                registry.as_deref(),
                fab,
                allow_unverified_parts,
                None,
                json,
            ),
        },
        Command::Mcp {
            stdio,
            sse,
            port,
            registry,
        } => run_mcp(stdio, sse, port, registry),
        Command::SupplyChain {
            input,
            registry,
            format,
        } => supply_chain(&input, registry.as_deref(), format),
        Command::Render {
            input,
            out,
            quality,
            side,
            width,
            height,
            registry,
            svg,
        } => render(
            &input,
            &out,
            quality,
            &side,
            width,
            height,
            registry.as_deref(),
            svg,
        ),
        Command::Capability { cmd } => capability_cmd(&cmd),
        Command::Registry {
            cmd,
            registry,
            user_registry,
            strict,
        } => registry_cmd(cmd, registry.as_deref(), user_registry.as_deref(), strict),
        Command::Part {
            cmd,
            registry,
            user_registry,
        } => part_cmd(cmd, registry.as_deref(), user_registry.as_deref()),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("synth: {e}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

const CAPABILITY_COMMANDS: [&str; 17] = [
    "validate",
    "dump-ast",
    "dump-ir",
    "export-kicad",
    "fix",
    "schema",
    "layout",
    "place",
    "route",
    "drc",
    "check",
    "mcp",
    "supply-chain",
    "render",
    "capability",
    "registry",
    "part",
];

type CapabilityLimit = (&'static str, Option<&'static str>, &'static str);

const UNSUPPORTED: &[CapabilityLimit] = &[
    (
        "coupled_diff_pair_routing",
        Some("diff_pair.couple"),
        "`couple tight|loose` is parsed and recorded in the IR, but Synth does not keep the two \
         halves of a differential pair coupled or check that they are; that is left to the router.",
    ),
    (
        "length_skew_enforcement",
        Some("diff_pair.max_skew"),
        "`max_skew` can be declared and is recorded in the IR, but nothing enforces it and no \
         stage fails on residual skew; any tuning is left to the router.",
    ),
    (
        "return_path_and_layer_transition_analysis",
        None,
        "Return paths, reference-plane continuity and signal layer transitions are not analysed.",
    ),
    (
        "inner_layer_impedance",
        None,
        "Only outer-layer microstrip is derived from the stackup, so stripline impedance is not \
         computed for any design net; synth_evaluate_thermal_si calculates it only from \
         caller-supplied dimensions.",
    ),
    (
        "split_ground_planes",
        None,
        "Each ground net gets its own unsplit full-board zone; split or segmented ground planes \
         are not designed.",
    ),
    (
        "thermal_relief_analysis",
        None,
        "Zone thermal-relief gap and spoke width are fixed export defaults, not analysed.",
    ),
    (
        "power_sequencing_and_power_good",
        None,
        "Enable ordering, power-good signals and sequencing timing are neither declared nor checked.",
    ),
];

const UNVERIFIED: &[CapabilityLimit] = &[
    (
        "diff_pair_impedance",
        Some("diff_pair.impedance"),
        "Used only on a declared stackup: an outer-layer microstrip width (and, for a true \
         differential pair, the gap between its legs) is derived and declared geometry is \
         checked, as an estimate. Inner-layer pairs are not verified.",
    ),
    (
        "rf_net_trace_width",
        None,
        "The native router still routes RF-named nets at a fixed 0.33 mm chosen by net name, and \
         ignores derived or declared widths; the width in the exported net class reaches only a \
         router that reads net classes.",
    ),
];

fn capability_limits(limits: &[CapabilityLimit]) -> Vec<serde_json::Value> {
    limits
        .iter()
        .map(|(id, constraint, summary)| {
            let mut entry = serde_json::json!({"id": id, "summary": summary});
            if let Some(constraint) = constraint {
                entry["constraint"] = serde_json::json!(constraint);
            }
            entry
        })
        .collect()
}

fn capability_descriptor() -> serde_json::Value {
    let board_families = synth_place::board_family::PROFILES
        .iter()
        .map(|profile| {
            serde_json::json!({
                "name": profile.name,
                "description": profile.description,
                "layers": profile.layers,
                "width_mm": profile.width_mm,
                "height_mm": profile.height_mm,
                "edge_clearance_mm": profile.edge_clearance_mm
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "schema_version": "1.1",
        "compiler": {
            "name": "synth",
            "version": env!("CARGO_PKG_VERSION")
        },
        "commands": CAPABILITY_COMMANDS.iter().map(|name| serde_json::json!({
            "name": name,
            "available": true
        })).collect::<Vec<_>>(),
        "physical_stages": [
            {"name": "layout", "evidence": ["layout_ir"]},
            {"name": "placement", "evidence": ["placement_ir", "placement_hash"]},
            {"name": "routing", "evidence": ["routing_ir", "routing_hash"]},
            {"name": "drc", "evidence": ["drc_report", "drc_status"]},
            {"name": "kicad_export", "evidence": ["artifact_hashes", "archive_sha256"]}
        ],
        "manufacturer_profiles": [
            {"name": "jlc-standard", "layers": [2, 4], "min_trace_width_mm": 0.127, "min_clearance_mm": 0.127, "min_drill_mm": 0.3},
            {"name": "pcbway-standard", "layers": [2, 4], "min_trace_width_mm": 0.15, "min_clearance_mm": 0.15, "min_drill_mm": 0.3},
            {"name": "oshpark-4layer", "layers": [4], "min_trace_width_mm": 0.125, "min_clearance_mm": 0.125, "min_drill_mm": 0.25}
        ],
        "language": {
            "syntax_version": "1.0",
            "statements": ["board", "import", "layers", "manufacturer", "revision", "schematic", "component", "connect", "net", "power", "module", "interface", "bus", "use", "bind", "netclass", "keepout", "stackup", "group", "sheet", "variant"],
            "attributes": ["company", "legends", "notes", "dnp", "prefix", "param", "as", "class", "diff_pair", "impedance", "trace_width", "clearance", "radius", "copper", "insulator", "er", "material", "value", "tolerance", "voltage", "power_rating", "dielectric", "description"],
            "endpoint_forms": ["component.pin", "named_net", "bus.member"]
        },
        "geometry_and_constraints": {
            "board_dimensions": {
                "source": "placement_cli_options",
                "width": {"option": "--width", "unit": "mm", "paired_with": "height"},
                "height": {"option": "--height", "unit": "mm", "paired_with": "width"}
            },
            "layer_count": {"statement": "layers", "type": "integer", "min": 1, "max": 64},
            "schematic_page": {
                "statement": "schematic",
                "body": "schematic { paper = \"A4\" }",
                "setting": "paper",
                "accepted": ["A5", "A4", "A3", "A2", "A1", "A0"],
                "default": "A4",
                "type": "string",
                "notes": "The requested page is used as-is; it is enlarged only when the content does not fit. A5 is selectable but never auto-fitted."
            },
            "schematic_overflow": {
                "statement": "schematic",
                "body": "schematic { paper = \"A4\" overflow = \"grow\" }",
                "setting": "overflow",
                "accepted": ["grow", "hierarchy"],
                "default": "grow",
                "type": "string",
                "aliases": {"hierarchy": ["hierarchical", "sheets"]},
                "notes": "grow climbs A4->A3->A2->A1->A0 then splits into a sheet hierarchy; hierarchy caps growth at the requested page and splits as soon as the content no longer fits."
            },
            "stackup": {
                "statement": "stackup",
                "body": "stackup { copper 0.035mm insulator 1.5mm er 4.4 material \"FR4\" copper 0.035mm }",
                "entries": ["copper", "insulator"],
                "copper": {"required": ["thickness"]},
                "insulator": {"required": ["er"], "optional": ["material"]},
                "units": ["mm", "mil"],
                "order": "top to bottom; copper first, alternating, copper last",
                "export": "written to the .kicad_pcb setup/stackup section; board thickness becomes the stack total",
                "default": "none: without a block the board exports the exporter's default thickness and no stackup section",
                "notes": "No fabricator presets are shipped; every value is declared by the design. A diff_pair impedance derives its net class trace width (and, for a true pair, its gap) from the first copper and insulator layers."
            },
            "routing_constraints": {
                "netclass": ["trace_width", "clearance", "color"],
                "diff_pair": ["impedance", "max_skew", "couple"],
                "keepout": ["radius"],
                "component": ["placement_hint"]
            },
            "units": ["mm", "mil", "ohm", "kohm", "mohm", "v", "mv", "a", "ma", "mhz", "ghz", "pf", "nf", "uf"]
        },
        "board_family_profiles": {
            "available": true,
            "source": "synth-place::board_family",
            "families": board_families
        },
        "unsupported": capability_limits(UNSUPPORTED),
        "unverified": capability_limits(UNVERIFIED)
    })
}

fn capability_cmd(cmd: &CapabilityCommand) -> anyhow::Result<u8> {
    match cmd {
        CapabilityCommand::List { json: true } => {
            serde_json::to_writer_pretty(std::io::stdout(), &capability_descriptor())?;
            println!();
        }
        CapabilityCommand::List { json: false } => {
            println!("COMMAND              AVAILABLE");
            println!("-------------------- ---------");
            for command in CAPABILITY_COMMANDS {
                println!("{command:<20} yes");
            }
            println!();
            println!("PHYSICAL STAGES: layout, placement, routing, drc, kicad_export");
            println!(
                "BOARD FAMILY PROFILES: {} shipped",
                synth_place::board_family::PROFILES.len()
            );
            for (heading, limits) in [("UNSUPPORTED", UNSUPPORTED), ("UNVERIFIED", UNVERIFIED)] {
                println!();
                println!("{heading}:");
                for (id, _, summary) in limits {
                    println!("  {id}: {summary}");
                }
            }
        }
    }

    Ok(EXIT_SUCCESS)
}

fn run_mcp(stdio: bool, sse: bool, port: u16, registry: Option<PathBuf>) -> anyhow::Result<u8> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("could not start tokio runtime: {e}"))?;

    if sse || !stdio {
        runtime.block_on(synth_mcp::run_sse_server(port, registry))?;
    } else {
        runtime.block_on(synth_mcp::run_stdio_server(registry))?;
    }
    Ok(EXIT_SUCCESS)
}

/// The proposal-and-release boundary used by the enterprise workflow. This deliberately
/// delegates each stage to the same CLI implementation used by agents and
/// humans, but captures their machine-readable results into one deterministic
/// report. No model, network, or project mutation is involved.
fn check(
    input: &Path,
    registry: Option<&Path>,
    fab: bool,
    allow_unverified_parts: bool,
    exception: Option<&release::Exception>,
    json: bool,
) -> anyhow::Result<u8> {
    if !input.is_file() {
        anyhow::bail!("input file {} does not exist", input.display());
    }
    let source = std::fs::read(input)?;
    let source_hash = sha256_hex(&source);
    let exe = std::env::current_exe()?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let work = std::env::temp_dir().join(format!("synth_check_{}_{}", std::process::id(), nonce));
    std::fs::create_dir_all(&work)?;

    let base = vec!["--jobs".to_string(), "1".to_string()];

    let mut validate_args = base.clone();
    validate_args.extend([
        "validate".into(),
        "--format".into(),
        "json".into(),
        input.display().to_string(),
    ]);
    if let Some(dir) = registry {
        validate_args.extend(["--registry".into(), dir.display().to_string()]);
    }
    let (validate_output, validate_timed_out) = run_check_child(&exe, &validate_args, 120)?;
    let validate_json = serde_json::from_slice::<serde_json::Value>(&validate_output.stdout)
        .unwrap_or_else(|_| serde_json::json!({"diagnostics": [], "parse_error": true}));
    let validate_pass = !validate_timed_out && validate_output.status.success();

    let (drc_json, drc_outcome, drc_timed_out) = if validate_pass {
        let mut drc_args = base.clone();
        drc_args.extend(["drc".into(), "--pretty".into(), input.display().to_string()]);
        if let Some(dir) = registry {
            drc_args.extend(["--registry".into(), dir.display().to_string()]);
        }
        let (drc_output, timed_out) = run_check_child(&exe, &drc_args, 30)?;
        (
            serde_json::from_slice::<serde_json::Value>(&drc_output.stdout)
                .unwrap_or_else(|_| serde_json::json!({"status": "unknown", "parse_error": true})),
            if timed_out {
                PreFabDrc::Unknown
            } else {
                pre_fab_drc(&drc_output)
            },
            timed_out,
        )
    } else {
        (
            serde_json::json!({"status": "unknown", "reason": "source gate failed"}),
            PreFabDrc::Unknown,
            false,
        )
    };

    let mut stages = serde_json::Map::new();
    stages.insert(
        "source".into(),
        serde_json::json!({
            "status": if validate_pass { "pass" } else { "fail" },
            "result": validate_json,
        }),
    );
    stages.insert(
        "drc".into(),
        serde_json::json!({
            "status": if !validate_pass || drc_timed_out {
                "unknown"
            } else {
                drc_outcome.stage_status()
            },
            "result": drc_json,
        }),
    );

    let drc_pass = drc_outcome.is_clean();
    let mut fab_pass = true;
    // Only a failed source gate skips the export.
    //
    // A DRC outcome of any kind does not: the export is the only thing that
    // produces the `kicad_drc` evidence, so skipping it on a violation or an
    // unrunnable check would replace a specific, counted diagnosis with
    // "upstream source or DRC gate failed" — or, worse, report an unknown
    // tool as a clean upstream stage. The export's own gate is fail-closed,
    // so running it on a dirty board cannot release anything; it writes into
    // this command's scratch directory either way.
    if fab && validate_pass {
        let export_dir = work.join("release");
        let verification_path = work.join("verification.json");
        let mut export_args = base.clone();
        export_args.extend([
            "export-kicad".into(),
            input.display().to_string(),
            "--out".into(),
            export_dir.display().to_string(),
            "--gerbers".into(),
            "--drill".into(),
            "--step".into(),
            "--pnp".into(),
            "--validate-erc".into(),
            "--verification-report".into(),
            verification_path.display().to_string(),
        ]);
        if let Some(dir) = registry {
            export_args.extend(["--registry".into(), dir.display().to_string()]);
        }
        if allow_unverified_parts {
            export_args.push("--allow-unverified-parts".into());
        }
        let (export_output, export_timed_out) = run_check_child(&exe, &export_args, 120)?;
        fab_pass = !export_timed_out && export_output.status.success();
        let native = std::fs::read_to_string(&verification_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

        // The release gate's own question, distinct from both "did the export
        // succeed" and "was it natively verified": does this package carry
        // overrides nobody authorized?
        let release = release::ReleaseManifest::read_from(&export_dir)
            .ok()
            .map(|m| m.with_exception(exception.cloned()));
        let verdict = release.as_ref().map(release::ReleaseManifest::gate_verdict);
        if matches!(verdict, Some(release::GateVerdict::Rejected)) {
            fab_pass = false;
        }
        let mut artifacts = serde_json::Map::new();
        if export_dir.is_dir() {
            let mut files = Vec::new();
            let mut pending = vec![export_dir.clone()];
            while let Some(dir) = pending.pop() {
                for entry in std::fs::read_dir(dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        pending.push(path);
                    } else {
                        let bytes = std::fs::read(&path)?;
                        let rel = path
                            .strip_prefix(&export_dir)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        files.push((rel, sha256_hex(&bytes)));
                    }
                }
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (path, hash) in files {
                artifacts.insert(path, serde_json::Value::String(hash));
            }
        }
        let status = manufacturing_status(fab_pass, export_timed_out, native.as_ref());
        fab_pass = status == "pass";
        let mut stage = serde_json::json!({
            "status": status,
            "artifacts": artifacts,
        });
        if export_timed_out {
            stage["reason"] = serde_json::json!("timeout");
            stage["detail"] = serde_json::json!("`export-kicad` exceeded its 120s budget");
        }
        if let Some(native) = native {
            stage["native"] = native;
        }
        if let Some(release) = &release {
            stage["release"] = serde_json::to_value(release)?;
            let verified = stage["native"]["release_ready"].as_bool().unwrap_or(false);
            stage["release_ready"] = serde_json::json!(release.release_ready && verified);
            // A timeout is the more fundamental reason, so it keeps the field.
            if matches!(verdict, Some(release::GateVerdict::Rejected)) && !export_timed_out {
                stage["reason"] = serde_json::json!("unauthorized_overrides");
            }
            if let Some(release::GateVerdict::AcceptedUnderException { authorized_by }) =
                verdict.as_ref()
            {
                stage["exception_authorized_by"] = serde_json::json!(authorized_by);
            }
        }
        stages.insert("manufacturing".into(), stage);
    } else if fab {
        fab_pass = false;
        stages.insert(
            "manufacturing".into(),
            serde_json::json!({"status": "unknown", "reason": "upstream source or DRC gate failed"}),
        );
    }

    let pass = validate_pass && drc_pass && (!fab || fab_pass);
    let report = serde_json::json!({
        "schema_version": "synth.check.v1",
        "status": if pass { "pass" } else { "fail" },
        "input": input.display().to_string(),
        "source_sha256": source_hash,
        "network": "not_used",
        "model": "not_used",
        "stages": stages,
    });
    let _ = std::fs::remove_dir_all(&work);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{} {}", if pass { "PASS" } else { "FAIL" }, input.display());
        let manufacturing = &report["stages"]["manufacturing"];
        for (name, stage) in report["stages"].as_object().into_iter().flatten() {
            println!(
                "  {name}: {}",
                stage["status"].as_str().unwrap_or("unknown")
            );
            if name == "manufacturing" {
                for native in stage["native"]["stages"].as_array().into_iter().flatten() {
                    if let (true, Some(detail)) =
                        (native["status"] == "fail", native["detail"].as_str())
                    {
                        println!("    {}: {detail}", native["stage"].as_str().unwrap_or("?"));
                    }
                }
            }
        }
        println!(
            "  source_sha256: {}",
            report["source_sha256"].as_str().unwrap_or("")
        );
        if let Some(release) = manufacturing["release"].as_object() {
            println!(
                "  release_ready: {}",
                manufacturing["release_ready"].as_bool().unwrap_or(false)
            );
            if let Some(overrides) = release.get("overrides").and_then(|o| o.as_array()) {
                if !overrides.is_empty() {
                    println!("  {}", release::UNTRUSTED_BANNER);
                    for record in overrides {
                        println!(
                            "    {} via {}",
                            record["code"].as_str().unwrap_or("?"),
                            record["flag"].as_str().unwrap_or("?")
                        );
                    }
                }
            }
        }
    }
    Ok(if pass {
        EXIT_SUCCESS
    } else {
        EXIT_VALIDATION_ERRORS
    })
}

/// What the pre-fab `synth drc` run established, before the export runs.
///
/// Three states, not two. `synth drc` exits non-zero for any board that is
/// not fabricable, which includes one whose router could not run, so its
/// exit code cannot answer "did DRC find a violation?" on its own.
///
/// Only [`PreFabDrc::Unknown`] stops the export, and that is a deliberate
/// asymmetry with [`PreFabDrc::Violations`]. A violation does not stop it:
/// the export's own gate refuses a dirty board anyway, and that run is what
/// produces the `kicad_drc` evidence naming the violation and its count, so
/// skipping it would replace a specific diagnosis with "upstream gate
/// failed". An unrunnable check is the opposite case, because then only the
/// export's native stages can say *why* it was unrunnable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreFabDrc {
    /// DRC ran and found nothing blocking.
    Clean,
    /// DRC ran and found errors or unconnected items.
    Violations,
    /// DRC could not be performed, or the record is unreadable.
    Unknown,
}

impl PreFabDrc {
    fn is_clean(self) -> bool {
        matches!(self, Self::Clean)
    }

    fn stage_status(self) -> &'static str {
        match self {
            Self::Clean => "pass",
            Self::Violations => "fail",
            Self::Unknown => "unknown",
        }
    }
}

/// Classify a `synth drc` run record.
fn pre_fab_drc(output: &std::process::Output) -> PreFabDrc {
    let Ok(report) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        // No readable record: unresolved, not clean.
        return PreFabDrc::Unknown;
    };
    let counts = &report["validation"]["kicad_drc"];
    if counts.is_null() {
        return PreFabDrc::Unknown;
    }
    let errors = counts["errors"].as_u64().unwrap_or(0);
    let unconnected = counts["unconnected"].as_u64().unwrap_or(0);
    if errors == 0 && unconnected == 0 {
        PreFabDrc::Clean
    } else {
        PreFabDrc::Violations
    }
}

fn manufacturing_status(
    fab_pass: bool,
    timed_out: bool,
    native: Option<&serde_json::Value>,
) -> &'static str {
    if timed_out {
        return "unknown";
    }
    let native_unknown = native
        .and_then(|n| n["stages"].as_array())
        .is_some_and(|stages| stages.iter().any(|s| s["status"] == "unknown"));
    if native_unknown {
        return "unknown";
    }
    if fab_pass {
        "pass"
    } else {
        "fail"
    }
}

fn registry_qualify(
    registry: &synth_registry::Registry,
    report_path: Option<&Path>,
    json: bool,
    only: Option<&str>,
    no_fail: bool,
) -> anyhow::Result<u8> {
    let facts = synth_layout::qualify_facts::InstalledKicad;
    let tool_version = format!("synth-cli {}", env!("CARGO_PKG_VERSION"));

    let mut subject = synth_registry::Registry::new();
    match only {
        Some(id) => {
            let part = registry
                .lookup(id)
                .ok_or_else(|| anyhow::anyhow!("no part `{id}` in the resolved registry"))?;
            subject.insert(part.clone());
        }
        None => {
            for (_, part) in registry.iter() {
                subject.insert(part.clone());
            }
        }
    }

    let report = synth_registry::qualify_registry(&subject, &facts, &facts, &tool_version);

    if let Some(path) = report_path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        eprintln!("wrote {}", path.display());
    } else if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_qualification(&report);
    }

    Ok(if no_fail || report.is_clean() {
        EXIT_SUCCESS
    } else {
        EXIT_VALIDATION_ERRORS
    })
}

fn print_qualification(report: &synth_registry::QualificationReport) {
    use synth_registry::{CheckStatus, FindingLevel};

    for part in &report.parts {
        // A qualified part with review findings still has something to say;
        // only a part with nothing at all to report is skipped.
        if part.status == CheckStatus::Pass && part.review_findings().next().is_none() {
            continue;
        }
        println!("{} [{}]", part.part_id, part.status.as_str());
        for finding in part.findings() {
            let tag = match finding.level {
                FindingLevel::Blocking => "blocking",
                FindingLevel::Review => "review",
            };
            println!("  {tag} {}: {}", finding.code, finding.message);
            println!("    expected: {}", finding.expected);
            println!("    found:    {}", finding.found);
        }
        for check in part.unknown_checks() {
            println!(
                "  unknown {}: {}",
                check.name,
                check.unknown_reason.as_deref().unwrap_or("no reason given")
            );
        }
    }

    let s = &report.summary;
    println!(
        "{} part(s): {} qualified, {} blocked, {} unproven",
        s.total, s.qualified, s.blocked, s.unproven
    );
    if s.with_review_findings > 0 {
        println!(
            "{} qualified part(s) carry review findings; these are reported, not fatal",
            s.with_review_findings
        );
    }
    if s.unproven > 0 {
        println!(
            "unproven parts have checks that could not run; install KiCad or set \
             KICAD_FOOTPRINT_DIR / KICAD_SYMBOL_DIR"
        );
    }
}

fn structural_refusal(board: &synth_ir::Board) -> Option<String> {
    let facts = synth_layout::qualify_facts::InstalledKicad;
    let mut subject = synth_registry::Registry::new();
    for component in &board.components {
        if let Some(part) = component.part.as_ref() {
            subject.insert(part.clone());
        }
    }
    if subject.is_empty() {
        return None;
    }

    let report = synth_registry::qualify_registry(
        &subject,
        &facts,
        &facts,
        &format!("synth-cli {}", env!("CARGO_PKG_VERSION")),
    );

    let defective: Vec<&synth_registry::PartQualification> =
        report.structurally_defective().collect();
    if defective.is_empty() {
        return None;
    }

    let mut out = String::from(
        "error: [E-SYNTH-QUAL-000] refusing fab export: part definitions are wrong about \
         their physical package — a pin map that disagrees with the footprint or symbol, or a \
         pin whose declared role contradicts the package. That routes nets to the wrong pads, \
         which no flag should wave through.\n",
    );
    for part in defective {
        for finding in part.structural_defects() {
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "  {} [{}]: {}\n    expected: {}\n    found:    {}",
                part.part_id, finding.code, finding.message, finding.expected, finding.found
            );
        }
    }
    out.push_str("Fix the part definitions, or run `synth registry qualify` for the full report.");
    Some(out)
}

fn run_check_child(
    exe: &Path,
    args: &[String],
    timeout_seconds: u64,
) -> anyhow::Result<(std::process::Output, bool)> {
    let mut child = ProcessCommand::new(exe)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_seconds);
    loop {
        if child.try_wait()?.is_some() {
            return Ok((child.wait_with_output()?, false));
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            return Ok((child.wait_with_output()?, true));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Handler for `synth registry {path,list,doctor}` (Phase 15, R15.2).
fn registry_cmd(
    cmd: RegistryCommand,
    registry: Option<&Path>,
    user_registry: Option<&Path>,
    strict: bool,
) -> anyhow::Result<u8> {
    let user = user_registry
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir);

    match cmd {
        RegistryCommand::Install { dir } => {
            let target_dir = dir
                .map(|d| d.join("parts"))
                .or_else(|| synth_registry::shipped_registry_dir().map(|d| d.join("parts")))
                .ok_or_else(|| {
                    anyhow::anyhow!("no install target: pass --dir or set XDG_DATA_HOME/HOME")
                })?;
            let count = synth_registry::write_embedded_seed(&target_dir)
                .map_err(|e| anyhow::anyhow!("install failed: {e}"))?;
            println!("installed {count} seed parts into {}", target_dir.display());
            println!(
                "resolution order: --registry > SYNTH_REGISTRY > ./registry/parts (with \
                 ancestors) > this directory > embedded seed"
            );
            Ok(EXIT_SUCCESS)
        }
        RegistryCommand::Path => {
            match resolve_tier1(registry) {
                Tier1Source::Dir(dir) => println!("tier1 (shipped): {}", dir.display()),
                Tier1Source::Embedded => {
                    println!("tier1 (shipped): <embedded seed registry>");
                    println!(
                        "                 (no registry/parts found; pass --registry, set \
                         SYNTH_REGISTRY, run from a synth checkout, or run \
                         `synth registry install`)"
                    );
                }
            }
            match &user {
                Some(u) => println!("tier2 (user):    {}", u.display()),
                None => println!(
                    "tier2 (user):    <not configured; set SYNTH_USER_REGISTRY_DIR or XDG_DATA_HOME>"
                ),
            }
            Ok(EXIT_SUCCESS)
        }
        RegistryCommand::Manifest { output, verify } => match resolve_tier1(registry) {
            Tier1Source::Dir(dir) => registry_manifest(&dir, output.as_deref(), verify.as_deref()),
            Tier1Source::Embedded => anyhow::bail!(
                "registry manifest requires an on-disk registry — only the embedded \
                     seed was found; run from a synth checkout or pass --registry"
            ),
        },
        RegistryCommand::Qualify { .. } => {
            let res = match (resolve_tier1(registry), user.as_deref()) {
                (Tier1Source::Dir(dir), Some(u)) if u.exists() => {
                    synth_registry::load_tiered(&dir, u, strict)
                        .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?
                }
                (Tier1Source::Dir(dir), _) => synth_registry::LoadResult {
                    registry: synth_registry::load_dir(&dir)
                        .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?,
                    warnings: Vec::new(),
                },
                (Tier1Source::Embedded, Some(u)) if u.exists() => {
                    synth_registry::load_user_overlay(
                        synth_registry::embedded_registry().clone(),
                        u,
                        strict,
                    )
                    .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?
                }
                (Tier1Source::Embedded, _) => synth_registry::LoadResult {
                    registry: synth_registry::embedded_registry().clone(),
                    warnings: Vec::new(),
                },
            };
            let RegistryCommand::Qualify {
                report,
                json,
                part,
                no_fail,
            } = cmd
            else {
                unreachable!("matched Qualify above")
            };
            registry_qualify(
                &res.registry,
                report.as_deref(),
                json,
                part.as_deref(),
                no_fail,
            )
        }
        RegistryCommand::List | RegistryCommand::Doctor => {
            let res = match (resolve_tier1(registry), user.as_deref()) {
                (Tier1Source::Dir(dir), Some(u)) if u.exists() => {
                    synth_registry::load_tiered(&dir, u, strict)
                        .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?
                }
                (Tier1Source::Dir(dir), _) => synth_registry::LoadResult {
                    registry: synth_registry::load_dir(&dir)
                        .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?,
                    warnings: Vec::new(),
                },
                (Tier1Source::Embedded, Some(u)) if u.exists() => {
                    synth_registry::load_user_overlay(
                        synth_registry::embedded_registry().clone(),
                        u,
                        strict,
                    )
                    .map_err(|e| anyhow::anyhow!("registry load failed: {e}"))?
                }
                (Tier1Source::Embedded, _) => synth_registry::LoadResult {
                    registry: synth_registry::embedded_registry().clone(),
                    warnings: Vec::new(),
                },
            };
            match cmd {
                RegistryCommand::List => {
                    for id in res.registry.ids() {
                        println!("{id}");
                    }
                }
                RegistryCommand::Doctor => {
                    let mut problems = 0;
                    for w in &res.warnings {
                        let synth_registry::LoadWarning::Shadow { id, user_path } = w;
                        println!(
                            "W-SYNTH-REG-001: user part `{id}` shadows shipped (at {})",
                            user_path.display()
                        );
                        problems += 1;
                    }
                    for (_, part) in res.registry.iter() {
                        if part.is_unverified() {
                            println!(
                                "W-SYNTH-PART-UNVERIFIED: part `{}` has no reviewer",
                                part.id
                            );
                            problems += 1;
                        }
                    }
                    if problems == 0 {
                        println!("ok: no shadowed or unverified parts");
                    }
                }
                _ => unreachable!(),
            }
            Ok(EXIT_SUCCESS)
        }
    }
}

/// Handler for `synth part import lcsc` (Phase 15, R15.4).
fn part_cmd(
    cmd: PartCommand,
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
) -> anyhow::Result<u8> {
    match cmd {
        PartCommand::ImportLcsc {
            code,
            from_file,
            footprint_dir,
        } => import_lcsc(
            &code,
            from_file.as_deref(),
            user_registry,
            footprint_dir.as_deref(),
        ),
        PartCommand::ImportKicad {
            lib_id,
            id,
            footprint,
            batch,
            limit,
        } => match batch {
            Some(file) => import_kicad_batch(&file, limit, user_registry),
            None => import_kicad(&lib_id, id.as_deref(), footprint.as_deref(), user_registry),
        },
        PartCommand::ImportZip { zip_path, id } => {
            import_kicad_zip(&zip_path, id.as_deref(), user_registry)
        }
        PartCommand::ResolveFootprint { apply, part_id } => {
            part_resolve_footprint(registry_dir, user_registry, apply, part_id.as_deref())
        }
        PartCommand::Stub {
            id,
            pins,
            user_registry,
        } => part_stub(&id, pins, user_registry.as_deref()),
    }
}

/// R15.10: generate or verify a SHA256 manifest of the Tier-1 registry.
/// Format: one `<sha256>  <relative-path>` line per file (sha256sum
/// compatible), sorted by path for determinism. The release pipeline
/// signs this file; CI verifies the tree against it.
fn registry_manifest(
    tier1: &Path,
    output: Option<&Path>,
    verify: Option<&Path>,
) -> anyhow::Result<u8> {
    use sha2::{Digest, Sha256};
    use std::io::Write as IoWrite;

    if !tier1.is_dir() {
        anyhow::bail!("Tier-1 registry directory not found: {}", tier1.display());
    }

    // Collect + hash every file under the Tier-1 tree, sorted by
    // relative path so the manifest is byte-stable across runs.
    let mut entries: Vec<(String, [u8; 32])> = Vec::new();
    let mut stack = vec![tier1.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children: Vec<_> = std::fs::read_dir(&dir)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|e| e.path())
            .collect();
        children.sort();
        for path in children {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let rel = path
                    .strip_prefix(tier1)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::read(&path)?;
                entries.push((rel, Sha256::digest(&bytes).into()));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let render = |entries: &[(String, [u8; 32])]| -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (rel, hash) in entries {
            for b in hash {
                let _ = write!(out, "{b:02x}");
            }
            let _ = writeln!(out, "  {rel}");
        }
        out
    };

    if let Some(manifest_path) = verify {
        let expected = std::fs::read_to_string(manifest_path).map_err(|e| {
            anyhow::anyhow!("cannot read manifest {}: {e}", manifest_path.display())
        })?;
        let actual = render(&entries);
        if expected == actual {
            println!(
                "manifest OK: {} files match {}",
                entries.len(),
                manifest_path.display()
            );
            Ok(EXIT_SUCCESS)
        } else {
            let expected_lines: std::collections::BTreeSet<&str> = expected.lines().collect();
            let actual_lines: std::collections::BTreeSet<&str> = actual.lines().collect();
            for line in expected_lines.symmetric_difference(&actual_lines) {
                eprintln!("manifest mismatch: {line}");
            }
            anyhow::bail!(
                "Tier-1 registry does not match manifest {}",
                manifest_path.display()
            );
        }
    } else {
        let text = render(&entries);
        if let Some(path) = output {
            std::fs::write(path, &text)?;
            println!("wrote {} ({} files)", path.display(), entries.len());
        } else {
            let mut stdout = std::io::stdout();
            stdout.write_all(text.as_bytes())?;
        }
        Ok(EXIT_SUCCESS)
    }
}

/// R15.9: collect the deduplicated ids of every board part that would
/// export with a synthesized bounding-box footprint fallback — i.e.
/// parts with no `kicad_footprint` at all, or whose referenced
/// footprint cannot be resolved to real pad geometry on disk.
fn parts_without_real_footprint(board: &synth_ir::Board) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut missing = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if !seen.insert(part.id.as_str().to_string()) {
            continue;
        }
        let resolves = part
            .kicad_footprint
            .as_deref()
            .and_then(synth_layout::kicad_footprint_loader::pads)
            .is_some();
        if !resolves {
            missing.push(part.id.as_str().to_string());
        }
    }
    missing
}

/// R15.3 trust boundary: part ids on `board` that carry `[provenance]`
/// but have no `reviewed_by` reviewer (`W-SYNTH-PART-UNVERIFIED`). Legacy
/// seed parts without a `[provenance]` section are trusted and excluded.
fn parts_unverified(board: &synth_ir::Board) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unverified = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if !seen.insert(part.id.as_str().to_string()) {
            continue;
        }
        if part.is_unverified() {
            unverified.push(part.id.as_str().to_string());
        }
    }
    unverified
}

/// R15.8 step 1: write a `create_part_stub` skeleton into the Tier-2
/// registry so agents can start from a valid TOML file instead of
/// hallucinating pinouts.
fn part_stub(id: &str, pins: usize, user_registry: Option<&Path>) -> anyhow::Result<u8> {
    if id.is_empty() {
        eprintln!("error: part id must not be empty");
        return Ok(1);
    }
    if pins == 0 {
        eprintln!("error: --pins must be at least 1");
        return Ok(1);
    }
    let dir = user_registry
        .map(Path::to_path_buf)
        .or_else(synth_registry::user_registry_dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Tier-2 registry: pass --user-registry or set SYNTH_USER_REGISTRY_DIR"
            )
        })?;
    std::fs::create_dir_all(&dir)?;
    let pin_names: Vec<String> = (1..=pins).map(|n| n.to_string()).collect();
    let toml_str = synth_registry::create_part_stub(id, &pin_names);
    let path = dir.join(format!("{id}.synth.toml"));
    std::fs::write(&path, toml_str)?;
    println!("wrote {}", path.display());
    println!(
        "next: fill in the real pinout from the datasheet, then set \
         [provenance].reviewed_by to clear W-SYNTH-PART-UNVERIFIED"
    );
    Ok(0)
}

fn import_lcsc(
    code: &str,
    from_file: Option<&Path>,
    user_registry: Option<&Path>,
    footprint_dir: Option<&Path>,
) -> anyhow::Result<u8> {
    // 1. Resolve the Tier-2 (per-user) registry directory.
    let user_dir = user_registry
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Tier-2 user registry configured; pass --user-registry or set SYNTH_USER_REGISTRY_DIR"
            )
        })?;
    std::fs::create_dir_all(&user_dir)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", user_dir.display()))?;

    // 2. Acquire the footprint CAD: a cached document, or a live fetch of
    //    LCSC's `pcbSvg` for this part.
    let raw = match from_file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?,
        None => fetch_lcsc_footprint_svg(code)?,
    };

    // 3. Parse + convert the CAD geometry into a KiCad footprint.
    //
    //    A document with no pads is a hard error, never an empty footprint:
    //    a pad-less `.kicad_mod` satisfies the "has a real footprint" export
    //    gate while being unbuildable, which is the exact failure the old
    //    EasyEDA-shape importer had.
    let fp = synth_registry::parse_component_svg(&raw).ok_or_else(|| {
        anyhow::anyhow!(
            "no pads found in the CAD document for {code}. Expected an LCSC component \
             footprint SVG (elements with c_partid=\"part_pad\"). If this came from \
             --from-file, check that the file is the `pcbSvg`, not the schematic symbol."
        )
    })?;
    let id = code.to_lowercase();
    let modl = synth_registry::svg_to_kicad_mod(&fp, &id);
    // The part's pins come from the pad designators. That is enough to
    // import the footprint and clear E-SYNTH-PIN-001, but it carries no
    // electrical types or capabilities: those come from the datasheet, and
    // the entry stays unverified until a human says so.
    let pins: Vec<String> = fp.pads.iter().map(|p| p.number.clone()).collect();

    // 4. Write the `.kicad_mod` into the user footprint directory.
    let fp_dir = footprint_dir.map_or_else(|| user_dir.join("footprints"), PathBuf::from);
    std::fs::create_dir_all(&fp_dir)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", fp_dir.display()))?;
    let pretty = fp_dir.join(format!("{id}.pretty"));
    std::fs::create_dir_all(&pretty)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", pretty.display()))?;
    let kmod = pretty.join(format!("{id}.kicad_mod"));
    std::fs::write(&kmod, &modl)
        .map_err(|e| anyhow::anyhow!("could not write {}: {e}", kmod.display()))?;

    // 5. Write the unverified `.synth.toml` skeleton into the user registry.
    let toml = synth_registry::easyeda::generate_part_toml(
        &id,
        code,
        "",
        "",
        &format!("https://lcsc.com/p/{code}.html"),
        &pins,
    );
    let part_path = user_dir.join(format!("{id}.synth.toml"));
    std::fs::write(&part_path, &toml)
        .map_err(|e| anyhow::anyhow!("could not write {}: {e}", part_path.display()))?;

    println!("imported {code} -> {}", part_path.display());
    println!("footprint   -> {}", kmod.display());
    if let Some(package) = &fp.package {
        println!("package     -> {package}");
    }
    println!(
        "pads ({}), canvas {:.2} x {:.2} mm",
        fp.pads.len(),
        fp.canvas_mm.0,
        fp.canvas_mm.1
    );
    println!("pad numbers: {}", pins.join(", "));
    println!(
        "next: set SYNTH_USER_FOOTPRINT_DIR={} when exporting a board using this part.",
        fp_dir.display()
    );
    println!(
        "pins were taken from the footprint's pad designators only. Fill in each \
         pin's name, electrical_type and capabilities from the datasheet, then set \
         [provenance].reviewed_by."
    );
    println!(
        "orientation is as EasyEDA draws it, which is not always the datasheet \
         orientation. CHECK PIN 1 against the datasheet before relying on this part."
    );
    println!(
        "this part is UNVERIFIED (provenance.source = imported, reviewed_by empty) until reviewed."
    );
    Ok(EXIT_SUCCESS)
}

/// Map a KiCad symbol pin electrical-type keyword to synth's `ElectricalType`
/// snake_case serialization. Unknown/unsupported types fall back to
/// `unclassified` so the skeleton still loads; the reviewer should confirm.
fn map_kicad_electrical_type(kicad: &str) -> &'static str {
    match kicad {
        "power_in" => "power_input",
        "power_out" => "power_output",
        "input" => "input",
        "output" => "output",
        "bidirectional" => "bidirectional",
        "passive" => "passive",
        "tri_state" => "three_statable",
        "open_collector" => "open_drain_low",
        "open_emitter" => "open_drain_high",
        "analog" => "analog",
        "clock" => "clock",
        "nc" | "no_connect" => "do_not_connect",
        _ => "unclassified",
    }
}

/// Infer a sensible default `kind` from the KiCad library/symbol name so the
/// generated skeleton needs less hand-editing. Conservative: anything
/// unrecognized is `ic`.
fn infer_kind(lib_id: &str) -> &'static str {
    let (lib, sym) = lib_id.split_once(':').unwrap_or((lib_id, ""));
    let lib = lib.to_lowercase();
    let sym = sym.to_lowercase();
    if lib.contains("resistor") || sym == "r" {
        "resistor"
    } else if lib.contains("capacitor") || sym == "c" {
        "capacitor"
    } else if lib.contains("inductor") || sym == "l" {
        "inductor"
    } else if lib.contains("connector")
        || lib.contains("socket")
        || sym.starts_with('j')
        || sym.starts_with('p')
        || sym.contains("conn")
    {
        "connector"
    } else if lib.contains("led") || lib.contains("diode") || sym == "d" || sym.contains("led") {
        "diode"
    } else if lib.contains("crystal") || lib.contains("oscillator") || sym.contains("xtal") {
        "crystal"
    } else {
        "ic"
    }
}

/// `synth part import kicad <lib_id>` (Phase 15, R15.5).
fn import_kicad(
    lib_id: &str,
    id_override: Option<&str>,
    footprint: Option<&str>,
    user_registry: Option<&Path>,
) -> anyhow::Result<u8> {
    let user_dir = user_registry
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Tier-2 user registry configured; pass --user-registry or set SYNTH_USER_REGISTRY_DIR"
            )
        })?;
    let pin_count = import_kicad_one(lib_id, id_override, footprint, &user_dir, false)?;
    if pin_count > 0 {
        println!(
            "this part is UNVERIFIED (provenance.source = imported, reviewed_by empty) until reviewed."
        );
    }
    Ok(EXIT_SUCCESS)
}

/// R15.12 seed growth: bulk-import every `lib_id` listed in `batch_file`
/// (one per line, `#` comments allowed) into the Tier-2 registry. Each
/// imported part lands unverified; the human review pass that sets
/// `reviewed_by` promotes it toward the Tier-1 seed target.
fn import_kicad_batch(
    batch_file: &Path,
    limit: Option<usize>,
    user_registry: Option<&Path>,
) -> anyhow::Result<u8> {
    let user_dir = user_registry
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Tier-2 user registry configured; pass --user-registry or set SYNTH_USER_REGISTRY_DIR"
            )
        })?;

    let raw = std::fs::read_to_string(batch_file)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", batch_file.display()))?;
    let lib_ids: Vec<String> = raw
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if lib_ids.is_empty() {
        anyhow::bail!("no lib_ids found in {}", batch_file.display());
    }

    let mut imported = 0usize;
    let mut failed: Vec<(String, String)> = Vec::new();
    for lib_id in &lib_ids {
        if let Some(cap) = limit {
            if imported >= cap {
                break;
            }
        }
        match import_kicad_one(lib_id, None, None, &user_dir, true) {
            Ok(_) => imported += 1,
            Err(e) => failed.push((lib_id.clone(), e.to_string())),
        }
    }

    println!();
    println!(
        "batch complete: {imported} imported, {} failed, from {} lib_ids",
        failed.len(),
        lib_ids.len()
    );
    for (lib_id, err) in &failed {
        println!("  FAILED {lib_id}: {err}");
    }
    if imported > 0 {
        println!(
            "all results are UNVERIFIED — run `synth registry doctor` for the review queue; \
             set reviewed_by after checking each pinout (see registry/CREDITS.md)."
        );
    }
    Ok(EXIT_SUCCESS)
}

/// Import one KiCad stock symbol into the Tier-2 registry. Returns the
/// pin count; `quiet` suppresses the per-pin listing used by batch mode.
fn import_kicad_one(
    lib_id: &str,
    id_override: Option<&str>,
    footprint: Option<&str>,
    user_dir: &Path,
    quiet: bool,
) -> anyhow::Result<usize> {
    use std::fmt::Write as _;
    std::fs::create_dir_all(user_dir)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", user_dir.display()))?;

    // Extract the physical pin inventory from the installed KiCad symbol.
    let pins = synth_layout::kicad_lib_loader::physical_pins(lib_id).ok_or_else(|| {
        anyhow::anyhow!(
            "could not load KiCad symbol '{lib_id}'; is KiCad installed and KICAD_SYMBOL_DIR set?"
        )
    })?;
    if pins.is_empty() {
        anyhow::bail!("KiCad symbol '{lib_id}' exposes no pins");
    }

    // Derive the PartId (filename stem).
    let default_id = lib_id
        .split_once(':')
        .map_or(lib_id, |(_, s)| s)
        .to_lowercase()
        .replace([' ', '-', '.', '/'], "_");
    let part_id = id_override.unwrap_or(&default_id);

    // Emit the `.synth.toml` skeleton with pins pre-filled from KiCad.
    let kind = infer_kind(lib_id);
    let mut out = String::new();
    let _ = writeln!(out, "id = \"{part_id}\"");
    let _ = writeln!(out, "kind = \"{kind}\"");
    let _ = writeln!(
        out,
        "description = \"Imported from KiCad stock symbol {lib_id}\""
    );
    let _ = writeln!(out, "kicad_symbol = \"{lib_id}\"");
    if let Some(fp) = footprint {
        let _ = writeln!(out, "kicad_footprint = \"{fp}\"");
    }
    let _ = writeln!(out);

    // Multi-unit symbols (dual op-amps, quad gates, dual flip-flops, ...)
    // repeat the same KiCad pin name once per unit (every gate has an
    // "A"/"~{Q}"/...); Synth requires pin names unique per part, so
    // disambiguate any name shared by more than one physical pin number by
    // suffixing the pin number. Pins with a name already unique are left
    // untouched so the common single-unit case still gets clean names.
    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for pin in &pins {
        if !pin.name.is_empty() {
            *name_counts.entry(pin.name.as_str()).or_insert(0) += 1;
        }
    }

    for pin in &pins {
        let name = if pin.name.is_empty() {
            pin.number.clone()
        } else if name_counts.get(pin.name.as_str()).copied().unwrap_or(0) > 1 {
            format!("{}_{}", pin.name, pin.number)
        } else {
            pin.name.clone()
        };
        let et = map_kicad_electrical_type(&pin.electrical_type);
        let _ = writeln!(out, "[[pins]]");
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "number = \"{}\"", pin.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{et}\"  # carried from KiCad; confirm from datasheet"
        );
        let _ = writeln!(out, "required = false");
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  # NOTE: capabilities default to empty; complete them from the"
    );
    let _ = writeln!(
        out,
        "  # datasheet before fabrication. Run `synth registry doctor` to re-check"
    );
    let _ = writeln!(
        out,
        "  # the pinout against the KiCad library via kicad_pin_check."
    );
    let _ = writeln!(
        out,
        "  # Pin names shared across multiple units (e.g. dual/quad gates) were"
    );
    let _ = writeln!(
        out,
        "  # suffixed with their pin number to stay unique; rename them to match"
    );
    let _ = writeln!(out, "  # the datasheet's per-unit convention if preferred.");
    let _ = writeln!(out);
    let _ = writeln!(out, "[provenance]");
    let _ = writeln!(out, "source = \"imported\"");
    let _ = writeln!(out, "generator = \"synth-part-import-kicad 0.1\"");
    let _ = writeln!(out, "reviewed_by = \"\"");

    let part_path = user_dir.join(format!("{part_id}.synth.toml"));
    std::fs::write(&part_path, &out)
        .map_err(|e| anyhow::anyhow!("could not write {}: {e}", part_path.display()))?;

    if quiet {
        println!("imported {lib_id} -> {}", part_path.display());
    } else {
        println!("imported {lib_id} -> {}", part_path.display());
        println!("pins ({}):", pins.len());
        for pin in &pins {
            let et = map_kicad_electrical_type(&pin.electrical_type);
            println!("  {}  {}  ({et})", pin.number, pin.name);
        }
    }
    Ok(pins.len())
}

/// Import a part from a SnapEDA/UltraLibrarian "Export to KiCad" zip
/// (or any single-part KiCad-format export zip — see
/// `PartCommand::ImportZip` for the ToS-compliance rationale: this
/// only ever reads the zip already on disk, never contacts either
/// vendor's site).
fn import_kicad_zip(
    zip_path: &Path,
    id_override: Option<&str>,
    user_registry: Option<&Path>,
) -> anyhow::Result<u8> {
    use std::fmt::Write as _;

    let user_dir = user_registry
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Tier-2 user registry configured; pass --user-registry or set SYNTH_USER_REGISTRY_DIR"
            )
        })?;
    std::fs::create_dir_all(&user_dir)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", user_dir.display()))?;

    let bytes = std::fs::read(zip_path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", zip_path.display()))?;
    let import =
        synth_layout::kicad_zip::parse_kicad_zip(&bytes).map_err(|e| anyhow::anyhow!("{e}"))?;

    let default_id = import
        .symbol_name
        .to_lowercase()
        .replace([' ', '-', '.', '/'], "_");
    let part_id = id_override.unwrap_or(&default_id);
    let kind = infer_kind(&import.symbol_name);

    let mut out = String::new();
    let _ = writeln!(out, "id = \"{part_id}\"");
    let _ = writeln!(out, "kind = \"{kind}\"");
    let _ = writeln!(
        out,
        "description = \"Imported from a KiCad-format export zip ({})\"",
        zip_path.display()
    );

    // Copy the footprint into the Tier-2 user footprint directory
    // under the same `<lib>.pretty/<lib>.kicad_mod` naming convention
    // `synth part import lcsc` uses, so `SYNTH_USER_FOOTPRINT_DIR`
    // resolves it identically at export time.
    let mut fp_dir_written: Option<PathBuf> = None;
    if let Some(fp_text) = &import.footprint_text {
        let fp_dir = user_dir.join("footprints");
        let pretty = fp_dir.join(format!("{part_id}.pretty"));
        std::fs::create_dir_all(&pretty)
            .map_err(|e| anyhow::anyhow!("could not create {}: {e}", pretty.display()))?;
        let kmod = pretty.join(format!("{part_id}.kicad_mod"));
        std::fs::write(&kmod, fp_text)
            .map_err(|e| anyhow::anyhow!("could not write {}: {e}", kmod.display()))?;
        let _ = writeln!(out, "kicad_footprint = \"{part_id}:{part_id}\"");
        fp_dir_written = Some(fp_dir);
    }
    let _ = writeln!(out);

    // Multi-unit symbols repeat pin names across units; disambiguate
    // the same way `import-kicad` does.
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
        let et = map_kicad_electrical_type(&pin.electrical_type);
        let _ = writeln!(out, "[[pins]]");
        let _ = writeln!(out, "name = \"{name}\"");
        let _ = writeln!(out, "number = \"{}\"", pin.number);
        let _ = writeln!(
            out,
            "electrical_type = \"{et}\"  # carried from KiCad; confirm from datasheet"
        );
        let _ = writeln!(out, "required = false");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "[provenance]");
    let _ = writeln!(out, "source = \"imported\"");
    let _ = writeln!(out, "generator = \"synth-part-import-zip 0.1\"");
    let _ = writeln!(out, "reviewed_by = \"\"");

    let part_path = user_dir.join(format!("{part_id}.synth.toml"));
    std::fs::write(&part_path, &out)
        .map_err(|e| anyhow::anyhow!("could not write {}: {e}", part_path.display()))?;

    println!("imported {} -> {}", import.symbol_name, part_path.display());
    println!("pins ({}):", import.pins.len());
    for pin in &import.pins {
        let et = map_kicad_electrical_type(&pin.electrical_type);
        println!("  {}  {}  ({et})", pin.number, pin.name);
    }
    if let Some(fp_dir) = fp_dir_written {
        println!(
            "footprint   -> {}",
            fp_dir
                .join(format!("{part_id}.pretty/{part_id}.kicad_mod"))
                .display()
        );
        println!(
            "next: set SYNTH_USER_FOOTPRINT_DIR={} when exporting a board using this part.",
            fp_dir.display()
        );
    } else {
        println!(
            "warning: zip had no .kicad_mod footprint; export will fall back to a synthesized bounding-box footprint until one is added."
        );
    }
    println!(
        "pins were taken from the footprint's pad designators only. Fill in each \
         pin's name, electrical_type and capabilities from the datasheet, then set \
         [provenance].reviewed_by."
    );
    println!(
        "this part is UNVERIFIED (provenance.source = imported, reviewed_by empty) until reviewed."
    );
    Ok(EXIT_SUCCESS)
}

/// Fetch a part's **footprint** CAD from LCSC's public product API.
///
/// Two steps, both on LCSC/JLCPCB-operated hosts:
///
/// 1. `wmsc.lcsc.com/ftps/wm/product/detail` returns the part's metadata,
///    including `edaSvgInfo.pcbSvg` — the URL of the component footprint SVG.
/// 2. That SVG is fetched and returned verbatim.
///
/// # Why not EasyEDA's component JSON
///
/// `easyeda.com/api/products/{code}/components` looks like the obvious source
/// and is what the previous importer used. It is the wrong document: it
/// carries the part's **schematic symbol**, not its footprint. For an
/// aQFN-73 that is two columns of 37 pins — and because the primitives now
/// arrive as compact tilde-delimited strings rather than objects, the old
/// parser matched none of them and emitted a `.kicad_mod` with **zero pads**,
/// silently. The symbol carries no pad geometry at all, so no amount of
/// fixing that parser would have turned it into a footprint.
///
/// The `pcbSvg` is the real land pattern and labels every pad
/// (`c_partid="part_pad" … number=… c_width=… c_height=…`), so
/// [`synth_registry::parse_component_svg`] needs no format archaeology.
///
/// The raw document is handed back for immediate conversion and is never
/// written to disk (see `registry/CREDITS.md`).
fn fetch_lcsc_footprint_svg(code: &str) -> anyhow::Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("could not start tokio runtime: {e}"))?;
    rt.block_on(async {
        // A browser UA: the product API sits behind a CDN that rejects
        // unrecognised agents before returning JSON, which is why the old
        // fetch failed with a *parse* error rather than an HTTP error.
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 ",
                "(KHTML, like Gecko) Chrome/120.0 Safari/537.36"
            ))
            .build()
            .map_err(|e| anyhow::anyhow!("could not build http client: {e}"))?;

        let detail_url = format!("https://wmsc.lcsc.com/ftps/wm/product/detail?productCode={code}");
        let detail: serde_json::Value = client
            .get(&detail_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("fetch {detail_url} failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("{detail_url} did not return JSON: {e}"))?;

        let pcb_svg = detail
            .pointer("/result/edaSvgInfo/pcbSvg")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "LCSC has no EDA footprint published for {code}. The part may be \\
                     assembly-only, have no CAD model, or the code may be wrong. \\
                     Check https://www.lcsc.com/product-detail/{code}"
                )
            })?;
        let url = if pcb_svg.starts_with("//") {
            format!("https:{pcb_svg}")
        } else {
            pcb_svg.to_string()
        };
        client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("fetch {url} failed: {e}"))?
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("reading {url} failed: {e}"))
    })
}

fn read_source(input: &Path) -> anyhow::Result<(String, String)> {
    let source = std::fs::read_to_string(input)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", input.display()))?;
    let file = input.to_string_lossy().into_owned();
    Ok((source, file))
}

/// Aesthetic-schematic ERC thresholds for a design (Phase E): the
/// `[schematic]` section of `<design>.synth.erc.toml` when it exists
/// and parses, else the built-in defaults. A malformed sidecar is
/// reported but never blocks — the defaults are always safe.
///
/// The schematic layout sidecar for `design`.
///
/// `synth layout` reports the *sheet*, so this is the schematic sidecar
/// (sheet millimetres). The PCB placement sidecar is a different file and is
/// not consulted here.
fn sidecar_path_for(design: &Path) -> Option<PathBuf> {
    synth_layout::schematic_sidecar_path(design)
}

fn schem_erc_config_for(design: &Path) -> synth_kicad::SchemErcConfig {
    let settings = match synth_validate::ErcConfig::load_for_design(design) {
        Ok(Some(config)) => config.schematic,
        Ok(None) => synth_validate::config::SchematicSettings::default(),
        Err(e) => {
            eprintln!(
                "warning: ignoring schematic ERC config for {}: {e}",
                design.display()
            );
            synth_validate::config::SchematicSettings::default()
        }
    };
    synth_kicad::SchemErcConfig {
        max_crossings: settings.max_crossings,
        decoupling_max_mm: settings.decoupling_max_mm,
        long_net_max_mm: settings.long_net_max_mm,
        max_junction_degree: settings.max_junction_degree,
        max_net_label_len: settings.max_net_label_len,
        min_sheet_fill_ratio: settings.min_sheet_fill_ratio,
    }
}

/// Where Tier-1 (shipped) parts come from.
#[derive(Debug, Clone)]
enum Tier1Source {
    /// An on-disk `registry/parts` directory.
    Dir(PathBuf),
    /// The seed registry compiled into the binary (`build.rs`
    /// embeds every `registry/parts/**/*.synth.toml`).
    Embedded,
}

/// Resolve the Tier-1 registry: explicit `--registry` flag, then
/// `SYNTH_REGISTRY`, then `registry/parts` in the working directory
/// or any ancestor (git-style), then the XDG *installed* seed
/// (`synth registry install` materializes it under
/// `$XDG_DATA_HOME/synth/registry/shipped`), then the seed registry
/// compiled into the binary. An explicit flag is honoured verbatim
/// even when the directory is missing, so a typo fails loudly
/// instead of silently falling back.
fn resolve_tier1(explicit: Option<&Path>) -> Tier1Source {
    if let Some(p) = explicit {
        return Tier1Source::Dir(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("SYNTH_REGISTRY") {
        if !env.is_empty() {
            return Tier1Source::Dir(PathBuf::from(env));
        }
    }
    if let Some(dir) = discover_registry_dir() {
        return Tier1Source::Dir(dir);
    }
    if let Some(shipped) = synth_registry::shipped_registry_dir() {
        if shipped.join("parts").is_dir() {
            return Tier1Source::Dir(shipped.join("parts"));
        }
    }
    Tier1Source::Embedded
}

/// Walk up from the working directory looking for
/// `registry/parts`, so commands run from a project subdirectory
/// still find the checkout's registry.
fn discover_registry_dir() -> Option<PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        let candidate = cur.join("registry").join("parts");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Print tiered-load warnings (e.g. `W-SYNTH-REG-001` shadowing) to
/// stderr so divergence stays visible without failing the load.
fn emit_registry_warnings(warnings: &[synth_registry::LoadWarning]) {
    for w in warnings {
        let synth_registry::LoadWarning::Shadow { id, user_path } = w;
        eprintln!(
            "synth: W-SYNTH-REG-001: user part `{}` (at {}) shadows a shipped part",
            id,
            user_path.display()
        );
    }
}

/// Load a registry, automatically merging the Tier-2 (per-user) registry
/// on top of the shipped Tier-1 directory when one is present (Phase 15,
/// R15.1). Shadowing is non-fatal here; pass `strict` via
/// [`load_registry_strict`] to promote it to an error.
fn load_registry(registry_dir: Option<&Path>) -> Option<synth_registry::Registry> {
    load_registry_strict(registry_dir, None, false)
}

/// Like [`load_registry`] but honours an explicit `--user-registry` path
/// and `--strict-registry` (shadows become load errors). Returns `None`
/// on any load failure.
fn load_registry_strict(
    registry_dir: Option<&Path>,
    user_dir: Option<&Path>,
    strict: bool,
) -> Option<synth_registry::Registry> {
    let user = user_dir
        .map(PathBuf::from)
        .or_else(synth_registry::user_registry_dir);
    match resolve_tier1(registry_dir) {
        Tier1Source::Dir(dir) => {
            // Agent harnesses historically passed the flat Tier-2 directory
            // through `--registry`. Treat that shape as a user overlay over
            // the embedded seed rather than replacing the shipped registry;
            // otherwise ordinary parts such as USB-C and passives become
            // unexpected hard errors even though the agent registered only
            // the design-specific parts.
            let flat_user_registry = is_flat_user_registry(&dir);
            if flat_user_registry {
                let base = match synth_registry::load_user_overlay(
                    synth_registry::embedded_registry().clone(),
                    &dir,
                    strict,
                ) {
                    Ok(res) => {
                        emit_registry_warnings(&res.warnings);
                        res.registry
                    }
                    Err(e) => {
                        eprintln!("synth: registry load failed (strict): {e}");
                        return None;
                    }
                };
                return match user {
                    Some(u) if u.exists() && u != dir => {
                        match synth_registry::load_user_overlay(base, &u, strict) {
                            Ok(res) => {
                                emit_registry_warnings(&res.warnings);
                                Some(res.registry)
                            }
                            Err(e) => {
                                eprintln!("synth: registry load failed (strict): {e}");
                                None
                            }
                        }
                    }
                    _ => Some(base),
                };
            }
            match user {
                Some(u) if u.exists() => match synth_registry::load_tiered(&dir, &u, strict) {
                    Ok(res) => {
                        emit_registry_warnings(&res.warnings);
                        Some(res.registry)
                    }
                    Err(e) => {
                        eprintln!("synth: registry load failed (strict): {e}");
                        eprintln!(
                            "synth: hint: check --registry, set SYNTH_REGISTRY, or run from a \
                         synth checkout"
                        );
                        None
                    }
                },
                _ => match synth_registry::load_dir(&dir) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        eprintln!("synth: could not load registry from {}: {e}", dir.display());
                        None
                    }
                },
            }
        }
        Tier1Source::Embedded => {
            eprintln!(
                "synth: no registry/parts found — using the embedded seed registry \
                 (hint: pass --registry, set SYNTH_REGISTRY, or run from a synth checkout \
                 to pick up local parts)"
            );
            match user {
                Some(u) if u.exists() => {
                    match synth_registry::load_user_overlay(
                        synth_registry::embedded_registry().clone(),
                        &u,
                        strict,
                    ) {
                        Ok(res) => {
                            emit_registry_warnings(&res.warnings);
                            Some(res.registry)
                        }
                        Err(e) => {
                            eprintln!("synth: registry load failed (strict): {e}");
                            None
                        }
                    }
                }
                _ => Some(synth_registry::embedded_registry().clone()),
            }
        }
    }
}

fn is_flat_user_registry(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut has_toml = false;
    let mut has_subdirectory = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            has_subdirectory = true;
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            has_toml = true;
        }
    }
    has_toml && !has_subdirectory
}

fn validate(
    input: &Path,
    format: Format,
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
    strict_registry: bool,
    parse_only: bool,
) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());

    let mut diagnostics = parse.diagnostics;
    let schematic_sidecar = sidecar_path_for(input);

    if !parse_only {
        if let Some(ast) = parse.ast.as_ref() {
            // Resolve imports against the input file's parent directory
            // (the sandbox root). Imports use relative paths only.
            let import_root = input
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            let loader = synth_ir::FsImportLoader { root: import_root };
            let resolved = synth_ir::resolve_imports(ast, &loader, &file);
            diagnostics.extend(resolved.diagnostics);

            if let Some(registry) =
                load_registry_strict(registry_dir, user_registry, strict_registry)
            {
                let lowered = synth_ir::lower(&resolved.program, &registry, &file);
                diagnostics.extend(lowered.diagnostics);
                if let Some(board) = lowered.board.as_ref() {
                    // Per-design ERC config (`<design>.synth.erc.toml`),
                    // when present, overrides the pin-conflict table and
                    // the deeper-check thresholds.
                    let erc_config = synth_validate::ErcConfig::load_for_design(input)
                        .map_err(|e| anyhow::anyhow!("ERC config: {e}"))?
                        .unwrap_or_default();
                    diagnostics.extend(synth_validate::run_erc_with_config(
                        board,
                        &file,
                        &erc_config,
                    ));
                    // Aesthetic schematic ERC (E-SYNTH-SCHEM-*): advisory
                    // warnings; never blocking. Per-sheet on §P26 split
                    // boards so findings attribute to their page (and the
                    // split itself clears the single-sheet overflow).
                    //
                    // Over the *schematic sidecar* layout, not the bare
                    // auto-layout: the sheet these rules judge is the one
                    // `synth render` and `synth_export` produce, and on the
                    // auto-layout a tuned design reports sheet overflow and
                    // low fill for a drawing that is fine.
                    let global =
                        synth_layout::layout_with_sidecar(board, schematic_sidecar.as_deref());
                    let sheets = synth_layout::sheets::layout_sheets(board, global);
                    let mut schem = synth_kicad::check_schem_erc_sheets(board, &sheets);
                    synth_kicad::attach_schem_erc_locations(&mut schem, board, &file);
                    diagnostics.extend(schem);
                }
            }
        }
    }

    let has_errors = diagnostics.iter().any(|d| d.severity.is_blocking());

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    match format {
        Format::Json => {
            let payload = serde_json::json!({
                "schema_version": synth_diagnostics::SCHEMA_VERSION,
                "diagnostics": diagnostics,
            });
            serde_json::to_writer_pretty(&mut out, &payload)?;
            writeln!(&mut out)?;
        }
        Format::Human => {
            if diagnostics.is_empty() {
                writeln!(&mut out, "ok: {}", input.display())?;
            } else {
                for d in &diagnostics {
                    let where_ = d.location.as_ref().map_or_else(
                        || "?".into(),
                        |l| format!("{}:{}-{}", l.file, l.span.byte_start, l.span.byte_end),
                    );
                    writeln!(
                        &mut out,
                        "{}: [{}] {} ({})",
                        d.severity, d.code, d.title, where_
                    )?;
                }
            }
        }
    }

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

fn dump_ast(input: &Path, pretty: bool) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let result = synth_parser::parse(&source, file);
    write_diagnostics_to_stderr(&result.diagnostics)?;

    let has_errors = result.has_errors();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut out, &result.ast)?;
    } else {
        serde_json::to_writer(&mut out, &result.ast)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

fn dump_ir(input: &Path, registry_dir: Option<&Path>, pretty: bool) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };

    let mut has_errors = parse.has_errors();
    let board = if let Some(ast) = parse.ast.as_ref() {
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        write_diagnostics_to_stderr(&resolved.diagnostics)?;
        if resolved
            .diagnostics
            .iter()
            .any(|d| d.severity.is_blocking())
        {
            has_errors = true;
        }
        if let Some(registry) = load_registry(registry_dir) {
            let lowered = synth_ir::lower(&resolved.program, &registry, &file);
            write_diagnostics_to_stderr(&lowered.diagnostics)?;
            if lowered.has_errors() {
                has_errors = true;
            }
            lowered.board
        } else {
            None
        }
    } else {
        None
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut out, &board)?;
    } else {
        serde_json::to_writer(&mut out, &board)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

fn dump_layout(
    input: &Path,
    registry_dir: Option<&Path>,
    pretty: bool,
    score: bool,
) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };

    let mut has_errors = parse.has_errors();
    let layout_and_score = if let Some(ast) = parse.ast.as_ref() {
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        write_diagnostics_to_stderr(&resolved.diagnostics)?;
        if resolved
            .diagnostics
            .iter()
            .any(|d| d.severity.is_blocking())
        {
            has_errors = true;
        }
        if let Some(registry) = load_registry(registry_dir) {
            let lowered = synth_ir::lower(&resolved.program, &registry, &file);
            write_diagnostics_to_stderr(&lowered.diagnostics)?;
            if lowered.has_errors() {
                has_errors = true;
            }
            lowered.board.as_ref().map(|board| {
                // Honour `<design>.schematic.layout.toml` like every other
                // consumer (preview, export) — `synth layout` is what an
                // agent inspects, so it must reflect persisted refinements.
                let layout =
                    synth_layout::layout_with_sidecar(board, sidecar_path_for(input).as_deref());
                let layout_score = score.then(|| {
                    let mut s = synth_layout::score::score(&layout, board);
                    s.aesthetic_violations = synth_kicad::check_schem_erc_with_config(
                        &layout,
                        board,
                        schem_erc_config_for(input),
                    )
                    .into_iter()
                    .map(|d| d.code)
                    .collect();
                    s
                });
                (layout, layout_score)
            })
        } else {
            None
        }
    } else {
        None
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let output = match layout_and_score {
        Some((layout, Some(layout_score))) => {
            let mut value = serde_json::to_value(&layout)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("score".to_string(), serde_json::to_value(&layout_score)?);
            }
            value
        }
        Some((layout, None)) => serde_json::to_value(&layout)?,
        None => serde_json::Value::Null,
    };
    if pretty {
        serde_json::to_writer_pretty(&mut out, &output)?;
    } else {
        serde_json::to_writer(&mut out, &output)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

fn dump_place(
    input: &Path,
    registry_dir: Option<&Path>,
    width: Option<f64>,
    height: Option<f64>,
    board_family: Option<&str>,
    pretty: bool,
) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };

    let mut has_errors = parse.has_errors();
    let placement = if let Some(ast) = parse.ast.as_ref() {
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        write_diagnostics_to_stderr(&resolved.diagnostics)?;
        if resolved
            .diagnostics
            .iter()
            .any(|d| d.severity.is_blocking())
        {
            has_errors = true;
        }
        if let Some(registry) = load_registry(registry_dir) {
            let lowered = synth_ir::lower(&resolved.program, &registry, &file);
            write_diagnostics_to_stderr(&lowered.diagnostics)?;
            if lowered.has_errors() {
                has_errors = true;
            }
            // Surface placement failures as structured
            // `E-SYNTH-PLACE-*` diagnostics (slice 5).
            let family_dimensions = board_family
                .map(|name| {
                    synth_place::board_family::get(name)
                        .ok_or_else(|| anyhow::anyhow!("unsupported board family: {name}"))
                        .map(|profile| (profile.width_mm, profile.height_mm))
                })
                .transpose()?;
            if let Some(name) = board_family {
                let profile =
                    synth_place::board_family::get(name).expect("board family was resolved above");
                let layers = lowered.board.as_ref().map_or(0, |board| board.layers);
                if !synth_place::board_family::supports_layers(profile, layers) {
                    anyhow::bail!("board family {name} does not support {layers} board layers")
                }
            }
            if family_dimensions.is_some() && (width.is_some() || height.is_some()) {
                anyhow::bail!("--board-family cannot be combined with --width/--height")
            }
            let requested_dimensions = match (width, height, family_dimensions) {
                (Some(w), Some(h), None) => Some((w, h)),
                (None, None, family) => family,
                _ => {
                    anyhow::bail!("--width and --height must be supplied together")
                }
            };
            lowered.board.as_ref().and_then(|b| {
                let result = match requested_dimensions {
                    Some((w, h)) => synth_place::place_with_dimensions(b, w, h),
                    None => synth_place::place(b),
                };
                match result {
                    // Publish the external coordinate space (footprint origin),
                    // not the internal courtyard centre, so `synth place` and
                    // the exported PCB agree. See `synth_place::to_external`.
                    Ok(p) => Some(synth_place::to_external(b, &p)),
                    Err(e) => {
                        let diags = e.to_diagnostics(b, &file);
                        let _ = write_diagnostics_to_stderr(&diags);
                        has_errors = true;
                        None
                    }
                }
            })
        } else {
            None
        }
    } else {
        None
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut out, &placement)?;
    } else {
        serde_json::to_writer(&mut out, &placement)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

/// Route a design through the external router and report the run record.
///
/// The command keeps its name and its JSON-on-stdout contract, but the
/// answer is now the run record: which engine ran, at what version, over
/// what input, and which of the four terminal states the run reached. An
/// exit code of zero means the board is independently validated, not that
/// a router exited successfully.
fn dump_route(
    input: &Path,
    registry_dir: Option<&Path>,
    engine: ExternalRouter,
    options: &RouterOptions,
    best_of: bool,
    pretty: bool,
) -> anyhow::Result<u8> {
    let report = route_design(input, registry_dir, engine, options, best_of)?;
    print_route_report(&report);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let value = serde_json::to_value(&report)?;
    if pretty {
        serde_json::to_writer_pretty(&mut out, &value)?;
    } else {
        serde_json::to_writer(&mut out, &value)?;
    }
    writeln!(&mut out)?;

    Ok(if report.is_fabrication_ready() {
        EXIT_SUCCESS
    } else {
        EXIT_VALIDATION_ERRORS
    })
}

/// Compile, place, export, and route a design through the external router.
///
/// The un-routed export lands in a scratch directory rather than over the
/// operator's own output tree, so inspecting a route never disturbs a
/// previous package.
fn route_design(
    input: &Path,
    registry_dir: Option<&Path>,
    engine: ExternalRouter,
    options: &RouterOptions,
    best_of: bool,
) -> anyhow::Result<synth_router::RouteReport> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let import_root = input
        .parent()
        .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let ast = parse
        .ast
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("parse failed; nothing to route"))?;
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    write_diagnostics_to_stderr(&resolved.diagnostics)?;
    let registry = load_registry(registry_dir)
        .ok_or_else(|| anyhow::anyhow!("registry could not be loaded"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    write_diagnostics_to_stderr(&lowered.diagnostics)?;
    let board = lowered
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lowering produced no IR; nothing to route"))?;

    let out_dir = std::env::temp_dir().join(format!(
        "synth-route-{}-{}",
        board.name.replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-',
            "_"
        ),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
    let sidecars = synth_kicad::Sidecars::resolve_for(input);
    let result = synth_kicad::export_with_sidecars(board, &out_dir, &sidecars)
        .map_err(|e| anyhow::anyhow!("kicad export failed: {e}"))?;
    route_externally(board, &result, input, engine, options, best_of)
}

/// Report the physical state of a design after routing it externally.
///
/// The independent checks are the same ones the export gate runs, on the
/// same board, so a clean result here means the same thing it does there.
fn dump_drc(
    input: &Path,
    registry_dir: Option<&Path>,
    engine: ExternalRouter,
    options: &RouterOptions,
    pretty: bool,
) -> anyhow::Result<u8> {
    let report = route_design(input, registry_dir, engine, options, false)?;
    print_route_report(&report);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let value = serde_json::to_value(&report)?;
    if pretty {
        serde_json::to_writer_pretty(&mut out, &value)?;
    } else {
        serde_json::to_writer(&mut out, &value)?;
    }
    writeln!(&mut out)?;

    Ok(if report.is_fabrication_ready() {
        EXIT_SUCCESS
    } else {
        EXIT_VALIDATION_ERRORS
    })
}

/// Stamped into `[provenance].generator` on every auto-repaired part, so a
/// later reader can tell a machine-written footprint correction from a
/// hand-edited one.
const GENERATOR_FOOTPRINT_REPAIR: &str = "synth-part-resolve-footprint 0.1";

/// Every part on `board` whose `kicad_footprint` does not resolve, with the
/// best candidate the local KiCad libraries offer.
#[derive(Debug, Clone)]
struct FootprintGap {
    part_id: String,
    refdes: String,
    wanted: String,
    candidates: Vec<synth_layout::footprint_resolve::Candidate>,
}

/// Find, report, and where possible repair unresolvable footprint references.
///
/// Returns `Some(())` when at least one correction was written to the Tier-2
/// user registry, meaning the caller must reload the registry and re-lower.
/// Returns `None` when there was nothing to do.
///
/// Never touches pin numbers. See the module docs of
/// `synth_layout::footprint_resolve` for why that line is drawn there.
fn repair_unresolvable_footprints(
    board: Option<&synth_ir::Board>,
    registry: &synth_registry::Registry,
    user_registry: Option<&Path>,
    registry_dir: Option<&Path>,
    file: &str,
) -> Option<()> {
    use synth_layout::footprint_resolve as fr;

    let board = board?;
    let index = fr::FootprintIndex::load();
    if index.is_empty() {
        return None;
    }

    let mut gaps: Vec<FootprintGap> = Vec::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(wanted) = part.kicad_footprint.as_deref() else {
            continue;
        };
        if synth_layout::kicad_footprint_loader::pads(wanted).is_some() {
            continue;
        }
        if gaps.iter().any(|g| g.part_id == part.id.as_str()) {
            continue;
        }
        let pins: Vec<String> = part.pins.iter().map(|p| p.number.0.clone()).collect();
        gaps.push(FootprintGap {
            part_id: part.id.as_str().to_string(),
            refdes: component.refdes.clone(),
            wanted: wanted.to_string(),
            candidates: fr::candidates(&index, wanted, &pins, 3, 0.0),
        });
    }

    if gaps.is_empty() {
        return None;
    }

    // Report every gap, whether or not it can be repaired, so the reason for
    // a `--force` export is always visible.
    for gap in &gaps {
        eprintln!(
            "warning: [E-SYNTH-FP-001] part `{}` ({}) references footprint `{}`, which is not installed",
            gap.part_id, gap.refdes, gap.wanted
        );
        if gap.candidates.is_empty() {
            eprintln!(
                "         no candidate found in {} footprint(s) under {:?}; \
                 import one (`synth part import-lcsc <C-code>`) or author it (`synth part stub`)",
                index.entries.len(),
                fr::search_roots()
            );
            continue;
        }
        for (i, c) in gap.candidates.iter().enumerate() {
            eprintln!("         [{:.2}] {}", c.score, c.lib_id);
            eprintln!("                {}", c.reason);
            if i == 0 {
                break;
            }
        }
    }

    // Repair only the unambiguous ones.
    let mut repaired = false;
    for gap in &gaps {
        let Some(best) = fr::resolve_one(&index, &gap.wanted, &[]) else {
            if gap.candidates.len() > 1 {
                eprintln!(
                    "warning: part `{}` left un-repaired: the top two candidates are within \
                     {:.2} of each other, which is too close to call. Set `kicad_footprint` \
                     on the part by hand.",
                    gap.part_id,
                    fr::AUTO_APPLY_MIN_MARGIN
                );
            }
            continue;
        };
        let Some(part) = registry.lookup(&gap.part_id) else {
            continue;
        };
        let mut fixed = part.clone();
        fixed.kicad_footprint = Some(best.lib_id.clone());
        // A machine-written correction is not a reviewed one. Marking it so
        // keeps `W-SYNTH-PART-UNVERIFIED` firing, which is the last gate
        // before someone sends this to fab.
        fixed.provenance = Some(synth_registry::Provenance {
            source: synth_registry::ProvenanceSource::Generated,
            generator: Some(GENERATOR_FOOTPRINT_REPAIR.to_string()),
            ..Default::default()
        });
        match write_user_part_overlay(&fixed, user_registry, registry_dir) {
            Ok(path) => {
                eprintln!(
                    "         auto-repaired: `{}` -> `{}` (written to {})",
                    gap.wanted,
                    best.lib_id,
                    path.display()
                );
                repaired = true;
            }
            Err(e) => {
                eprintln!(
                    "warning: could not write repair for part `{}` into the user registry: {e}",
                    gap.part_id
                );
            }
        }
    }

    let _ = file;
    repaired.then_some(())
}

/// Write `part` into the Tier-2 user registry as a shadowing overlay.
fn write_user_part_overlay(
    part: &synth_registry::Part,
    user_registry: Option<&Path>,
    _registry_dir: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let dir = match user_registry {
        Some(d) => PathBuf::from(d),
        None => synth_registry::user_registry_dir()
            .ok_or_else(|| anyhow::anyhow!("no user registry directory available"))?,
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.synth.toml", sanitize_part_id(part.id.as_str())));
    let text = synth_registry::part_to_toml(
        part,
        &[
            &format!("Auto-repaired by `{GENERATOR_FOOTPRINT_REPAIR}`."),
            "This file shadows the Tier-1 registry entry of the same id.",
            "`provenance.reviewed_by` is empty, so the part still reports",
            "W-SYNTH-PART-UNVERIFIED: set it once a human has checked the package.",
        ],
    );
    std::fs::write(&path, text)?;
    Ok(path)
}

/// A part id is a filename component; keep it to a safe subset.
fn sanitize_part_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Handler for `synth part resolve-footprint`.
fn part_resolve_footprint(
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
    apply: bool,
    part_filter: Option<&str>,
) -> anyhow::Result<u8> {
    use synth_layout::footprint_resolve as fr;

    let registry = load_registry_strict(registry_dir, user_registry, false)
        .ok_or_else(|| anyhow::anyhow!("registry could not be loaded"))?;
    let index = fr::FootprintIndex::load();
    if index.is_empty() {
        eprintln!(
            "no KiCad footprint libraries found under {:?}",
            fr::search_roots()
        );
        return Ok(1);
    }
    eprintln!(
        "indexed {} footprints from {:?}",
        index.entries.len(),
        fr::search_roots()
    );

    let mut unresolvable = 0usize;
    let mut repaired = 0usize;
    let ids: Vec<String> = registry
        .iter()
        .map(|(_, p)| p.id.as_str().to_string())
        .filter(|id| part_filter.is_none_or(|f| f == id.as_str()))
        .collect();
    for id in ids {
        let Some(part) = registry.lookup(&id) else {
            continue;
        };
        let Some(wanted) = part.kicad_footprint.as_deref() else {
            continue;
        };
        if synth_layout::kicad_footprint_loader::pads(wanted).is_some() {
            continue;
        }
        unresolvable += 1;
        let pins: Vec<String> = part.pins.iter().map(|p| p.number.0.clone()).collect();
        eprintln!("\n{id}: `{wanted}` is not installed");
        let cands = fr::candidates(&index, wanted, &pins, 3, 0.0);
        if cands.is_empty() {
            eprintln!("  no candidate in the local libraries");
            continue;
        }
        for c in &cands {
            eprintln!("  [{:.2}] {}", c.score, c.lib_id);
            eprintln!("         {}", c.reason);
        }
        if !apply {
            continue;
        }
        let Some(best) = fr::resolve_one(&index, wanted, &pins) else {
            eprintln!(
                "  not auto-applied: the top two candidates are within {:.2}, too close to call",
                fr::AUTO_APPLY_MIN_MARGIN
            );
            continue;
        };
        let mut fixed = part.clone();
        fixed.kicad_footprint = Some(best.lib_id.clone());
        fixed.provenance = Some(synth_registry::Provenance {
            source: synth_registry::ProvenanceSource::Generated,
            generator: Some(GENERATOR_FOOTPRINT_REPAIR.to_string()),
            ..Default::default()
        });
        match write_user_part_overlay(&fixed, user_registry, registry_dir) {
            Ok(path) => {
                eprintln!("  applied -> {} ({})", best.lib_id, path.display());
                repaired += 1;
            }
            Err(e) => eprintln!("  could not write overlay: {e}"),
        }
    }
    eprintln!(
        "\n{unresolvable} part(s) reference a footprint that is not installed; {repaired} repaired"
    );
    Ok(u8::from(unresolvable != 0))
}

// strict_registry/validate_erc/force/allow_unverified_parts are four
// independent, non-exclusive CLI toggles (`--strict-registry`,
// `--validate-erc`, `--force`, `--allow-unverified-parts`) — an enum
// would need a variant per combination for no clarity gain.
#[allow(clippy::fn_params_excessive_bools)]
fn export_kicad(
    input: &Path,
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
    strict_registry: bool,
    out_dir: &Path,
    fab: synth_kicad::FabRequest,
    validate_erc: bool,
    verification_report: Option<&Path>,
    force: bool,
    safe: bool,
    allow_unverified_parts: bool,
    autoroute: bool,
    router: ExternalRouter,
    freerouting_jar: Option<&Path>,
    freerouting_java: Option<&Path>,
    kicad_routing_tools_repo: Option<&Path>,
    kicad_routing_tools_python: Option<&Path>,
    krt_escalation: KrtEscalation,
    krt_fab_tier: KrtFabTier,
    krt_fab_overrides: Option<&Path>,
    krt_same_net_pad_clearance: f64,
    krt_allow_via_in_pad: bool,
    best_of: bool,
    allow_incomplete: bool,
) -> anyhow::Result<u8> {
    if autoroute {
        // Still parsed, so an existing script fails with something
        // actionable instead of "unexpected argument".
        anyhow::bail!(
            "--autoroute was removed: Synth no longer routes copper itself. Every export \\
             now runs --router <engine> (default: freerouting) and validates the result \\
             independently. Drop the flag, or pass --router kicad-routing-tools to select \\
             an external checkout."
        );
    }
    // A draft is the one export that is allowed to be un-routed, and an
    // un-routed board must never produce fabrication artifacts. Check the
    // combination here, before any file is written: `run_fab` runs before the
    // final validation gate, so otherwise `--allow-incomplete --gerbers`
    // would emit Gerbers from an unrouted board and only then fail. `--force`
    // is not a way around this either — fabrication artifacts from a draft are
    // never release evidence.
    if allow_incomplete && !fab.is_empty() {
        anyhow::bail!(
            "--allow-incomplete cannot be combined with fabrication artifacts \
             (--gerbers / --drill / --step): a draft export is never fabricable. \
             Export the draft first, route and validate it, then run the export \
             again without --allow-incomplete to produce the manufacturing package."
        );
    }

    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let ast = parse
        .ast
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("parse failed; not exporting"))?;
    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    write_diagnostics_to_stderr(&resolved.diagnostics)?;
    let registry = load_registry_strict(registry_dir, user_registry, strict_registry)
        .ok_or_else(|| anyhow::anyhow!("registry could not be loaded"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    write_diagnostics_to_stderr(&lowered.diagnostics)?;

    // Automatic footprint repair (§R15.9). A part whose `kicad_footprint`
    // names a footprint that is not installed is almost always a name the
    // author mistyped rather than a part KiCad does not ship — and the right
    // one is normally already in the local library. When the match is both
    // confident and unambiguous, the correction is written into the Tier-2
    // user registry (the documented home for machine-generated overrides)
    // and the design is re-lowered against it, so everything downstream
    // sees the fix.
    //
    // Deliberately not done for ambiguous matches, and deliberately never
    // for pin numbers: a wrong footprint is recoverable because a footprint
    // either exists or does not, whereas an invented pin designator produces
    // a board that routes and passes every check while being unbuildable.
    let lowered = match repair_unresolvable_footprints(
        lowered.board.as_ref(),
        &registry,
        user_registry,
        registry_dir,
        &file,
    ) {
        Some(()) => {
            // Re-read the registry so the freshly written Tier-2 overlay
            // shadows the entry the repair just corrected, then re-lower so
            // every later stage sees the fixed footprint.
            let repaired_registry =
                load_registry_strict(registry_dir, user_registry, strict_registry).ok_or_else(
                    || anyhow::anyhow!("registry could not be reloaded after footprint repair"),
                )?;
            let lowered = synth_ir::lower(&resolved.program, &repaired_registry, &file);
            write_diagnostics_to_stderr(&lowered.diagnostics)?;
            lowered
        }
        None => lowered,
    };
    let board = lowered
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lowering produced no IR; not exporting"))?;

    let mut overrides: Vec<release::OverrideRecord> = Vec::new();

    // Slice 7: Run Synth ERC validation before exporting
    let erc_diags = synth_validate::run_erc(board, &file);
    write_diagnostics_to_stderr(&erc_diags)?;
    let has_erc_errors = erc_diags.iter().any(|d| d.severity.is_blocking());
    if has_erc_errors {
        if !force {
            anyhow::bail!("Synth ERC validation failed; use --force to export anyway");
        }
        let blocking = erc_diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .count();
        overrides.push(release::OverrideRecord::new(
            release::OverrideKind::ErcErrors,
            Vec::new(),
            format!("{blocking} blocking ERC diagnostic(s) suppressed by --force"),
        ));
    }

    // R15.9 footprint guarantee: every part on the board must resolve to a
    // real `.kicad_mod` footprint (imported via LCSC/EasyEDA, referenced from
    // stock KiCad, or shipped in the registry). Parts without one export with
    // a synthesized bounding-box fallback whose pad geometry is a guess —
    // not fabricable. Reject unless `--force`.
    let unfootprinted = parts_without_real_footprint(board);
    if !unfootprinted.is_empty() {
        let list = unfootprinted.join(", ");
        if force {
            eprintln!(
                "error: {banner} [E-SYNTH-EXPORT-001] exporting with synthesized \
                 bounding-box footprints for: {list} (--force). Pad geometry is a guess, \
                 not the manufacturer's land pattern.",
                banner = release::UNTRUSTED_BANNER
            );
            overrides.push(release::OverrideRecord::new(
                release::OverrideKind::BoundingBoxFootprints,
                unfootprinted.clone(),
                format!(
                    "{} part(s) exported with a synthesized land pattern",
                    unfootprinted.len()
                ),
            ));
        } else {
            eprintln!(
                "error: [E-SYNTH-EXPORT-001] the following parts have no real `.kicad_mod` \
                 footprint and would export with a synthesized bounding-box fallback: {list}. \
                 Import real footprints (`synth part import lcsc` / `synth part import kicad`), \
                 ship them in the registry, or re-run with --force to export anyway."
            );
            return Ok(1);
        }
    }

    // Honour manual tuning: the schematic sidecar beside the source
    // (`<design>.schematic.layout.toml`, sheet millimetres) drives the
    // exported sheet. Resolved before the aesthetic ERC below so both
    // describe the same drawing.
    let sidecars = synth_kicad::Sidecars::resolve_for(input);
    let sidecar = sidecars.schematic.as_deref();

    // Aesthetic schematic ERC (E-SYNTH-SCHEM-*): advisory warnings
    // printed alongside the rule-based ERC; never blocks export.
    // Thresholds come from the design's `<design>.synth.erc.toml`
    // `[schematic]` section when present (Phase E), else the defaults.
    let pre_layout = synth_layout::layout_with_sidecar(board, sidecar);
    let pre_sheets = synth_layout::sheets::layout_sheets(board, pre_layout.clone());
    let mut schem_diags = synth_kicad::check_schem_erc_sheets_with_config(
        board,
        &pre_sheets,
        schem_erc_config_for(input),
    );
    synth_kicad::attach_schem_erc_locations(&mut schem_diags, board, &input.display().to_string());
    write_diagnostics_to_stderr(&schem_diags)?;

    if let Some(notice) = synth_layout::SidecarKind::Schematic.migration_notice(input) {
        eprintln!("warning: {notice}");
    }

    let result = synth_kicad::export_with_sidecars(board, out_dir, &sidecars)
        .map_err(|e| anyhow::anyhow!("kicad export failed: {e}"))?;

    // Copper is generated by the external router, never by Synth. The
    // export above is the un-routed baseline; this run reads it, routes it,
    // and installs the result only if it passes independent validation.
    let routing = route_externally(
        board,
        &result,
        input,
        router,
        &router_options(
            router,
            freerouting_jar,
            freerouting_java,
            kicad_routing_tools_repo,
            kicad_routing_tools_python,
            krt_escalation,
            krt_fab_tier,
            krt_fab_overrides,
            krt_same_net_pad_clearance,
            krt_allow_via_in_pad,
        ),
        best_of,
    )?;

    eprintln!("wrote {}", result.project_path.display());
    eprintln!("wrote {}", result.schematic_path.display());
    eprintln!("wrote {}", result.library_path.display());
    eprintln!("wrote {}", result.pcb_path.display());
    eprintln!("wrote {}", result.bom_path.display());

    let mut fab_evidence = None;
    if !fab.is_empty() {
        // R15.3 trust boundary: a fab submission (gerbers/drill/step) must
        // not silently include parts nobody has reviewed. Interactive
        // preview/validation stay non-blocking (`W-SYNTH-PART-UNVERIFIED`
        // is a warning there); manufacturing export is where it becomes a
        // hard gate, per §18.8.2.
        if let Some(refusal) = structural_refusal(board) {
            eprintln!("{refusal}");
            return Ok(1);
        }
        let unverified = parts_unverified(board);
        if !unverified.is_empty() {
            let list = unverified.join(", ");
            if !allow_unverified_parts {
                eprintln!(
                    "error: [W-SYNTH-PART-UNVERIFIED] refusing fab export: the following parts \
                     have no reviewer (`[provenance].reviewed_by` empty): {list}. Review them \
                     and set `reviewed_by`, or re-run with --allow-unverified-parts to submit \
                     anyway."
                );
                return Ok(1);
            }
            eprintln!(
                "error: {banner} [W-SYNTH-PART-UNVERIFIED] fab export includes parts no one \
                 has reviewed: {list} (--allow-unverified-parts).",
                banner = release::UNTRUSTED_BANNER
            );
            overrides.push(release::OverrideRecord::new(
                release::OverrideKind::UnverifiedParts,
                unverified.clone(),
                format!("{} part(s) have no reviewer", unverified.len()),
            ));
        }
        match synth_kicad::run_fab(&result.pcb_path, out_dir, &fab) {
            Ok(artifacts) => {
                if let Some(dir) = artifacts.gerbers_dir {
                    eprintln!("wrote gerbers into {}", dir.display());
                }
                if let Some(dir) = artifacts.drill_dir {
                    eprintln!("wrote drill files into {}", dir.display());
                }
                if let Some(path) = artifacts.step_path {
                    eprintln!("wrote {}", path.display());
                }
                fab_evidence = Some(synth_diagnostics::NativeCheckEvidence::concluded(
                    FAB_STAGE,
                    synth_drc::kicad_cli::binary(),
                    Vec::new(),
                    0,
                ));
            }
            Err(e) => {
                let mut evidence = synth_diagnostics::NativeCheckEvidence::unknown(
                    FAB_STAGE,
                    synth_drc::kicad_cli::binary(),
                    Vec::new(),
                    e.unknown_reason(),
                    format!("kicad-cli fab export failed: {e}"),
                )
                .with_version(synth_drc::kicad_cli::version());
                if let Some(stderr) = e.stderr() {
                    evidence = evidence.with_stderr(stderr);
                }
                fab_evidence = Some(evidence);
            }
        }
    }

    let production = !fab.is_empty();
    let mut native = Vec::new();
    if let Some(evidence) = fab_evidence {
        if !evidence.is_trusted() {
            report_unavailable_check(&evidence);
        }
        native.push(evidence);
    }

    // Slice 3: Run KiCad ERC if requested
    let mut has_kicad_erc_errs = false;
    if validate_erc {
        eprintln!(
            "running kicad-cli sch erc on {}",
            result.schematic_path.display()
        );
        let erc = synth_kicad::run_kicad_erc(&result.schematic_path);
        if erc.evidence.status == NativeCheckStatus::Unknown {
            report_unavailable_check(&erc.evidence);
        } else {
            if erc.violations.is_empty() {
                eprintln!("kicad-cli sch erc: 0 violations found");
            } else {
                for v in &erc.violations {
                    eprintln!(
                        "[kicad-erc] {}: [{}] {}",
                        v.severity, v.violation_type, v.description
                    );
                }
            }
            has_kicad_erc_errs = erc.errors().next().is_some();
        }
        native.push(erc.evidence);
    }

    // KiCad DRC over the delivered board.
    //
    // Reuse the run the routing pipeline already performed rather than
    // invoking KiCad a second time: two runs over the same board can
    // disagree if the board changed in between, and the one that actually
    // gated installation is the one whose result matters. When routing did
    // not produce a DRC result at all, the check is run here so the gate
    // still has evidence.
    let mut has_kicad_drc_errors = false;
    let drc = match routing.validation.as_ref().and_then(|v| v.kicad_drc) {
        Some(counts) => {
            let performed = routing
                .validation
                .as_ref()
                .is_some_and(|v| !v.unavailable_checks.iter().any(|c| c.contains("drc")));
            synth_drc::replay(&counts.into(), performed)
        }
        None => synth_drc::run_kicad_cli_drc(&result.pcb_path),
    };
    if drc.evidence.status == NativeCheckStatus::Unknown {
        report_unavailable_check(&drc.evidence);
    } else {
        for v in &drc.violations {
            eprintln!("[kicad-drc] error: [{}] {}", v.code, v.message);
        }
        has_kicad_drc_errors = !drc.violations.is_empty();
        report_drc_counts(&drc.counts);
    }
    let release_drc_blocked = production && drc.counts.blocking_count() > 0;
    if release_drc_blocked {
        eprintln!(
            "error: release export blocked: KiCad DRC is not clean ({}); \
             --force does not override this.",
            drc.counts
        );
    }
    native.push(drc.evidence);

    let blocked_by_unknown: Vec<_> = native
        .iter()
        .filter(|e| e.status == NativeCheckStatus::Unknown)
        .filter(|e| production || e.stage == synth_kicad::ERC_STAGE)
        .collect();
    if !blocked_by_unknown.is_empty() {
        eprintln!();
        if production {
            eprintln!(
                "error: UNTRUSTED / NOT FOR FABRICATION — this export has no native \
                 verification evidence:"
            );
        } else {
            eprintln!("error: requested native verification could not be performed:");
        }
        for evidence in &blocked_by_unknown {
            eprintln!("  {}", evidence.summary_line());
        }
        eprintln!(
            "An unavailable check is not a pass. Install KiCad (or set KICAD_CLI), \
             then re-run; --force does not override this."
        );
    }

    if let Some(path) = verification_report {
        write_verification_report(path, input, &native)?;
    }

    if has_kicad_drc_errors && force && !production {
        overrides.push(release::OverrideRecord::new(
            release::OverrideKind::NativeDrcErrors,
            Vec::new(),
            "KiCad PCB DRC violations suppressed by --force".to_string(),
        ));
    }

    // The routing run is part of the package's identity: which engine
    // generated the copper, at what version, and whether anyone checked it.
    // An unvalidated route downgrades the manifest for the same reason an
    // override does.
    let run_record = request_run_record_path(board, &result);
    let manifest = release::ReleaseManifest::new(
        &format!("synth-cli {}", env!("CARGO_PKG_VERSION")),
        input,
        !fab.is_empty(),
        overrides,
        release::reviewer_state(board),
    )
    .with_release_blocked(release_drc_blocked)
    .with_routing(Some(release::RoutingState::from_report(
        &routing,
        &run_record,
    )));
    let manifest_path = manifest
        .write_to(out_dir)
        .map_err(|e| anyhow::anyhow!("could not write the release manifest: {e}"))?;
    eprintln!("wrote {}", manifest_path.display());
    if let Some(banner) = manifest.banner() {
        eprintln!();
        eprintln!("{banner}");
    }

    // --safe conflicts with the override flags at parse time; this is the
    // second half of the promise. Safe mode asserts the package it produced
    // is actually clean, so a path that records an override without a flag
    // behind it cannot slip through a release gate either.
    if safe && manifest.has_overrides() {
        eprintln!(
            "error: --safe was requested but the export recorded {} override(s); see {}",
            manifest.overrides.len(),
            manifest_path.display()
        );
        return Ok(EXIT_VALIDATION_ERRORS);
    }

    // A draft export is explicitly allowed to be un-routed: it exists so an
    // external router can complete it, or so a human can review it. Say so
    // loudly, because the artifacts it writes are otherwise indistinguishable
    // from a release.
    if allow_incomplete && !routing.is_fabrication_ready() {
        eprintln!(
            "warning: DRAFT / NOT FOR FABRICATION — exporting an un-routed board: {}",
            routing.summary()
        );
        eprintln!(
            "  the un-routed baseline is preserved beside the export for review or \
             for an external router to complete; the routing run record names it."
        );
    }

    let has_errors = parse.has_errors()
        || lowered.has_errors()
        || (has_erc_errors && !force)
        || (has_kicad_drc_errors && !force)
        || release_drc_blocked
        || has_kicad_erc_errs
        || !blocked_by_unknown.is_empty()
        // Fail-closed: an un-routed export is not fabricable, and neither
        // is a board whose router could not run or whose copper failed
        // independent validation. `--force` deliberately does not override
        // this, for the same reason it does not override a KiCad DRC error.
        // `--allow-incomplete` is the one deliberate exception: it produces
        // a draft, and never a release-ready package.
        || (!routing.is_fabrication_ready() && !allow_incomplete);
    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

const FAB_STAGE: &str = "kicad_fab";

fn report_drc_counts(counts: &synth_drc::DrcCounts) {
    let line = format!("kicad-cli pcb drc: {counts}");
    if counts.is_clean() {
        eprintln!("{line} (Phase 13 Zero-DRC gate clean)");
    } else {
        eprintln!("{line}");
        eprintln!(
            "  note: unconnected pads belong to a net but have no copper path yet; \
             warnings are advisory, not rule violations"
        );
    }
}

fn report_unavailable_check(evidence: &synth_diagnostics::NativeCheckEvidence) {
    eprintln!("warning: {}", evidence.summary_line());
    eprintln!("  tool: {}", evidence.tool);
    if let Some(version) = &evidence.tool_version {
        eprintln!("  version: {version}");
    }
    if !evidence.command.is_empty() {
        eprintln!("  command: {}", evidence.command.join(" "));
    }
    if let Some(detail) = &evidence.detail {
        eprintln!("  detail: {detail}");
    }
    if let Some(stderr) = &evidence.stderr {
        eprintln!("  stderr: {}", stderr.replace('\n', "\n          "));
    }
}

fn write_verification_report(
    path: &Path,
    input: &Path,
    native: &[synth_diagnostics::NativeCheckEvidence],
) -> anyhow::Result<()> {
    let release_ready = native
        .iter()
        .all(synth_diagnostics::NativeCheckEvidence::is_trusted);
    let report = serde_json::json!({
        "schema_version": "synth.verification.v1",
        "input": input.display().to_string(),
        "release_ready": release_ready,
        "stages": native,
    });
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

/// Everything the CLI can configure about a routing run.
///
/// Collected in one place so the CLI's flags, the MCP tool arguments, and
/// the adapters read the same settings, and so the settings that reach the
/// report are exactly the ones the run record claims were applied.
#[derive(Debug, Clone, PartialEq)]
struct RouterOptions {
    freerouting_jar: Option<PathBuf>,
    freerouting_java: Option<PathBuf>,
    kicad_routing_tools_repo: Option<PathBuf>,
    kicad_routing_tools_python: Option<PathBuf>,
    escalation: KrtEscalation,
    fab_tier: KrtFabTier,
    fab_overrides: Option<PathBuf>,
    same_net_pad_clearance: f64,
    allow_via_in_pad: bool,
    /// Wall-clock budget for the whole external run.
    timeout_secs: u64,
}

impl RouterOptions {
    /// Apply an explicit wall-clock override.
    ///
    /// A zero or absent flag keeps the default rather than meaning "no
    /// budget", which would kill every run instantly.
    fn with_timeout(mut self, secs: Option<u64>) -> Self {
        if let Some(secs) = secs.filter(|s| *s > 0) {
            self.timeout_secs = secs;
        }
        self
    }
}

#[allow(clippy::fn_params_excessive_bools)]
fn router_options(
    _router: ExternalRouter,
    freerouting_jar: Option<&Path>,
    freerouting_java: Option<&Path>,
    kicad_routing_tools_repo: Option<&Path>,
    kicad_routing_tools_python: Option<&Path>,
    escalation: KrtEscalation,
    fab_tier: KrtFabTier,
    fab_overrides: Option<&Path>,
    same_net_pad_clearance: f64,
    allow_via_in_pad: bool,
) -> RouterOptions {
    RouterOptions {
        freerouting_jar: freerouting_jar.map(Path::to_path_buf),
        freerouting_java: freerouting_java.map(Path::to_path_buf),
        kicad_routing_tools_repo: kicad_routing_tools_repo.map(Path::to_path_buf),
        kicad_routing_tools_python: kicad_routing_tools_python.map(Path::to_path_buf),
        escalation,
        fab_tier,
        fab_overrides: fab_overrides.map(Path::to_path_buf),
        same_net_pad_clearance,
        allow_via_in_pad,
        timeout_secs: default_router_timeout_secs(),
    }
}

fn default_router_timeout_secs() -> u64 {
    std::env::var("SYNTH_ROUTER_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or_else(|| synth_router::RouterLimits::default().wall_clock.as_secs())
}

/// Build the adapter request for a board that has just been exported.
///
/// The exported PCB is preserved as the baseline before the router is
/// started, so a failed or partial run still leaves a reviewable board
/// behind rather than nothing.
fn build_route_request(
    board: &synth_ir::Board,
    result: &synth_kicad::ExportResult,
    engine: synth_router::RouterEngine,
    options: &RouterOptions,
) -> anyhow::Result<synth_router::RouteRequest> {
    let stem = result
        .pcb_path
        .file_stem()
        .map_or_else(|| "board".to_string(), |s| s.to_string_lossy().into_owned());
    let mut request = synth_router::RouteRequest::new(&result.pcb_path, &result.out_dir, engine);
    request.stem = stem;
    request.limits = synth_router::RouterLimits::default().with_wall_clock(options.timeout_secs);
    request.kicad_routing_tools.escalation = synth_router::KrtEscalation::from(options.escalation);
    request.kicad_routing_tools.fab_tier = options.fab_tier.as_str().to_string();
    request.kicad_routing_tools.same_net_pad_clearance_mm = options.same_net_pad_clearance;
    request.policy.allow_via_in_pad = options.allow_via_in_pad;
    request.use_profile_floor(
        &board
            .manufacturer
            .clone()
            .unwrap_or_else(|| "jlc-standard".to_string()),
    );
    request.input_hash = sha256_file(&result.pcb_path)?;
    request
        .kicad_routing_tools
        .fab_overrides
        .clone_from(&options.fab_overrides);
    // Environment-only configuration reaches the adapters through the
    // environment they already read; explicit flags win because they are
    // exported here before discovery runs.
    if let Some(jar) = &options.freerouting_jar {
        std::env::set_var("SYNTH_FREEROUTING_JAR", jar);
    }
    if let Some(java) = &options.freerouting_java {
        std::env::set_var("SYNTH_FREEROUTING_JAVA", java);
    }
    if let Some(repo) = &options.kicad_routing_tools_repo {
        std::env::set_var("KICAD_ROUTING_TOOLS_REPO", repo);
    }
    if let Some(python) = &options.kicad_routing_tools_python {
        std::env::set_var("SYNTH_KRT_PYTHON", python);
    }
    request.preserve_baseline()?;
    Ok(request)
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

/// Hex-encode a SHA-256 digest. `Sha256::digest` returns a byte array that does
/// not implement `LowerHex`, so the hex has to be written byte by byte.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Run the external router and report the outcome to the operator.
///
/// The returned report is the same document every other surface reads, so
/// the router identity and terminal state the CLI prints cannot drift from
/// what the run record says.
fn route_externally(
    board: &synth_ir::Board,
    result: &synth_kicad::ExportResult,
    input: &Path,
    engine: ExternalRouter,
    options: &RouterOptions,
    best_of: bool,
) -> anyhow::Result<synth_router::RouteReport> {
    let mut request = build_route_request(board, result, engine.into(), options)?;
    eprintln!(
        "routing {} with {}",
        request.router_input_path().display(),
        if best_of {
            "every installed engine"
        } else {
            engine.as_str()
        }
    );
    // A prior attempt on this exact board may have left advice worth starting
    // from; say so before the run rather than only in the log afterwards.
    let outcomes_dir = synth_router::outcomes::configured_dir();
    if let Some(dir) = &outcomes_dir {
        if let Some(order) = synth_router::outcomes::best_known_order(dir, &request.input_hash) {
            eprintln!(
                "  known-good retry order from previous runs: {}",
                order.join(", ")
            );
        }
    }
    let report = if best_of {
        let base_out = request.out_dir.clone();
        let portfolio = synth_router::route_best_of(&request, &synth_router::RouterEngine::all());
        for attempt in &portfolio.attempts {
            eprintln!("  attempt: {}", attempt.summary());
        }
        eprintln!("  winner: {}", portfolio.winner.as_str());
        // The winner's artifacts live in its own subdirectory; point the
        // request there so the reports below reference the board that won.
        request.engine = portfolio.winner;
        request.out_dir = base_out.join(portfolio.winner.artifact_tag());
        portfolio.report().clone()
    } else {
        synth_router::route(&request)
    };
    print_route_report(&report);

    if let Some(dir) = &outcomes_dir {
        let record = synth_router::outcomes::OutcomeRecord::from_report(&report);
        let _ = synth_router::outcomes::append(dir, &record);
    }

    if let Some(validation) = &report.validation {
        let _ = synth_router::write_artifact(&request.connectivity_report_path(), validation);
    }

    // Fabricating without a validated route is exactly the outcome the
    // gate exists to prevent, so say why rather than emitting files that
    // look deliverable.
    if !report.is_fabrication_ready() {
        if report.state == synth_router::RouteState::RouterUnavailable {
            eprintln!(
                "error: {} is not available; the exported board at {} is the \
                 un-routed baseline and is preserved for review.",
                engine.as_str(),
                request.baseline_path().display()
            );
        } else if report.state == synth_router::RouteState::ValidationFailed {
            eprintln!(
                "error: the routed board failed independent validation; see {}.",
                request.report_path().display()
            );
        } else {
            eprintln!(
                "warning: the routed board is review-only — a required check could \
                 not be performed, which is not the same as a clean result. See {}.",
                request.report_path().display()
            );
        }
        eprintln!("(design: {})", input.display());
    }
    Ok(report)
}

fn print_route_report(report: &synth_router::RouteReport) {
    eprintln!("{}", report.summary());
    if let Some(provenance) = Some(&report.provenance) {
        if let Some(version) = &provenance.engine_version {
            eprintln!("  router version: {version}");
        }
        if let Some(runtime) = &provenance.runtime_version {
            eprintln!("  runtime: {runtime}");
        }
        eprintln!("  duration: {} ms", provenance.duration_ms);
    }
    for reason in report
        .validation
        .as_ref()
        .map(|v| v.blocking_reasons.clone())
        .unwrap_or_default()
    {
        eprintln!("  blocking: {reason}");
    }
    for check in report
        .validation
        .as_ref()
        .map(|v| v.unavailable_checks.clone())
        .unwrap_or_default()
    {
        eprintln!("  not checked: {check}");
    }
    if let Some(counts) = report.validation.as_ref().and_then(|v| v.kicad_drc) {
        eprintln!(
            "  kicad-cli pcb drc: {} error(s), {} unconnected, {} warning(s)",
            counts.errors, counts.unconnected, counts.warnings
        );
    }
    // Reported before the router is judged, because it explains a failure the
    // router cannot avoid: a fine-pitch package that cannot take a fab-floor
    // via will produce sub-floor vias however well it is routed.
    for escape in &report.escape {
        eprintln!("  escape: {}", escape.summary_line());
        eprintln!("    fix: {}", escape.remediation);
    }
    if !report.recommended_routing_order.is_empty() {
        eprintln!(
            "  retry order: {}  (pass as routing_order to reserve routes for the hardest nets first)",
            report.recommended_routing_order.join(", ")
        );
    }
}

/// Where the run record for a given export lives.
///
/// Derived from the same stem the adapter used, so the manifest points at
/// the file the router actually wrote rather than at a path recomputed
/// here and liable to drift.
fn request_run_record_path(board: &synth_ir::Board, result: &synth_kicad::ExportResult) -> PathBuf {
    result.out_dir.join(format!(
        "{}.routing.json",
        synth_router::sanitize_stem(&board.name)
    ))
}

/// Print the availability of every supported routing engine.
fn dump_routers(pretty: bool) -> anyhow::Result<u8> {
    let out_dir = std::env::temp_dir().join(format!("synth-routers-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let capabilities = synth_router::capability::discover_all_in_out_dir(&out_dir);
    let _ = std::fs::remove_dir_all(&out_dir);

    if pretty {
        eprintln!("{}", serde_json::to_string_pretty(&capabilities)?);
    } else {
        eprintln!("{}", serde_json::to_string(&capabilities)?);
    }
    for capability in &capabilities {
        match (&capability.version, capability.available) {
            (Some(version), true) => eprintln!(
                "{}: available ({} on {})",
                capability.engine,
                version,
                capability
                    .runtime_version
                    .as_deref()
                    .unwrap_or("unknown runtime")
            ),
            (_, true) => eprintln!("{}: available (version unrecorded)", capability.engine),
            (_, false) => {
                eprintln!("{}: unavailable", capability.engine);
                if let Some(detail) = &capability.detail {
                    eprintln!("  {detail}");
                }
                if let Some(failure) = capability.failure() {
                    eprintln!("  fix: {}", failure.remediation);
                }
            }
        }
    }
    Ok(EXIT_SUCCESS)
}

fn collect_diagnostics(
    input: &Path,
    registry_dir: Option<&Path>,
) -> anyhow::Result<(String, Vec<synth_diagnostics::Diagnostic>)> {
    let source = std::fs::read_to_string(input)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", input.display()))?;
    let file = input.to_string_lossy().into_owned();
    let parse = synth_parser::parse(&source, file.clone());
    let mut diagnostics = parse.diagnostics;
    if let Some(ast) = parse.ast.as_ref() {
        let import_root = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let loader = synth_ir::FsImportLoader { root: import_root };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        diagnostics.extend(resolved.diagnostics);
        if let Some(registry) = load_registry(registry_dir) {
            let lowered = synth_ir::lower(&resolved.program, &registry, &file);
            diagnostics.extend(lowered.diagnostics);
            if let Some(board) = lowered.board.as_ref() {
                diagnostics.extend(synth_validate::run_erc(board, &file));
            }
        }
    }
    Ok((source, diagnostics))
}

/// Apply every diagnostic's top-confidence patch to `input`. Patches
/// are applied in reverse byte order so earlier patches do not shift
/// later byte offsets. Returns exit code 0 on a successful run
/// regardless of how many patches were applied — agents call this in
/// a loop until `validate` is clean.
fn fix(input: &Path, registry_dir: Option<&Path>, smt: bool, dry_run: bool) -> anyhow::Result<u8> {
    let (source, diagnostics) = collect_diagnostics(input, registry_dir)?;

    let mut diag_patches: Vec<(&synth_diagnostics::Diagnostic, synth_diagnostics::Patch)> =
        diagnostics
            .iter()
            .filter_map(|d| {
                if smt {
                    if let Some(smt_patch) = d
                        .suggested_fixes
                        .iter()
                        .find(|p| matches!(p.kind, synth_diagnostics::PatchKind::SolveSmt { .. }))
                    {
                        return Some((d, smt_patch.clone()));
                    }
                }
                d.suggested_fixes.first().cloned().map(|p| (d, p))
            })
            .collect();

    if diag_patches.is_empty() {
        eprintln!(
            "synth fix: no machine-applicable patches in {} diagnostics",
            diagnostics.len()
        );
        if dry_run {
            print!("{source}");
        }
        return Ok(EXIT_SUCCESS);
    }

    // Reverse byte order so applying earlier patches doesn't shift
    // later byte offsets.
    diag_patches.sort_by_key(|(d, p)| std::cmp::Reverse(diag_patch_anchor_offset(d, p)));

    let mut current = source.clone();
    let mut applied = 0_usize;
    let mut skipped = 0_usize;
    for (_diag, patch) in &diag_patches {
        let res = match &patch.kind {
            synth_diagnostics::PatchKind::SolveSmt { constraint, .. } => {
                if smt {
                    patch.apply(&current)
                } else {
                    eprintln!("synth fix: skipping SMT patch ({constraint}); pass --smt to solve");
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
                applied += 1;
            }
            Err(e) => {
                eprintln!("synth fix: skipping patch ({e})");
                skipped += 1;
            }
        }
    }

    eprintln!(
        "synth fix: applied {applied} / {total} patch(es), skipped {skipped}",
        total = diag_patches.len(),
    );

    if dry_run {
        print!("{current}");
    } else if current != source {
        std::fs::write(input, &current)
            .map_err(|e| anyhow::anyhow!("could not write {}: {e}", input.display()))?;
        eprintln!("synth fix: wrote {}", input.display());
    }
    Ok(EXIT_SUCCESS)
}

/// Byte offset a patch is anchored to, for stable application order.
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

/// Emit the JSON Schema for the diagnostic protocol to stdout.
fn schema(kind: SchemaKind) -> anyhow::Result<u8> {
    let schema = match kind {
        SchemaKind::Diagnostic => synth_diagnostics::diagnostic_schema(),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer_pretty(&mut out, &schema)?;
    writeln!(&mut out)?;
    Ok(EXIT_SUCCESS)
}

fn write_diagnostics_to_stderr(
    diagnostics: &[synth_diagnostics::Diagnostic],
) -> anyhow::Result<()> {
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    for d in diagnostics {
        let where_ = d.location.as_ref().map_or_else(
            || "?".into(),
            |l| format!("{}:{}-{}", l.file, l.span.byte_start, l.span.byte_end),
        );
        writeln!(
            &mut err,
            "{}: [{}] {} ({})",
            d.severity, d.code, d.title, where_
        )?;
    }
    Ok(())
}

fn supply_chain(input: &Path, registry_dir: Option<&Path>, format: Format) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse
        .ast
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("parse failed; cannot query supply chain"))?;
    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let registry = load_registry(registry_dir)
        .ok_or_else(|| anyhow::anyhow!("registry could not be loaded"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    let board = lowered
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lowering produced no IR; cannot query supply chain"))?;

    let engine = synth_supply::SupplyEngine::new()
        .map_err(|e| anyhow::anyhow!("could not initialize supply engine: {e}"))?;

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

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let bom_map = rt
        .block_on(engine.query_bom(&bom_entries))
        .map_err(|e| anyhow::anyhow!("supply chain query failed: {e}"))?;

    match format {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(&bom_map)?);
        }
        Format::Human => {
            println!(
                "{:<6} {:<18} {:<10} {:<10} {:<8} {:<6} {:<10} {:<10}",
                "REF", "PART_ID", "PN", "STOCK", "QTY", "MOQ", "UNIT_PRICE", "LIFECYCLE"
            );
            println!("{}", "-".repeat(84));
            for (ref_des, statuses) in &bom_map {
                let inst = board.components.iter().find(|c| c.refdes == *ref_des);
                let part_id = inst
                    .and_then(|c| c.part.as_ref())
                    .map_or("?", |p| p.id.as_str());
                let default_pn = inst
                    .and_then(|c| c.part.as_ref())
                    .and_then(|p| p.lcsc_pn.as_deref().or(p.mpn.as_deref()))
                    .unwrap_or("-");

                if statuses.is_empty() {
                    println!(
                        "{:<6} {:<18} {:<10} {:<10} {:<8} {:<6} {:<10} {:<10}",
                        ref_des, part_id, default_pn, "UNKNOWN", "-", "-", "-", "Unknown"
                    );
                } else {
                    for s in statuses {
                        let stock_str = if s.in_stock { "YES" } else { "NO" };
                        let price_str = s
                            .unit_price_usd
                            .map_or("-".to_string(), |p| format!("${p:.4}"));
                        let lifecycle_str = format!("{:?}", s.lifecycle);
                        println!(
                            "{:<6} {:<18} {:<10} {:<10} {:<8} {:<6} {:<10} {:<10}",
                            ref_des,
                            part_id,
                            s.part_number,
                            stock_str,
                            s.stock_qty,
                            s.moq,
                            price_str,
                            lifecycle_str
                        );
                    }
                }
            }
        }
    }

    Ok(EXIT_SUCCESS)
}

fn render(
    input: &Path,
    out_path: &Path,
    quality: RenderQuality,
    side: &str,
    width: u32,
    height: u32,
    registry_dir: Option<&Path>,
    svg: bool,
) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());
    write_diagnostics_to_stderr(&parse.diagnostics)?;

    let ast = parse
        .ast
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("parse failed; not rendering"))?;
    let import_root = input
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let loader = synth_ir::FsImportLoader { root: import_root };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    write_diagnostics_to_stderr(&resolved.diagnostics)?;
    let registry = load_registry(registry_dir)
        .ok_or_else(|| anyhow::anyhow!("registry could not be loaded"))?;
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    write_diagnostics_to_stderr(&lowered.diagnostics)?;
    let board = lowered
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lowering produced no IR; not rendering"))?;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let tmp_dir =
        std::env::temp_dir().join(format!("synth_render_{}_{}", std::process::id(), nonce));

    let export_res = synth_kicad::export(board, &tmp_dir)
        .map_err(|e| anyhow::anyhow!("kicad export for rendering failed: {e}"))?;

    let kicad_cli = std::env::var("KICAD_CLI").unwrap_or_else(|_| "kicad-cli".into());

    let out_str = out_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid output path"))?;
    let pcb_str = export_res
        .pcb_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid pcb path"))?;

    let status = std::process::Command::new(&kicad_cli)
        .args([
            "pcb",
            "render",
            pcb_str,
            "--output",
            out_str,
            "--quality",
            quality.as_str(),
            "--side",
            side,
            "--width",
            &width.to_string(),
            "--height",
            &height.to_string(),
        ])
        .status()
        .map_err(|e| anyhow::anyhow!("failed to execute {kicad_cli}: {e}"))?;

    if !status.success() {
        std::fs::remove_dir_all(&tmp_dir).ok();
        anyhow::bail!("kicad-cli pcb render failed with status: {status}");
    }

    eprintln!("rendered 3D PCB image -> {}", out_path.display());

    if svg {
        let svg_out = out_path.with_extension("svg");
        let svg_str = svg_out
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid svg path"))?;
        let svg_status = std::process::Command::new(&kicad_cli)
            .args([
                "pcb",
                "export",
                "svg",
                pcb_str,
                "--output",
                svg_str,
                "--layers",
                "F.Cu,B.Cu,F.Silkscreen,Edge.Cuts",
            ])
            .status()
            .map_err(|e| anyhow::anyhow!("failed to execute {kicad_cli} for svg: {e}"))?;

        if svg_status.success() {
            eprintln!("rendered 2D copper SVG -> {}", svg_out.display());
        } else {
            eprintln!("warning: kicad-cli pcb export svg returned failure: {svg_status}");
        }
    }

    std::fs::remove_dir_all(&tmp_dir).ok();
    Ok(EXIT_SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_kicad_electrical_types() {
        assert_eq!(map_kicad_electrical_type("power_in"), "power_input");
        assert_eq!(map_kicad_electrical_type("power_out"), "power_output");
        assert_eq!(map_kicad_electrical_type("input"), "input");
        assert_eq!(map_kicad_electrical_type("output"), "output");
        assert_eq!(map_kicad_electrical_type("bidirectional"), "bidirectional");
        assert_eq!(map_kicad_electrical_type("passive"), "passive");
        assert_eq!(map_kicad_electrical_type("tri_state"), "three_statable");
        assert_eq!(
            map_kicad_electrical_type("open_collector"),
            "open_drain_low"
        );
        assert_eq!(map_kicad_electrical_type("open_emitter"), "open_drain_high");
        assert_eq!(map_kicad_electrical_type("analog"), "analog");
        assert_eq!(map_kicad_electrical_type("clock"), "clock");
        assert_eq!(map_kicad_electrical_type("nc"), "do_not_connect");
        assert_eq!(map_kicad_electrical_type("no_connect"), "do_not_connect");
        // Unknown types degrade to unclassified rather than failing.
        assert_eq!(map_kicad_electrical_type("weird_type"), "unclassified");
    }

    #[test]
    fn infers_kind_from_lib_and_symbol() {
        assert_eq!(infer_kind("Device:R"), "resistor");
        assert_eq!(infer_kind("Device:C"), "capacitor");
        assert_eq!(infer_kind("Device:L"), "inductor");
        assert_eq!(infer_kind("Connector:J1"), "connector");
        assert_eq!(infer_kind("Regulator_Linear:AMS1117-3.3"), "ic");
        assert_eq!(infer_kind("MCU_Microchip_ATmega:ATmega328P"), "ic");
    }

    fn language_constraints() -> std::collections::BTreeSet<String> {
        fn variants<T: serde::de::DeserializeOwned>(statement: &str) -> Vec<String> {
            // serde lists the valid variants in its unknown-variant error; reading them keeps
            // this list from drifting away from the AST.
            let err = serde_json::from_value::<T>(serde_json::json!({"kind": "?"}))
                .err()
                .expect("an unknown attribute kind must be rejected")
                .to_string();
            let listed = err
                .split_once("expected")
                .expect("serde lists the variants")
                .1;
            listed
                .split('`')
                .skip(1)
                .step_by(2)
                .map(|attr| format!("{statement}.{attr}"))
                .collect()
        }
        [
            variants::<synth_ast::NetclassAttr>("netclass"),
            variants::<synth_ast::DiffPairAttr>("diff_pair"),
            variants::<synth_ast::KeepoutAttr>("keepout"),
        ]
        .concat()
        .into_iter()
        .collect()
    }

    fn limit_entries(descriptor: &serde_json::Value, section: &str) -> Vec<serde_json::Value> {
        descriptor[section]
            .as_array()
            .unwrap_or_else(|| panic!("descriptor must carry a `{section}` array"))
            .clone()
    }

    #[test]
    fn every_language_constraint_is_supported_unsupported_or_unverified() {
        let descriptor = capability_descriptor();
        let mut classified = std::collections::BTreeSet::new();
        for (statement, attrs) in descriptor["geometry_and_constraints"]["routing_constraints"]
            .as_object()
            .unwrap()
        {
            for attr in attrs.as_array().unwrap() {
                classified.insert(format!("{statement}.{}", attr.as_str().unwrap()));
            }
        }
        for section in ["unsupported", "unverified"] {
            for entry in limit_entries(&descriptor, section) {
                if let Some(constraint) = entry["constraint"].as_str() {
                    classified.insert(constraint.to_string());
                }
            }
        }

        let unclassified: Vec<_> = language_constraints()
            .into_iter()
            .filter(|c| !classified.contains(c))
            .collect();
        assert!(
            unclassified.is_empty(),
            "unclassified constraints: {unclassified:?}"
        );
    }

    #[test]
    fn limit_entries_are_named_and_refer_to_real_constraints() {
        let descriptor = capability_descriptor();
        let language = language_constraints();
        let mut ids = std::collections::BTreeSet::new();
        for section in ["unsupported", "unverified"] {
            let entries = limit_entries(&descriptor, section);
            assert!(!entries.is_empty(), "`{section}` must not be empty");
            for entry in entries {
                let id = entry["id"].as_str().unwrap();
                assert!(
                    !entry["summary"].as_str().unwrap().is_empty(),
                    "{id} has no summary"
                );
                assert!(ids.insert(id.to_string()), "duplicate limit id {id}");
                if let Some(constraint) = entry["constraint"].as_str() {
                    assert!(
                        language.contains(constraint),
                        "{id} names unknown {constraint}"
                    );
                }
            }
        }
    }
}
