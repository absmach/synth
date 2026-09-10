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

mod preview;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

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
        /// Force export even if Synth ERC validation produces error diagnostics.
        #[arg(long)]
        force: bool,
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
        #[arg(long)]
        pretty: bool,
    },

    /// Parse + lower + place + route (Phase 8). Dumps the
    /// `Routing` IR (per-net trace segments + vias) as JSON to
    /// stdout. Slice 1A returns an empty routing; subsequent
    /// slices add the Lee maze + A* router and negotiated
    /// congestion.
    Route {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        /// Path to directory for logging routing outcome pairs (Dataset 6).
        #[arg(long = "log-routing-outcomes", value_name = "DIR")]
        log_routing_outcomes: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
    },

    /// Parse + lower + place + route + DRC (Phase 9). Dumps
    /// the `DrcReport` (per-rule violations against the
    /// supplied manufacturer profile, or JLC's standard
    /// hobbyist tier when no profile is given) as JSON to
    /// stdout. Exits with the validation error code if any
    /// violation fires.
    Drc {
        input: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        #[arg(long, value_name = "FILE")]
        profile: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
    },

    /// Start a local HTTP server that watches the given `.synth`
    /// source and shows a live, read-only schematic + diagnostics
    /// view in the browser. Source-of-truth stays in the user's
    /// editor; this command never modifies the source.
    Preview {
        /// Path to a `.synth` source file.
        input: PathBuf,

        /// Path to the component registry.
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,

        /// Directory holding the Trunk-built browser bundle.
        /// Default: `crates/synth-web/dist` relative to the working
        /// directory. Run `trunk build --release` in
        /// `crates/synth-web/` to produce it.
        #[arg(long, value_name = "DIR")]
        assets_dir: Option<PathBuf>,

        /// Port to bind on 127.0.0.1.
        #[arg(long, default_value_t = 8080)]
        port: u16,
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

        /// Path to the Tier-2 (per-user) registry directory.
        #[arg(long, value_name = "DIR")]
        user_registry: Option<PathBuf>,
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
            force,
            allow_unverified_parts,
            user_registry,
            strict_registry,
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
            force,
            allow_unverified_parts,
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
            pretty,
        } => dump_place(&input, registry.as_deref(), pretty),
        Command::Route {
            input,
            registry,
            log_routing_outcomes,
            pretty,
        } => dump_route(
            &input,
            registry.as_deref(),
            log_routing_outcomes.as_deref(),
            pretty,
        ),
        Command::Drc {
            input,
            registry,
            profile,
            pretty,
        } => dump_drc(&input, registry.as_deref(), profile.as_deref(), pretty),
        Command::Preview {
            input,
            registry,
            assets_dir,
            port,
        } => run_preview(input, registry, assets_dir, port),
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
        Command::Registry {
            cmd,
            registry,
            user_registry,
            strict,
        } => registry_cmd(cmd, registry.as_deref(), user_registry.as_deref(), strict),
        Command::Part { cmd, user_registry } => part_cmd(cmd, user_registry.as_deref()),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("synth: {e}");
            ExitCode::from(EXIT_USAGE)
        }
    }
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

fn run_preview(
    input: PathBuf,
    registry: Option<PathBuf>,
    assets_dir: Option<PathBuf>,
    port: u16,
) -> anyhow::Result<u8> {
    if !input.exists() {
        anyhow::bail!("input file {} does not exist", input.display());
    }
    let resolved_assets = match assets_dir {
        Some(dir) if dir.exists() => dir,
        Some(dir) => {
            eprintln!(
                "synth preview: specified assets dir `{}` does not exist.",
                dir.display()
            );
            anyhow::bail!("missing assets directory");
        }
        None => {
            let candidates = [
                PathBuf::from("crates").join("synth-web").join("dist"),
                PathBuf::from("dist"),
            ];
            candidates
                .into_iter()
                .find(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("crates").join("synth-web").join("dist"))
        }
    };
    if !resolved_assets.exists() {
        eprintln!(
            "synth preview: assets dir `{}` does not exist.",
            resolved_assets.display(),
        );
        eprintln!("Build the browser bundle first:");
        eprintln!("  cd crates/synth-web && trunk build --release");
        anyhow::bail!("missing assets directory");
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("could not start tokio runtime: {e}"))?;
    runtime.block_on(preview::run_preview(input, registry, resolved_assets, port))?;
    Ok(EXIT_SUCCESS)
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
fn part_cmd(cmd: PartCommand, user_registry: Option<&Path>) -> anyhow::Result<u8> {
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

    // 2. Acquire the EasyEDA CAD JSON (from a local file or a live fetch).
    let raw = match from_file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?,
        None => fetch_component_json(code)?,
    };

    // 3. Parse + convert the CAD geometry into a KiCad footprint.
    let comp = synth_registry::easyeda::parse_easyeda(&raw)
        .map_err(|e| anyhow::anyhow!("could not parse EasyEDA JSON: {e}"))?;
    let id = code.to_lowercase();
    let pins = synth_registry::easyeda::extract_pins(&comp);
    let modl = synth_registry::easyeda::to_kicad_mod(&comp, &id);

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
    println!("pins ({}): {}", pins.len(), pins.join(", "));
    println!(
        "next: set SYNTH_USER_FOOTPRINT_DIR={} when exporting a board using this part.",
        fp_dir.display()
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
        "this part is UNVERIFIED (provenance.source = imported, reviewed_by empty) until reviewed."
    );
    Ok(EXIT_SUCCESS)
}

/// Best-effort live fetch of a component's EasyEDA CAD JSON from LCSC.
///
/// The LCSC product-detail endpoint returns the EasyEDA CAD document either as a
/// JSON string under `result.data` or, on some deployments, directly. We accept
/// both shapes. Network access is required; callers without it should pass
/// `--from-file` with a cached CAD document.
fn fetch_component_json(code: &str) -> anyhow::Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("could not start tokio runtime: {e}"))?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent("Synth-EDA/0.0.1 (part-import)")
            .build()
            .map_err(|e| anyhow::anyhow!("could not build http client: {e}"))?;
        let url = format!("https://lcsc.com/api/products/detail?productCode={code}");
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("fetch {url} failed: {e}"))?;
        let text = resp
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("reading body failed: {e}"))?;
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(data) = v.get("result").and_then(|r| r.get("data")) {
                if let Some(s) = data.as_str() {
                    return Ok(s.to_string());
                }
                if data.is_object() {
                    return Ok(data.to_string());
                }
            }
        }
        Ok(text)
    })
}

fn read_source(input: &PathBuf) -> anyhow::Result<(String, String)> {
    let source = std::fs::read_to_string(input)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", input.display()))?;
    let file = input.to_string_lossy().into_owned();
    Ok((source, file))
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
        Tier1Source::Dir(dir) => match user {
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
        },
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

fn validate(
    input: &PathBuf,
    format: Format,
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
    strict_registry: bool,
    parse_only: bool,
) -> anyhow::Result<u8> {
    let (source, file) = read_source(input)?;
    let parse = synth_parser::parse(&source, file.clone());

    let mut diagnostics = parse.diagnostics;

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
                    diagnostics.extend(synth_validate::run_erc(board, &file));
                    // Aesthetic schematic ERC (E-SYNTH-SCHEM-*): advisory
                    // warnings over the auto-layout; never blocking.
                    let layout = synth_layout::layout(board);
                    diagnostics.extend(synth_kicad::check_schem_erc(&layout, board));
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

fn dump_ast(input: &PathBuf, pretty: bool) -> anyhow::Result<u8> {
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

fn dump_ir(input: &PathBuf, registry_dir: Option<&Path>, pretty: bool) -> anyhow::Result<u8> {
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
    input: &PathBuf,
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
                let layout = synth_layout::layout(board);
                let layout_score = score.then(|| {
                    let mut s = synth_layout::score::score(&layout, board);
                    s.aesthetic_violations = synth_kicad::check_schem_erc(&layout, board)
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

fn dump_place(input: &PathBuf, registry_dir: Option<&Path>, pretty: bool) -> anyhow::Result<u8> {
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
            lowered.board.as_ref().map(|b| match synth_place::place(b) {
                Ok(p) => Some(p),
                Err(e) => {
                    let diags = e.to_diagnostics(b, &file);
                    let _ = write_diagnostics_to_stderr(&diags);
                    has_errors = true;
                    None
                }
            })
        } else {
            None
        }
    } else {
        None
    }
    .flatten();

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

fn dump_route(
    input: &PathBuf,
    registry_dir: Option<&Path>,
    log_routing_outcomes: Option<&Path>,
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
    let routing = if let Some(ast) = parse.ast.as_ref() {
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
            lowered
                .board
                .as_ref()
                .and_then(|b| match synth_place::place(b) {
                    Ok(p) => {
                        let r = synth_route::route(b, &p);
                        if let Some(log_dir) = log_routing_outcomes {
                            if let Err(e) = synth_route::log_routing_outcome(b, &p, &r, log_dir) {
                                eprintln!("synth route: failed to log routing outcome: {e}");
                            }
                        }
                        // Slice 6: emit a structured diagnostic
                        // for every unrouted net.
                        let route_diags = r.to_diagnostics(&file);
                        if !route_diags.is_empty() {
                            let _ = write_diagnostics_to_stderr(&route_diags);
                            has_errors = true;
                        }
                        Some(r)
                    }
                    Err(e) => {
                        let diags = e.to_diagnostics(b, &file);
                        let _ = write_diagnostics_to_stderr(&diags);
                        has_errors = true;
                        None
                    }
                })
        } else {
            None
        }
    } else {
        None
    };

    // `display_segments`: the same routing, cosmetically 45°-mitered for
    // rendering/export (see synth_route::miter::apply_octilinear_mitering
    // and its module-level safety note). This is purely additive —
    // `segments` (`r`, never mutated) stays the untouched, axis-aligned
    // ground truth DRC/length/export are computed from; mitering only ever
    // runs on `mitered`, a disposable clone that's read for this one JSON
    // field and then dropped. No `Grid` is available here to feed the
    // mitering pass's optional clearance re-check, so it falls back to its
    // geometry-only safety (a miter only ever removes copper from the
    // inside of a bend).
    let json_value = match &routing {
        Some(r) => {
            let mut v = serde_json::to_value(r)?;
            let mut mitered = r.clone();
            synth_route::miter::apply_octilinear_mitering(&mut mitered, None);
            if let serde_json::Value::Object(ref mut map) = v {
                map.insert(
                    "display_segments".to_string(),
                    serde_json::to_value(&mitered.segments)?,
                );
            }
            v
        }
        None => serde_json::Value::Null,
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if pretty {
        serde_json::to_writer_pretty(&mut out, &json_value)?;
    } else {
        serde_json::to_writer(&mut out, &json_value)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

fn dump_drc(
    input: &PathBuf,
    registry_dir: Option<&Path>,
    profile_path: Option<&Path>,
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
    let report = if let Some(ast) = parse.ast.as_ref() {
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
            lowered.board.as_ref().and_then(|b| {
                let placement = match synth_place::place(b) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = write_diagnostics_to_stderr(&e.to_diagnostics(b, &file));
                        has_errors = true;
                        return None;
                    }
                };
                let routing = synth_route::route(b, &placement);
                let profile = match profile_path {
                    Some(p) => match synth_drc::ManufacturerProfile::from_toml_file(p) {
                        Ok(prof) => prof,
                        Err(e) => {
                            eprintln!("synth drc: {e}");
                            has_errors = true;
                            return None;
                        }
                    },
                    None => synth_drc::ManufacturerProfile::jlc_standard(),
                };
                let r = synth_drc::check(b, &placement, &routing, &profile);
                if !r.is_clean() {
                    has_errors = true;
                    for v in &r.violations {
                        eprintln!("error: [{}] {}", v.code, v.message);
                    }
                }
                Some(r)
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
        serde_json::to_writer_pretty(&mut out, &report)?;
    } else {
        serde_json::to_writer(&mut out, &report)?;
    }
    writeln!(&mut out)?;

    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

// strict_registry/validate_erc/force/allow_unverified_parts are four
// independent, non-exclusive CLI toggles (`--strict-registry`,
// `--validate-erc`, `--force`, `--allow-unverified-parts`) — an enum
// would need a variant per combination for no clarity gain.
#[allow(clippy::fn_params_excessive_bools)]
fn export_kicad(
    input: &PathBuf,
    registry_dir: Option<&Path>,
    user_registry: Option<&Path>,
    strict_registry: bool,
    out_dir: &Path,
    fab: synth_kicad::FabRequest,
    validate_erc: bool,
    force: bool,
    allow_unverified_parts: bool,
) -> anyhow::Result<u8> {
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
    let board = lowered
        .board
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lowering produced no IR; not exporting"))?;

    // Slice 7: Run Synth ERC validation before exporting
    let erc_diags = synth_validate::run_erc(board, &file);
    write_diagnostics_to_stderr(&erc_diags)?;
    let has_erc_errors = erc_diags.iter().any(|d| d.severity.is_blocking());
    if has_erc_errors && !force {
        anyhow::bail!("Synth ERC validation failed; use --force to export anyway");
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
                "warning: [E-SYNTH-EXPORT-001] exporting with synthesized bounding-box \
                 footprints for: {list} (--force)"
            );
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

    // Aesthetic schematic ERC (E-SYNTH-SCHEM-*): advisory warnings
    // printed alongside the rule-based ERC; never blocks export.
    let pre_layout = synth_layout::layout(board);
    let schem_diags = synth_kicad::check_schem_erc(&pre_layout, board);
    write_diagnostics_to_stderr(&schem_diags)?;

    // Honour manual tuning: if a sidecar sits beside the source
    // (`<stem>.synth.layout.toml`), the export applies those drags.
    let sidecar_path = input.with_file_name(format!(
        "{}.synth.layout.toml",
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("design")
    ));
    let sidecar = sidecar_path.is_file().then_some(sidecar_path.as_path());

    let result = synth_kicad::export_with_sidecar(board, out_dir, sidecar)
        .map_err(|e| anyhow::anyhow!("kicad export failed: {e}"))?;

    eprintln!("wrote {}", result.project_path.display());
    eprintln!("wrote {}", result.schematic_path.display());
    eprintln!("wrote {}", result.library_path.display());
    eprintln!("wrote {}", result.pcb_path.display());
    eprintln!("wrote {}", result.bom_path.display());

    if !fab.is_empty() {
        // R15.3 trust boundary: a fab submission (gerbers/drill/step) must
        // not silently include parts nobody has reviewed. Interactive
        // preview/validation stay non-blocking (`W-SYNTH-PART-UNVERIFIED`
        // is a warning there); manufacturing export is where it becomes a
        // hard gate, per §18.8.2.
        let unverified = parts_unverified(board);
        if !unverified.is_empty() && !allow_unverified_parts {
            let list = unverified.join(", ");
            eprintln!(
                "error: [W-SYNTH-PART-UNVERIFIED] refusing fab export: the following parts \
                 have no reviewer (`[provenance].reviewed_by` empty): {list}. Review them and \
                 set `reviewed_by`, or re-run with --allow-unverified-parts to submit anyway."
            );
            return Ok(1);
        }
        let artifacts = synth_kicad::run_fab(&result.pcb_path, out_dir, &fab)
            .map_err(|e| anyhow::anyhow!("kicad-cli fab export failed: {e}"))?;
        if let Some(dir) = artifacts.gerbers_dir {
            eprintln!("wrote gerbers into {}", dir.display());
        }
        if let Some(dir) = artifacts.drill_dir {
            eprintln!("wrote drill files into {}", dir.display());
        }
        if let Some(path) = artifacts.step_path {
            eprintln!("wrote {}", path.display());
        }
    }

    // Slice 3: Run KiCad ERC if requested
    let mut has_kicad_erc_errs = false;
    if validate_erc {
        eprintln!(
            "running kicad-cli sch erc on {}",
            result.schematic_path.display()
        );
        match synth_kicad::run_kicad_erc(&result.schematic_path) {
            Ok(violations) => {
                if violations.is_empty() {
                    eprintln!("kicad-cli sch erc: 0 violations found");
                } else {
                    for v in &violations {
                        eprintln!(
                            "[kicad-erc] {}: [{}] {}",
                            v.severity, v.violation_type, v.description
                        );
                        if v.severity.eq_ignore_ascii_case("error") {
                            has_kicad_erc_errs = true;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: could not run kicad-cli sch erc: {e}");
            }
        }
    }

    // Slice 13.4: Run KiCad native PCB DRC verification gate
    let mut has_kicad_drc_errors = false;
    match synth_drc::run_kicad_cli_drc(&result.pcb_path) {
        Ok(violations) => {
            if violations.is_empty() {
                eprintln!("kicad-cli pcb drc: 0 violations found (Phase 13 Zero-DRC gate clean)");
            } else {
                for v in &violations {
                    eprintln!("[kicad-drc] error: [{}] {}", v.code, v.message);
                    has_kicad_drc_errors = true;
                }
            }
        }
        Err(e) => {
            eprintln!("warning: could not run kicad-cli pcb drc: {e}");
        }
    }

    let has_errors = parse.has_errors()
        || lowered.has_errors()
        || (has_erc_errors && !force)
        || (has_kicad_drc_errors && !force)
        || has_kicad_erc_errs;
    Ok(if has_errors {
        EXIT_VALIDATION_ERRORS
    } else {
        EXIT_SUCCESS
    })
}

/// Run the full validate pipeline and return every diagnostic.
/// Shared between `validate` (which formats them) and `fix`
/// (which picks patches out of them).
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

fn supply_chain(
    input: &PathBuf,
    registry_dir: Option<&Path>,
    format: Format,
) -> anyhow::Result<u8> {
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
    input: &PathBuf,
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
}
