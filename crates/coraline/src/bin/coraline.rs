#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts we can't control (base64, getrandom, hashbrown)"
)]
use std::path::{Path, PathBuf};

use coraline::audit;
use coraline::config;
use coraline::context;
use coraline::db;
use coraline::doctor;
use coraline::extraction;
use coraline::logging;
use coraline::mcp::McpServer;
use coraline::memory;
use coraline::sync::GitHooksManager;
use coraline::types::NodeKind;
use coraline::types::{BuildContextOptions, ContextFormat, EdgeKind};
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
use coraline::vectors;
use tracing::{debug, info};

use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Debug, Parser)]
#[command(name = "coraline")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Code intelligence and knowledge graph for any codebase")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Install,
    Init(InitArgs),
    Index(IndexArgs),
    Sync(SyncArgs),
    Status(StatusArgs),
    Stats(StatsArgs),
    Doctor(DoctorArgs),
    Query(QueryArgs),
    Context(ContextArgs),
    Callers(CallersArgs),
    Callees(CalleesArgs),
    Impact(ImpactArgs),
    Config(ConfigArgs),
    Hooks(HooksArgs),
    Serve(ServeArgs),
    /// Audit documentation accuracy and coverage against the code graph.
    AuditDocs(AuditDocsArgs),
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    Embed(EmbedArgs),
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    Model(ModelArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    path: Option<PathBuf>,
    #[arg(short = 'i', long = "index")]
    index: bool,
    #[arg(long = "no-hooks")]
    no_hooks: bool,
    #[arg(
        short = 'f',
        long = "force",
        help = "Overwrite existing .coraline directory without prompting"
    )]
    force: bool,
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    /// Download the embedding model during init (skips the TTY prompt).
    #[arg(long = "embed")]
    embed: bool,
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    /// Skip the embedding model entirely (no prompt, no download).
    /// Conflicts with `--embed`.
    #[arg(long = "no-embed", conflicts_with = "embed")]
    no_embed: bool,
    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    /// Non-interactive mode. Auto-accepts the model download prompt.
    #[arg(short = 'y', long = "yes")]
    yes: bool,
}

#[derive(Debug, Args)]
struct IndexArgs {
    path: Option<PathBuf>,
    #[arg(short = 'f', long = "force")]
    force: bool,
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

#[derive(Debug, Args)]
struct SyncArgs {
    path: Option<PathBuf>,
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Project root (defaults to current directory).
    path: Option<PathBuf>,
    /// Skip slow model-load + inference probes.
    #[arg(long = "quick", conflicts_with = "deep")]
    quick: bool,
    /// Explicit deep mode (default): include model load + inference + coverage probes.
    #[arg(long = "deep", conflicts_with = "quick")]
    deep: bool,
    /// Output report as JSON for machine consumption.
    #[arg(long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct QueryArgs {
    search: String,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(short = 'l', long = "limit", default_value_t = 10)]
    limit: usize,
    #[arg(short = 'k', long = "kind")]
    kind: Option<String>,
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct ContextArgs {
    task: String,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(short = 'n', long = "max-nodes", default_value_t = 50)]
    max_nodes: usize,
    #[arg(short = 'c', long = "max-code", default_value_t = 10)]
    max_code: usize,
    #[arg(long = "no-code")]
    no_code: bool,
    #[arg(short = 'f', long = "format", default_value = "markdown")]
    format: String,
}

#[derive(Debug, Args)]
struct StatsArgs {
    path: Option<PathBuf>,
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct CallersArgs {
    node_id: String,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(short = 'l', long = "limit", default_value_t = 20)]
    limit: usize,
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct CalleesArgs {
    node_id: String,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(short = 'l', long = "limit", default_value_t = 20)]
    limit: usize,
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct ImpactArgs {
    node_id: String,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(short = 'd', long = "depth", default_value_t = 3)]
    depth: usize,
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    /// Print config as JSON
    #[arg(short = 'j', long = "json")]
    json: bool,
    /// Section to display (indexing, context, sync, vectors)
    #[arg(short = 's', long = "section")]
    section: Option<String>,
    /// Set a value: --set section.key=value
    #[arg(long = "set")]
    set: Option<String>,
    /// Migrate legacy config.json to config.toml
    #[arg(long = "migrate")]
    migrate: bool,
}

#[derive(Debug, Args)]
struct HooksArgs {
    #[command(subcommand)]
    action: HooksAction,
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum HooksAction {
    Install,
    Remove,
    Status,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    #[arg(long = "mcp")]
    mcp: bool,
    #[arg(
        long = "timeout",
        default_value_t = 120_000,
        help = "Timeout in milliseconds for tool execution (default: 120000 = 2 minutes)"
    )]
    timeout_ms: u64,
}

#[derive(Debug, Args)]
struct AuditDocsArgs {
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    /// Hide stale-reference findings.
    #[arg(long = "no-stale")]
    no_stale: bool,
    /// Hide undocumented-export findings.
    #[arg(long = "no-undocumented")]
    no_undocumented: bool,
    /// Maximum items to display per category.
    #[arg(short = 'l', long = "limit", default_value_t = 50)]
    limit: usize,
    /// Output raw JSON instead of formatted text.
    #[arg(short = 'j', long = "json")]
    json: bool,
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
#[derive(Debug, Args)]
struct EmbedArgs {
    /// Project root (defaults to current directory).
    path: Option<PathBuf>,
    /// Number of nodes to embed per batch (for progress display).
    #[arg(long = "batch-size", default_value_t = 50)]
    batch_size: usize,
    /// Suppress progress output.
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
    /// Download the model from `HuggingFace` if not already present.
    #[arg(long = "download")]
    download: bool,
    /// ONNX variant to download when using `--download`.
    /// Defaults to the configured model's recommended variant.
    #[arg(long = "variant")]
    variant: Option<String>,
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
#[derive(Debug, Args)]
struct ModelArgs {
    #[arg(short = 'p', long = "path")]
    path: Option<PathBuf>,
    /// Suppress progress output.
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
    #[command(subcommand)]
    action: ModelAction,
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
#[derive(Debug, Subcommand)]
enum ModelAction {
    /// Download model files from `HuggingFace` (tokenizer + ONNX weights).
    Download {
        /// Which supported model to download (see `coraline model list`).
        /// Defaults to `vectors.model` from config.toml.
        #[arg(long = "model")]
        model: Option<String>,
        /// ONNX variant filename to download.
        /// Defaults to the model's recommended variant.
        #[arg(long = "variant")]
        variant: Option<String>,
        /// Re-download even if the file already exists.
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    /// Show which model files are present in the model directory.
    Status {
        /// Which supported model to inspect. Defaults to `vectors.model`.
        #[arg(long = "model")]
        model: Option<String>,
    },
    /// Copy model files from the legacy per-project location to the shared global directory.
    ///
    /// The legacy location is `.coraline/models/nomic-embed-text-v1.5/` inside the
    /// project root. After migration the old directory can be removed manually.
    ///
    /// This is deliberately nomic-only: the legacy per-project layout predates
    /// multi-model support entirely, so there is nothing to migrate for any
    /// other model.
    Migrate,
    /// List every embedding model Coraline knows how to download and run.
    List,
}

fn main() {
    let cli = Cli::parse();
    if matches!(cli.command, None | Some(Command::Install)) {
        run_installer();
        return;
    }

    let Some(command) = cli.command else {
        return;
    };

    // Resolve project root early so logging can target the right directory
    let project_root_hint = match &command {
        Command::Init(a) => a.path.clone(),
        Command::Index(a) => a.path.clone(),
        Command::Sync(a) => a.path.clone(),
        Command::Status(a) => a.path.clone(),
        Command::Stats(a) => a.path.clone(),
        Command::Doctor(a) => a.path.clone(),
        Command::Query(a) => a.path.clone(),
        Command::Context(a) => a.path.clone(),
        Command::Callers(a) => a.path.clone(),
        Command::Callees(a) => a.path.clone(),
        Command::Impact(a) => a.path.clone(),
        Command::Config(a) => a.path.clone(),
        Command::Hooks(a) => a.path.clone(),
        Command::Serve(a) => a.path.clone(),
        Command::AuditDocs(a) => a.path.clone(),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Embed(a) => a.path.clone(),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Model(a) => a.path.clone(),
        Command::Install => None,
    };
    let project_root = resolve_project_root(project_root_hint);
    // Suppress logging entirely during a fresh `init` so stdout progress
    // output stays clean. (logging::init independently refuses to create
    // .coraline/logs/ for any command until the project is actually
    // initialized, so this is a UX nicety, not the correctness guard.)
    let log_root = if matches!(command, Command::Init(_)) && !is_initialized(&project_root) {
        None
    } else {
        Some(project_root.as_path())
    };
    let _log_guard = logging::init(log_root);
    info!("coraline starting");
    debug!(command = ?command, "dispatching command");

    match command {
        Command::Install => run_installer(),
        Command::Init(args) => run_init(args),
        Command::Index(args) => run_index(args),
        Command::Sync(args) => run_sync(args),
        Command::Status(args) => run_status(args),
        Command::Stats(args) => run_stats(args),
        Command::Doctor(args) => run_doctor(args),
        Command::Query(args) => run_query(args),
        Command::Context(args) => run_context(args),
        Command::Callers(args) => run_callers(args),
        Command::Callees(args) => run_callees(args),
        Command::Impact(args) => run_impact(args),
        Command::Config(args) => run_config(args),
        Command::Hooks(args) => match args.action {
            HooksAction::Install => run_hooks_install(args.path),
            HooksAction::Remove => run_hooks_remove(args.path),
            HooksAction::Status => run_hooks_status(args.path),
        },
        Command::Serve(args) => {
            if args.mcp {
                let mut server = McpServer::with_timeout(args.path, args.timeout_ms);
                if let Err(err) = server.start() {
                    eprintln!("Failed to start MCP server: {err}");
                    std::process::exit(1);
                }
            } else {
                println!("Use --mcp to start the MCP server.");
            }
        }
        Command::AuditDocs(args) => run_audit_docs(args),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Embed(args) => run_embed(args),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Model(args) => run_model(args),
    }
}

/// Resolve which model a `coraline model` subcommand should act on: the
/// `--model` flag if given, else `vectors.model` from config.toml.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn resolve_action_model(project_root: &Path, model_flag: Option<&str>) -> (String, PathBuf) {
    let mut cfg = config::load_toml_config(project_root).unwrap_or_default();
    if let Some(m) = model_flag {
        cfg.vectors.model = m.to_string();
        cfg.vectors.model_dir = None;
    }
    vectors::resolve_model_dir(&cfg.vectors)
        .unwrap_or_else(|_| (cfg.vectors.model.clone(), vectors::global_model_dir()))
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model_download(
    project_root: &Path,
    quiet: bool,
    model: Option<&str>,
    variant: Option<String>,
    force: bool,
) {
    let (model_name, model_dir) = resolve_action_model(project_root, model);
    let spec = match vectors::model_spec(&model_name) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let variant = variant.unwrap_or_else(|| spec.default_filename.to_string());

    if !quiet {
        println!(
            "Downloading {model_name} ({variant}) into {} ...",
            model_dir.display()
        );
    }
    if let Err(e) = vectors::download_model(&model_name, &model_dir, &variant, !force, quiet) {
        eprintln!("Download failed: {e}");
        std::process::exit(1);
    }
    if !quiet {
        println!("Done. Run `coraline embed` to generate embeddings.");
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model_status(project_root: &Path, model: Option<&str>) {
    let (model_name, model_dir) = resolve_action_model(project_root, model);
    println!("Model:           {model_name}");
    println!("Model directory: {}", model_dir.display());
    println!();
    match vectors::model_spec(&model_name) {
        Ok(spec) => {
            for name in spec.preference_order {
                let p = model_dir.join(name);
                if let Ok(meta) = std::fs::metadata(&p) {
                    println!("  {name:<30}  {:>6} MB  [present]", meta.len() / 1_000_000);
                } else {
                    println!("  {name:<30}  (not present)");
                }
            }
        }
        Err(e) => eprintln!("{e}"),
    }
    println!();
    for name in &["tokenizer.json", "tokenizer_config.json"] {
        let p = model_dir.join(name);
        if p.exists() {
            println!("  {name:<30}  [present]");
        } else {
            println!("  {name:<30}  (not present)");
        }
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model_migrate(project_root: &Path) {
    let global_dir = vectors::global_model_dir();
    let legacy_dir = project_root
        .join(".coraline")
        .join("models")
        .join(vectors::DEFAULT_MODEL);

    let global_has_model = vectors::MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| global_dir.join(name).exists());

    if global_has_model {
        println!(
            "Shared model directory already populated: {}",
            global_dir.display()
        );
        println!("Nothing to migrate.");

        return;
    }

    let legacy_has_model = vectors::MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| legacy_dir.join(name).exists());

    if !legacy_has_model {
        println!(
            "No model files found in legacy location: {}",
            legacy_dir.display()
        );
        println!(
            "Run `coraline model download` to fetch the model into {}",
            global_dir.display()
        );

        return;
    }

    // The lazy migration function prints its own message when files are copied.
    vectors::maybe_migrate_legacy_model(&global_dir, project_root);
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model_list() {
    println!("Supported embedding models:\n");
    for spec in vectors::SUPPORTED_MODELS {
        let default_marker = if spec.name == vectors::DEFAULT_MODEL {
            " (default)"
        } else {
            ""
        };
        println!("  {}{}", spec.name, default_marker);
        println!("    {}", spec.description);
        println!(
            "    dimension: {}, default variant: {}",
            spec.dimension, spec.default_filename
        );
        println!();
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model(args: ModelArgs) {
    let project_root = resolve_project_root(args.path);

    // Lazily migrate any nomic model files found in the legacy per-project
    // location (deliberately nomic-only, see `ModelAction::Migrate` docs).
    vectors::maybe_migrate_legacy_model(&vectors::global_model_dir(), &project_root);

    match args.action {
        ModelAction::Download {
            model,
            variant,
            force,
        } => run_model_download(&project_root, args.quiet, model.as_deref(), variant, force),
        ModelAction::Status { model } => run_model_status(&project_root, model.as_deref()),
        ModelAction::Migrate => run_model_migrate(&project_root),
        ModelAction::List => run_model_list(),
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_embed(args: EmbedArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    // Auto-download model files if requested.
    if args.download {
        let (model_name, model_dir) = resolve_action_model(&project_root, None);
        let variant = match vectors::model_spec(&model_name) {
            Ok(spec) => args
                .variant
                .clone()
                .unwrap_or_else(|| spec.default_filename.to_string()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        if !args.quiet {
            println!(
                "Downloading {model_name} ({variant}) into {} ...",
                model_dir.display()
            );
        }
        if let Err(e) = vectors::download_model(&model_name, &model_dir, &variant, true, args.quiet)
        {
            eprintln!("Download failed: {e}");
            std::process::exit(1);
        }
    }

    let mut vm = load_embedding_model(&project_root, args.quiet);

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let nodes = db::get_all_nodes(&conn).unwrap_or_else(|err| {
        eprintln!("Failed to read nodes: {err}");
        std::process::exit(1);
    });

    let total = nodes.len();
    if total == 0 {
        println!("No nodes found. Run `coraline index` first.");
        return;
    }

    if !args.quiet {
        println!("Embedding {total} nodes…");
    }

    let pb = (!args.quiet)
        .then(|| spinner_bar(total as u64, "{spinner:.cyan} Embedding {pos}/{len} {msg}"));

    let (ok, skipped) = embed_nodes(
        &nodes,
        &mut vm,
        &conn,
        pb.as_ref(),
        args.batch_size,
        args.quiet,
    );

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    if !args.quiet {
        println!("Embedded {ok}/{total} nodes ({skipped} skipped)");
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn load_embedding_model(project_root: &Path, quiet: bool) -> vectors::VectorManager {
    let result = if quiet {
        vectors::VectorManager::from_project(project_root)
    } else {
        let pb = spinner_indefinite("Loading embedding model…");
        let res = vectors::VectorManager::from_project(project_root);
        pb.finish_and_clear();
        println!("Loading embedding model…");
        res
    };
    result.unwrap_or_else(|err| {
        eprintln!("Failed to load model: {err}");
        let (_, model_dir) = resolve_action_model(project_root, None);
        eprintln!(
            "Download model.onnx + tokenizer.json into {} (or run `coraline model download`)",
            model_dir.display()
        );
        std::process::exit(1);
    })
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn embed_nodes(
    nodes: &[coraline::types::Node],
    vm: &mut vectors::VectorManager,
    conn: &rusqlite::Connection,
    pb: Option<&ProgressBar>,
    batch_size: usize,
    quiet: bool,
) -> (usize, usize) {
    let mut ok = 0usize;
    let mut skipped = 0usize;
    let total = nodes.len();

    for (i, node) in nodes.iter().enumerate() {
        let text = vectors::node_embed_text(
            &node.name,
            &node.qualified_name,
            node.docstring.as_deref(),
            node.signature.as_deref(),
        );

        match vm.embed(&text) {
            Ok(embedding) => {
                if let Err(err) =
                    vectors::store_embedding(conn, &node.id, &embedding, vm.model_name())
                {
                    if !quiet {
                        eprintln!(
                            "  Warning: failed to store embedding for {}: {err}",
                            node.id
                        );
                    }
                    skipped += 1;
                } else {
                    ok += 1;
                }
            }
            Err(err) => {
                if !quiet {
                    eprintln!("  Warning: failed to embed {}: {err}", node.name);
                }
                skipped += 1;
            }
        }

        if let Some(pb) = pb {
            pb.set_message(format!("({:?}) {}", node.kind, node.name));
            pb.inc(1);
        } else if !quiet && (i + 1) % batch_size == 0 {
            print!("\r  {}/{total}", i + 1);
        }
    }

    (ok, skipped)
}

fn cargo_bin_dir() -> PathBuf {
    // Prefer CARGO_HOME if set, then fall back to the platform home directory.
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        return PathBuf::from(cargo_home).join("bin");
    }
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    if let Some(home) = std::env::var_os(home_var) {
        return PathBuf::from(home).join(".cargo").join("bin");
    }
    PathBuf::from(".cargo/bin")
}

fn run_installer() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Coraline v{version} — installation check\n");

    // 1. Where is this binary right now?
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Could not determine current executable path: {e}");
            std::process::exit(1);
        }
    };
    let current_exe = current_exe.canonicalize().unwrap_or(current_exe);
    println!("Current binary : {}", current_exe.display());

    // 2. Determine the standard cargo bin directory.
    let cargo_bin = cargo_bin_dir();
    let bin_name = if cfg!(windows) {
        "coraline.exe"
    } else {
        "coraline"
    };
    let target = cargo_bin.join(bin_name);
    println!("Install target : {}\n", target.display());

    // 3. Copy to cargo bin if not already there.
    let already_installed = current_exe == target.canonicalize().unwrap_or_else(|_| target.clone());
    if already_installed {
        println!("✔  Already installed at: {}", target.display());
    } else {
        if let Err(e) = std::fs::create_dir_all(&cargo_bin) {
            eprintln!("Error creating {}: {e}", cargo_bin.display());
            std::process::exit(1);
        }
        match std::fs::copy(&current_exe, &target) {
            Ok(_) => println!("✔  Installed to: {}", target.display()),
            Err(e) => {
                eprintln!("Failed to copy binary to {}: {e}", target.display());
                if cfg!(windows) {
                    eprintln!("Try running the installer as Administrator, or install via:");
                } else {
                    eprintln!("Try running with sudo, or install via:");
                }
                eprintln!("  cargo install coraline");
                std::process::exit(1);
            }
        }
    }

    // 4. Set executable bit on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&target) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(&target, perms);
        }
    }

    // 5. PATH check.
    println!();
    if which("coraline") {
        println!("✔  'coraline' is on PATH — run `coraline --version` to verify.");
    } else {
        println!("⚠  The install directory is not on PATH.");
        if cfg!(windows) {
            println!(
                "   Add it via: System Properties → Environment Variables → PATH → add:\n   {}",
                cargo_bin.display()
            );
        } else {
            println!("   Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):");
            println!("     export PATH=\"$HOME/.cargo/bin:$PATH\"");
        }
        println!("   Then open a new terminal and run: coraline --version");
    }
}

fn run_init(args: InitArgs) {
    let project_root = resolve_project_root(args.path);

    if is_initialized(&project_root) {
        // If the user just wants to (re)index an already-initialized project,
        // skip the destructive overwrite entirely.
        if args.index && !args.force {
            println!(
                "Coraline already initialized in {}.",
                project_root.display()
            );
            #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
            init_model::handle_model_decision(&project_root, args.embed, args.no_embed, args.yes);
            run_index(IndexArgs {
                path: Some(project_root),
                force: false,
                quiet: false,
            });
            return;
        }

        if !args.force {
            // Only prompt when stdin is a terminal; otherwise abort safely.
            if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                eprint!(
                    "Coraline is already initialized in {}. Overwrite? [y/N] ",
                    project_root.display()
                );
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err()
                    || !input.trim().eq_ignore_ascii_case("y")
                {
                    println!("Aborted.");
                    return;
                }
            } else {
                eprintln!(
                    "Coraline already initialized in {}. Use --force to overwrite.",
                    project_root.display()
                );
                return;
            }
        }
        // Remove the existing .coraline directory before re-initializing.
        if let Err(err) = std::fs::remove_dir_all(project_root.join(".coraline")) {
            eprintln!("Failed to remove existing .coraline directory: {err}");
            std::process::exit(1);
        }
    }

    if let Err(err) = create_coraline_dir(&project_root) {
        eprintln!("Failed to create .coraline directory: {err}");
        std::process::exit(1);
    }

    // Create default config.toml
    if let Err(err) = config::write_toml_template(&project_root) {
        eprintln!("Failed to write config.toml: {err}");
        std::process::exit(1);
    }

    if let Err(err) = db::initialize_database(&project_root) {
        eprintln!("Failed to initialize database: {err}");
        std::process::exit(1);
    }

    // Create initial memory templates
    let project_name = project_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    if let Err(err) = memory::create_initial_memories(&project_root, project_name) {
        eprintln!("Warning: Failed to create initial memories: {err}");
    }

    println!("Initialized Coraline in {}", project_root.display());

    if !args.no_hooks {
        let hooks = GitHooksManager::new(&project_root);
        if hooks.is_git_repository() {
            let result = hooks.install_hook();
            if result.success {
                println!("Git hooks installed.");
            } else {
                eprintln!("Git hooks not installed: {}", result.message);
            }
        }
    }

    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    init_model::handle_model_decision(&project_root, args.embed, args.no_embed, args.yes);

    if args.index {
        run_index(IndexArgs {
            path: Some(project_root),
            force: false,
            quiet: false,
        });
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
mod init_model {
    use super::{Path, PathBuf, config, vectors};

    /// Inputs to the embedding-model decision.
    ///
    /// Bundled in a struct so the decision function takes a single argument
    /// (and to keep `clippy::fn_params_excessive_bools` happy).
    /// `struct_excessive_bools` is allow-by-default in this project.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ModelInputs {
        pub model_present: bool,
        pub no_embed: bool,
        pub embed: bool,
        pub yes: bool,
        pub is_tty: bool,
    }

    /// After a fresh `init`, decide what to do about the embedding model and
    /// execute the chosen action. Behaviour is sum of the `InitArgs` flags
    /// (`--embed`, `--no-embed`, `--yes`) plus the TTY state and whether the
    /// model is already on disk. All non-embedding tools remain fully
    /// functional regardless of the chosen action.
    pub fn handle_model_decision(project_root: &Path, embed: bool, no_embed: bool, yes: bool) {
        let (model_name, model_dir) = resolve_configured_model(project_root);
        let model_present = vectors::model_spec(&model_name).is_ok_and(|spec| {
            spec.preference_order
                .iter()
                .any(|name| model_dir.join(name).exists())
        });
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
        let inputs = ModelInputs {
            model_present,
            no_embed,
            embed,
            yes,
            is_tty,
        };
        let action = decide_model_action(inputs);
        execute_model_action(project_root, &action);
    }

    /// What action to take regarding the embedding model after `init`.
    ///
    /// The decision is pure: it takes the inputs and returns the action. The
    /// IO/execution side is in [`execute_model_action`].
    #[derive(Debug, PartialEq, Eq)]
    pub enum ModelAction {
        /// Model already present; nothing to do.
        NoOp,
        /// User opted out (`--no-embed` or declined the TTY prompt).
        Skip,
        /// Download the model now.
        Download,
        /// Interactive prompt needed.
        Prompt,
        /// Non-interactive, no explicit decision; print a hint.
        Hint,
    }

    /// Pure decision function for the post-init model action.
    ///
    /// Precedence:
    /// 1. Model present → `NoOp`
    /// 2. `--no-embed` → `Skip`
    /// 3. `--embed` or `--yes` → `Download`
    /// 4. TTY → `Prompt`
    /// 5. Otherwise → `Hint`
    pub const fn decide_model_action(inputs: ModelInputs) -> ModelAction {
        if inputs.model_present {
            return ModelAction::NoOp;
        }
        if inputs.no_embed {
            return ModelAction::Skip;
        }
        if inputs.embed || inputs.yes {
            return ModelAction::Download;
        }
        if inputs.is_tty {
            return ModelAction::Prompt;
        }
        ModelAction::Hint
    }

    /// Resolve the configured model name and its on-disk directory
    /// (`vectors.model` / `vectors.model_dir` from config.toml).
    pub fn resolve_configured_model(project_root: &Path) -> (String, PathBuf) {
        let cfg = config::load_toml_config(project_root).unwrap_or_default();
        vectors::resolve_model_dir(&cfg.vectors)
            .unwrap_or_else(|_| (cfg.vectors.model.clone(), vectors::global_model_dir()))
    }

    pub fn execute_model_action(project_root: &Path, action: &ModelAction) {
        use std::io::Write as _;
        let (model_name, model_dir) = resolve_configured_model(project_root);

        match action {
            ModelAction::NoOp => {}
            ModelAction::Skip => {
                println!("Skipped. Run `coraline model download` later to enable semantic search.");
            }
            ModelAction::Download => {
                download_model_and_report(&model_name, &model_dir);
            }
            ModelAction::Prompt => {
                eprint!("Download embedding model for semantic search? (~137 MB) [Y/n] ");
                let _ = std::io::stderr().flush();
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err() {
                    return;
                }
                let answer = input.trim();
                if answer.is_empty() || answer.eq_ignore_ascii_case("y") {
                    download_model_and_report(&model_name, &model_dir);
                } else {
                    println!(
                        "Skipped. Run `coraline model download` later to enable semantic search."
                    );
                }
            }
            ModelAction::Hint => {
                eprintln!(
                    "Tip: run `coraline model download` then `coraline embed` to enable semantic search."
                );
            }
        }
    }

    fn download_model_and_report(model_name: &str, model_dir: &Path) {
        let Ok(spec) = vectors::model_spec(model_name) else {
            eprintln!("Unknown embedding model '{model_name}'.");
            return;
        };
        println!("Downloading {model_name} into {} ...", model_dir.display());
        match vectors::download_model(model_name, model_dir, spec.default_filename, true, false) {
            Ok(()) => println!("Done. Run `coraline embed` to generate embeddings."),
            Err(e) => {
                eprintln!("Model download failed: {e}");
                eprintln!("You can retry later with: coraline model download");
            }
        }
    }
}

fn run_index(args: IndexArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    // Load config with auto-migration from config.json if needed
    let toml_cfg = match config::load_config_with_migration(&project_root, true) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };
    let cfg = config::toml_to_code_graph_config(&project_root, &toml_cfg);

    if !args.quiet {
        println!("Indexing project…");
    }

    let result = if args.quiet {
        extraction::index_all(&project_root, &cfg, args.force, None)
    } else {
        let pb = spinner_bar(0, "{spinner:.cyan} {msg}");
        let cb = |p: extraction::IndexProgress| update_index_spinner(&pb, &p);
        let res = extraction::index_all(&project_root, &cfg, args.force, Some(&cb));
        pb.finish_and_clear();
        res
    }
    .unwrap_or_else(|err| {
        eprintln!("Indexing failed: {err}");
        std::process::exit(1);
    });

    if !args.quiet {
        println!("Indexed {} files", result.files_indexed);
        println!("Created {} nodes", result.nodes_created);
        println!("Completed in {}ms", result.duration_ms);
    }
}

fn run_sync(args: SyncArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    // Load config with auto-migration from config.json if needed
    let toml_cfg = match config::load_config_with_migration(&project_root, true) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };
    let cfg = config::toml_to_code_graph_config(&project_root, &toml_cfg);

    let result = if args.quiet {
        extraction::sync(&project_root, &cfg, None)
    } else {
        let pb = spinner_bar(0, "{spinner:.cyan} {msg}");
        let cb = |p: extraction::IndexProgress| update_index_spinner(&pb, &p);
        let res = extraction::sync(&project_root, &cfg, Some(&cb));
        pb.finish_and_clear();
        res
    }
    .unwrap_or_else(|err| {
        eprintln!("Sync failed: {err}");
        std::process::exit(1);
    });

    if !args.quiet {
        let total_changes = result.files_added + result.files_modified + result.files_removed;
        if total_changes == 0 {
            println!("Already up to date");
        } else {
            println!("Synced {total_changes} files");
            if result.files_added > 0 {
                println!("  Added: {}", result.files_added);
            }
            if result.files_modified > 0 {
                println!("  Modified: {}", result.files_modified);
            }
            if result.files_removed > 0 {
                println!("  Removed: {}", result.files_removed);
            }
            println!("Updated {} nodes", result.nodes_updated);
        }
    }
}

fn run_status(args: StatusArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        println!("Coraline Status\n");
        println!("Project: {}", project_root.display());
        println!("Not initialized. Run `coraline init`.");
        return;
    }

    let cfg_path = config::config_path(&project_root);
    let db_path = db::database_path(&project_root);
    let db_size = std::fs::metadata(&db_path).map_or(0, |m| m.len());

    println!("Coraline Status\n");
    println!("Project: {}", project_root.display());
    println!("Config:  {}", cfg_path.display());
    println!("Database: {} ({} bytes)", db_path.display(), db_size);

    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    {
        let (model_name, model_dir) = doctor::resolve_status_model(&project_root);
        match doctor::compute_model_state(&model_dir, &model_name) {
            doctor::ModelState::Present {
                ref name,
                size_bytes,
            } => {
                let size_mb = size_bytes / 1_000_000;
                println!("Embeddings: {name} ({size_mb} MB)");
            }
            doctor::ModelState::Absent => {
                println!("Embeddings: not present");
                println!("            Run `coraline model download` to enable semantic search.");
            }
        }
        println!("Model:      {model_name}");
        println!("Model dir:  {}", model_dir.display());
    }

    let hooks = GitHooksManager::new(&project_root);
    if hooks.is_git_repository() {
        if hooks.is_hook_installed() {
            println!("Git hooks: installed");
        } else {
            println!("Git hooks: not installed");
        }
    } else {
        println!("Git hooks: not a git repository");
    }
}

fn run_doctor(args: DoctorArgs) {
    let project_root = resolve_project_root(args.path);
    let deep = !args.quick;
    let report = doctor::run_all(&project_root, deep);

    if args.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to serialize report: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print_doctor_report(&report, deep);
    }

    std::process::exit(report.exit_code);
}

fn print_doctor_report(report: &doctor::Report, deep: bool) {
    if deep {
        let pb = spinner_indefinite("Running doctor probes…");
        for probe in &report.probes {
            let mark = if probe.ok { "✔" } else { "✘" };
            pb.println(format!("{mark}  {}", probe.name));
            if !probe.ok
                && let Some(fix) = &probe.fix
            {
                pb.println(format!("    → {fix}"));
            }
        }
        pb.finish_and_clear();
    } else {
        for probe in &report.probes {
            let mark = if probe.ok { "✔" } else { "✘" };
            println!("{mark}  {}", probe.name);
            if !probe.ok
                && let Some(fix) = &probe.fix
            {
                println!("    → {fix}");
            }
        }
    }
}

fn run_query(args: QueryArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let kind = args.kind.as_deref().and_then(parse_node_kind);
    let results = db::search_nodes(&conn, &args.search, kind, args.limit).unwrap_or_else(|err| {
        eprintln!("Search failed: {err}");
        std::process::exit(1);
    });

    if args.json {
        let json = serde_json::to_string_pretty(&results).unwrap_or_default();
        println!("{json}");
        return;
    }

    if results.is_empty() {
        println!("No results found for \"{}\"", args.search);
        return;
    }

    println!("Search Results for \"{}\":\n", args.search);
    for result in results {
        let node = result.node;
        println!(
            "{:?} {} ({:.0}%)",
            node.kind,
            node.name,
            result.score * 100.0
        );
        println!("  {}:{}", node.file_path, node.start_line);
        if let Some(signature) = node.signature {
            println!("  {signature}");
        }
        println!();
    }
}

fn run_context(args: ContextArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let format = match args.format.to_ascii_lowercase().as_str() {
        "json" => ContextFormat::Json,
        _ => ContextFormat::Markdown,
    };

    let options = BuildContextOptions {
        max_nodes: Some(args.max_nodes),
        max_code_blocks: Some(args.max_code),
        max_code_block_size: None,
        include_code: Some(!args.no_code),
        format: Some(format),
        search_limit: None,
        traversal_depth: None,
        min_score: None,
    };

    let output =
        context::build_context(&project_root, &args.task, &options).unwrap_or_else(|err| {
            eprintln!("Failed to build context: {err}");
            std::process::exit(1);
        });

    println!("{output}");
}

fn run_hooks_install(path: Option<PathBuf>) {
    let project_root = resolve_project_root(path);
    let hooks = GitHooksManager::new(&project_root);
    let result = hooks.install_hook();
    if result.success {
        println!("{}", result.message);
        if let Some(backup) = result.backup_path {
            println!("Previous hook backed up at {}", backup.display());
        }
    } else {
        eprintln!("{}", result.message);
        std::process::exit(1);
    }
}

fn run_hooks_remove(path: Option<PathBuf>) {
    let project_root = resolve_project_root(path);
    let hooks = GitHooksManager::new(&project_root);
    let result = hooks.remove_hook();
    if result.success {
        println!("{}", result.message);
    } else {
        eprintln!("{}", result.message);
        std::process::exit(1);
    }
}

fn run_hooks_status(path: Option<PathBuf>) {
    let project_root = resolve_project_root(path);
    let hooks = GitHooksManager::new(&project_root);
    if !hooks.is_git_repository() {
        println!("Not a git repository.");
        return;
    }
    if hooks.is_hook_installed() {
        println!("Git hook is installed.");
    } else {
        println!("Git hook is not installed.");
    }
}

fn run_audit_docs(args: AuditDocsArgs) {
    let project_root = resolve_project_root(args.path);

    let report = match audit::audit_docs(&project_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to run doc audit: {e}");
            eprintln!("Make sure the project has been indexed (`coraline index`).");
            std::process::exit(1);
        }
    };

    if args.json {
        let stale: Vec<_> = report
            .stale_refs
            .iter()
            .take(args.limit)
            .map(|r| {
                serde_json::json!({
                    "reference": r.reference_name,
                    "doc_file": r.doc_file,
                    "section": r.doc_section,
                    "line": r.line,
                    "column": r.column
                })
            })
            .collect();
        let undoc: Vec<_> = report
            .undocumented_exports
            .iter()
            .take(args.limit)
            .map(|u| {
                serde_json::json!({
                    "name": u.name,
                    "qualified_name": u.qualified_name,
                    "kind": u.kind,
                    "file": u.file_path,
                    "line": u.start_line
                })
            })
            .collect();
        let out = serde_json::json!({
            "doc_files_indexed": report.doc_files_indexed,
            "doc_sections_indexed": report.doc_sections_indexed,
            "stale_refs": stale,
            "undocumented_exports": undoc
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    // Human-readable output
    println!(
        "Doc audit — {} file(s), {} section(s) indexed\n",
        report.doc_files_indexed, report.doc_sections_indexed
    );

    if !args.no_stale {
        let total = report.stale_refs.len();
        if total == 0 {
            println!("✓ No stale references found.");
        } else {
            println!(
                "Stale references ({total} total{})\n",
                if total > args.limit {
                    format!(", showing first {}", args.limit)
                } else {
                    String::new()
                }
            );
            for r in report.stale_refs.iter().take(args.limit) {
                println!(
                    "  {}:{} — `{}` (section: {})",
                    r.doc_file, r.line, r.reference_name, r.doc_section
                );
            }
            println!();
        }
    }

    if !args.no_undocumented {
        let total = report.undocumented_exports.len();
        if total == 0 {
            println!("✓ All exported symbols have documentation coverage.");
        } else {
            println!(
                "Undocumented exports ({total} total{})\n",
                if total > args.limit {
                    format!(", showing first {}", args.limit)
                } else {
                    String::new()
                }
            );
            for u in report.undocumented_exports.iter().take(args.limit) {
                println!(
                    "  {} {} — {} line {}",
                    u.kind, u.name, u.file_path, u.start_line
                );
            }
        }
    }
}

fn run_stats(args: StatsArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let stats = db::get_db_stats(&conn).unwrap_or_else(|err| {
        eprintln!("Failed to get stats: {err}");
        std::process::exit(1);
    });

    if args.json {
        let json = serde_json::to_string_pretty(&stats).unwrap_or_default();
        println!("{json}");
        return;
    }

    println!("Coraline Statistics\n");
    println!("Files:     {}", stats.file_count);
    println!("\nNodes:     {}", stats.node_count);
    println!("Edges:     {}", stats.edge_count);
    println!("Unresolved refs: {}", stats.unresolved_count);
}

fn run_callers(args: CallersArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let node = db::get_node_by_id(&conn, &args.node_id)
        .unwrap_or_else(|err| {
            eprintln!("Database error: {err}");
            std::process::exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("Node not found: {}", args.node_id);
            std::process::exit(1);
        });

    let edges = db::get_edges_by_target(&conn, &args.node_id, Some(EdgeKind::Calls), args.limit)
        .unwrap_or_else(|err| {
            eprintln!("Failed to get callers: {err}");
            std::process::exit(1);
        });

    if args.json {
        let results: Vec<_> = edges
            .iter()
            .filter_map(|e| db::get_node_by_id(&conn, &e.source).ok().flatten())
            .map(|n| serde_json::json!({ "id": n.id, "name": n.name, "kind": n.kind, "file": n.file_path, "line": n.start_line }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!("Callers of {} ({:?}):\n", node.name, node.kind);
    if edges.is_empty() {
        println!("  No callers found.");
        return;
    }
    for edge in &edges {
        if let Ok(Some(caller)) = db::get_node_by_id(&conn, &edge.source) {
            println!(
                "  {:?} {} ({}:{})",
                caller.kind, caller.name, caller.file_path, caller.start_line
            );
        }
    }
}

fn run_callees(args: CalleesArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let node = db::get_node_by_id(&conn, &args.node_id)
        .unwrap_or_else(|err| {
            eprintln!("Database error: {err}");
            std::process::exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("Node not found: {}", args.node_id);
            std::process::exit(1);
        });

    let edges = db::get_edges_by_source(&conn, &args.node_id, Some(EdgeKind::Calls), args.limit)
        .unwrap_or_else(|err| {
            eprintln!("Failed to get callees: {err}");
            std::process::exit(1);
        });

    if args.json {
        let results: Vec<_> = edges
            .iter()
            .filter_map(|e| db::get_node_by_id(&conn, &e.target).ok().flatten())
            .map(|n| serde_json::json!({ "id": n.id, "name": n.name, "kind": n.kind, "file": n.file_path, "line": n.start_line }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!("Callees of {} ({:?}):\n", node.name, node.kind);
    if edges.is_empty() {
        println!("  No callees found.");
        return;
    }
    for edge in &edges {
        if let Ok(Some(callee)) = db::get_node_by_id(&conn, &edge.target) {
            println!(
                "  {:?} {} ({}:{})",
                callee.kind, callee.name, callee.file_path, callee.start_line
            );
        }
    }
}

fn run_impact(args: ImpactArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let conn = db::open_database(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to open database: {err}");
        std::process::exit(1);
    });

    let node = db::get_node_by_id(&conn, &args.node_id)
        .unwrap_or_else(|err| {
            eprintln!("Database error: {err}");
            std::process::exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("Node not found: {}", args.node_id);
            std::process::exit(1);
        });

    // BFS outward from target edges (who directly or transitively uses this node)
    let mut visited = std::collections::HashSet::new();
    let mut frontier = vec![args.node_id.clone()];
    visited.insert(args.node_id.clone());

    for _ in 0..args.depth {
        let mut next = Vec::new();
        for id in &frontier {
            if let Ok(edges) = db::get_edges_by_target(&conn, id, None, 100) {
                for edge in edges {
                    if visited.insert(edge.source.clone()) {
                        next.push(edge.source);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    visited.remove(&args.node_id);

    if args.json {
        let results: Vec<_> = visited
            .iter()
            .filter_map(|id| db::get_node_by_id(&conn, id).ok().flatten())
            .map(|n| serde_json::json!({ "id": n.id, "name": n.name, "kind": n.kind, "file": n.file_path }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!(
        "Impact of {} ({:?}) — depth {}:\n",
        node.name, node.kind, args.depth
    );
    if visited.is_empty() {
        println!("  No dependents found.");
        return;
    }
    let mut affected: Vec<_> = visited
        .iter()
        .filter_map(|id| db::get_node_by_id(&conn, id).ok().flatten())
        .collect();
    affected.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
    });
    for n in &affected {
        println!(
            "  {:?} {} ({}:{})",
            n.kind, n.name, n.file_path, n.start_line
        );
    }
    println!("\n{} affected symbol(s)", affected.len());
}

fn run_config(args: ConfigArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    // Handle --migrate
    if args.migrate {
        if config::needs_migration(&project_root) {
            println!("Migrating config.json → config.toml...");
            match config::migrate_config(&project_root, true) {
                Ok(()) => {
                    println!("✓ Migration complete (config.json backed up as config.json.backup)");
                }
                Err(err) => {
                    eprintln!("Migration failed: {err}");
                    std::process::exit(1);
                }
            }
        } else if config::toml_config_path(&project_root).exists() {
            println!("Config already using config.toml (no migration needed)");
        } else {
            println!("No config.json found to migrate");
        }
        return;
    }

    // Handle --set section.key=value
    if let Some(set_expr) = &args.set {
        let parts: Vec<&str> = set_expr.splitn(2, '=').collect();
        let &[path_part, value_str] = parts.as_slice() else {
            eprintln!("Invalid --set format. Expected: section.key=value");
            std::process::exit(1);
        };
        let path_parts: Vec<&str> = path_part.splitn(2, '.').collect();
        let &[section, key] = path_parts.as_slice() else {
            eprintln!(
                "Invalid --set path. Expected: section.key=value (e.g. indexing.batch_size=50)"
            );
            std::process::exit(1);
        };

        let mut cfg = config::load_toml_config(&project_root).unwrap_or_else(|err| {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        });

        // Parse value as JSON for type flexibility
        let json_value: serde_json::Value = serde_json::from_str(value_str)
            .unwrap_or_else(|_| serde_json::Value::String(value_str.to_string()));

        let mut cfg_json = serde_json::to_value(&cfg).unwrap_or_default();
        if let Some(section_obj) = cfg_json.get_mut(section).and_then(|v| v.as_object_mut()) {
            section_obj.insert(key.to_string(), json_value.clone());
        } else {
            eprintln!("Unknown config section: {section}");
            std::process::exit(1);
        }

        cfg = serde_json::from_value(cfg_json).unwrap_or_else(|err| {
            eprintln!("Invalid value for {section}.{key}: {err}");
            std::process::exit(1);
        });

        config::save_toml_config(&project_root, &cfg).unwrap_or_else(|err| {
            eprintln!("Failed to save config: {err}");
            std::process::exit(1);
        });

        println!("Updated {section}.{key} = {json_value}");
        return;
    }

    let cfg = config::load_toml_config(&project_root).unwrap_or_else(|err| {
        eprintln!("Failed to load config: {err}");
        std::process::exit(1);
    });

    if args.json {
        let mut v = serde_json::to_value(&cfg).unwrap_or_default();
        if let Some(section) = &args.section {
            v = v
                .get(section.as_str())
                .cloned()
                .unwrap_or(serde_json::Value::Null);
        }
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }

    // Pretty-print TOML
    let toml_str = toml::to_string_pretty(&cfg).unwrap_or_else(|_| format!("{cfg:#?}"));
    if let Some(section) = &args.section {
        // Print only the requested section
        let section_header = format!("[{section}]");
        let mut in_section = false;
        for line in toml_str.lines() {
            if line.starts_with('[') {
                in_section = line == section_header;
            }
            if in_section {
                println!("{line}");
            }
        }
    } else {
        println!("{toml_str}");
    }
}

fn resolve_project_root(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn is_initialized(project_root: &Path) -> bool {
    // `.coraline/` alone isn't a reliable signal — other code (e.g. the
    // logger) can create it before `coraline init` has actually run.
    // `config.toml` is only ever written by a real init, so it's the
    // authoritative marker.
    config::toml_config_path(project_root).is_file()
}

fn create_coraline_dir(project_root: &Path) -> std::io::Result<()> {
    let dir = project_root.join(".coraline");
    std::fs::create_dir_all(&dir)?;
    let gitignore_path = dir.join(".gitignore");
    if !gitignore_path.exists() {
        let content = "# Coraline data files\n# These are local to each machine and should not be committed\n\n# Database\n*.db\n*.db-wal\n*.db-shm\n\n# Cache\ncache/\n\n# Logs\n*.log\n";
        std::fs::write(gitignore_path, content)?;
    }
    Ok(())
}

/// Braille spinner frames used for indeterminate / between-update progress.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Steady tick interval for the spinner glyph (~12 fps).
const SPINNER_TICK_MS: u64 = 80;

/// Build a styled `ProgressBar` that animates a braille spinner while showing a
/// counter and message. When stdout is not a TTY the bar falls back to a static
/// line that still updates on `set_message`.
#[expect(
    clippy::expect_used,
    reason = "templates are compile-time constants we control"
)]
fn spinner_bar(len: u64, template: &str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template(template)
            .expect("valid progress template")
            .tick_strings(SPINNER_FRAMES),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(SPINNER_TICK_MS));
    pb
}

/// Spinner for indeterminate operations (no known total, e.g. model download).
#[expect(
    clippy::expect_used,
    reason = "template is a compile-time constant we control"
)]
fn spinner_indefinite(message: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .expect("valid spinner template")
            .tick_strings(SPINNER_FRAMES),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(SPINNER_TICK_MS));
    pb
}

/// Push a fresh `IndexProgress` into an existing spinner bar, growing the bar
/// if the underlying total changed (e.g. moving from Scanning -> Parsing).
fn update_index_spinner(pb: &ProgressBar, progress: &extraction::IndexProgress) {
    let phase = match progress.phase {
        extraction::IndexPhase::Scanning => "Scanning",
        extraction::IndexPhase::Parsing => "Parsing",
        extraction::IndexPhase::Storing => "Storing",
        extraction::IndexPhase::Resolving => "Resolving",
    };
    if pb.length() != Some(progress.total as u64) {
        pb.set_length(progress.total as u64);
    }
    pb.set_position(progress.current as u64);
    let file = progress
        .current_file
        .as_ref()
        .map(|f| format!(" {f}"))
        .unwrap_or_default();
    pb.set_message(format!("{phase} {}{file}", pb.position()));
}

fn parse_node_kind(value: &str) -> Option<NodeKind> {
    match value.to_ascii_lowercase().as_str() {
        "file" => Some(NodeKind::File),
        "module" => Some(NodeKind::Module),
        "class" => Some(NodeKind::Class),
        "struct" => Some(NodeKind::Struct),
        "interface" => Some(NodeKind::Interface),
        "trait" => Some(NodeKind::Trait),
        "protocol" => Some(NodeKind::Protocol),
        "function" => Some(NodeKind::Function),
        "method" => Some(NodeKind::Method),
        "property" => Some(NodeKind::Property),
        "field" => Some(NodeKind::Field),
        "variable" => Some(NodeKind::Variable),
        "constant" => Some(NodeKind::Constant),
        "enum" => Some(NodeKind::Enum),
        "enum_member" => Some(NodeKind::EnumMember),
        "type_alias" => Some(NodeKind::TypeAlias),
        "namespace" => Some(NodeKind::Namespace),
        "parameter" => Some(NodeKind::Parameter),
        "import" => Some(NodeKind::Import),
        "export" => Some(NodeKind::Export),
        "route" => Some(NodeKind::Route),
        "component" => Some(NodeKind::Component),
        _ => None,
    }
}

fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };

    let mut extensions: Vec<std::ffi::OsString> = Vec::new();
    if cfg!(windows) {
        if let Some(pathext) = std::env::var_os("PATHEXT") {
            extensions = std::env::split_paths(&pathext)
                .map(std::path::PathBuf::into_os_string)
                .collect();
        }
        if extensions.is_empty() {
            extensions.push(std::ffi::OsString::from(".exe"));
        }
    }

    for dir in std::env::split_paths(&path) {
        let base = dir.join(name);
        if cfg!(windows) {
            if base.exists() && base.is_file() {
                return true;
            }
            for ext in &extensions {
                let candidate =
                    PathBuf::from(format!("{}{}", base.display(), ext.to_string_lossy()));
                if candidate.exists() && candidate.is_file() {
                    return true;
                }
            }
        } else if base.exists() && base.is_file() && is_executable(&base) {
            return true;
        }
    }

    false
}

fn is_executable(path: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            return metadata.permissions().mode() & 0o111 != 0;
        }
        false
    }

    #[cfg(not(unix))]
    {
        path.exists() && path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A bare `.coraline/` directory (e.g. left behind by logging, or any
    /// other incidental creator) must not count as "initialized" — only a
    /// real `coraline init` writes `config.toml`.
    #[test]
    fn is_initialized_false_for_bare_coraline_dir() -> TestResult {
        let temp_dir = tempfile::TempDir::new()?;
        let root = temp_dir.path();
        std::fs::create_dir_all(root.join(".coraline").join("logs"))?;

        assert!(!is_initialized(root));
        Ok(())
    }

    #[test]
    fn is_initialized_true_once_config_toml_exists() -> TestResult {
        let temp_dir = tempfile::TempDir::new()?;
        let root = temp_dir.path();
        let coraline_dir = root.join(".coraline");
        std::fs::create_dir_all(&coraline_dir)?;
        std::fs::write(coraline_dir.join("config.toml"), "")?;

        assert!(is_initialized(root));
        Ok(())
    }

    #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
    mod embeddings_tests {
        use super::*;

        #[test]
        fn model_state_absent_when_no_files() -> TestResult {
            let temp_dir = tempfile::TempDir::new()?;
            let state = doctor::compute_model_state(temp_dir.path(), vectors::DEFAULT_MODEL);
            assert_eq!(state, doctor::ModelState::Absent);
            Ok(())
        }

        #[test]
        fn model_state_present_picks_first_preferred_variant() -> TestResult {
            let temp_dir = tempfile::TempDir::new()?;
            std::fs::write(temp_dir.path().join("model_int8.onnx"), vec![0u8; 42])?;
            let state = doctor::compute_model_state(temp_dir.path(), vectors::DEFAULT_MODEL);
            assert_eq!(
                state,
                doctor::ModelState::Present {
                    name: "model_int8.onnx".to_string(),
                    size_bytes: 42,
                }
            );
            Ok(())
        }

        #[test]
        fn status_respects_model_dir_override() -> TestResult {
            let temp_dir = tempfile::TempDir::new()?;
            let root = temp_dir.path();
            let coraline_dir = root.join(".coraline");
            std::fs::create_dir_all(&coraline_dir)?;
            let custom_dir = root.join("custom-models");
            std::fs::create_dir_all(&custom_dir)?;
            let config = format!("[vectors]\nmodel_dir = \"{}\"\n", custom_dir.display());
            std::fs::write(coraline_dir.join("config.toml"), config)?;

            let (_, resolved) = doctor::resolve_status_model(root);
            assert_eq!(resolved, custom_dir);
            Ok(())
        }
    }
}

#[cfg(all(test, any(feature = "embeddings", feature = "embeddings-dynamic")))]
mod init_flag_tests {
    use super::init_model::*;

    #[test]
    fn model_present_is_noop() {
        let m = ModelInputs {
            model_present: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::NoOp);
        let m = ModelInputs {
            model_present: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::NoOp);
    }

    #[test]
    fn model_present_short_circuits_all_flags() {
        // Even with --embed and --yes, model present is NoOp.
        let m = ModelInputs {
            model_present: true,
            embed: true,
            yes: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::NoOp);
        // --no-embed is irrelevant when model is present.
        let m = ModelInputs {
            model_present: true,
            no_embed: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::NoOp);
    }

    #[test]
    fn no_embed_skips() {
        let m = ModelInputs {
            no_embed: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Skip);
        let m = ModelInputs {
            no_embed: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Skip);
    }

    #[test]
    fn embed_downloads_regardless_of_tty() {
        let m = ModelInputs {
            embed: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Download);
        let m = ModelInputs {
            embed: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Download);
    }

    #[test]
    fn yes_auto_downloads() {
        let m = ModelInputs {
            yes: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Download);
        let m = ModelInputs {
            yes: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Download);
    }

    #[test]
    fn no_embed_wins_over_yes() {
        let m = ModelInputs {
            no_embed: true,
            yes: true,
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Skip);
    }

    #[test]
    fn no_flags_tty_prompts() {
        let m = ModelInputs {
            is_tty: true,
            ..Default::default()
        };
        assert_eq!(decide_model_action(m), ModelAction::Prompt);
    }

    #[test]
    fn no_flags_non_tty_hints() {
        let m = ModelInputs::default();
        assert_eq!(decide_model_action(m), ModelAction::Hint);
    }
}
