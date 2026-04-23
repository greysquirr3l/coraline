#![allow(clippy::multiple_crate_versions)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use coraline::audit;
use coraline::config;
use coraline::context;
use coraline::db;
use coraline::extraction;
use coraline::logging;
use coraline::mcp::McpServer;
use coraline::memory;
use coraline::sync::GitHooksManager;
use coraline::types::NodeKind;
use coraline::types::{BuildContextOptions, ContextFormat, EdgeKind};
use coraline::update;
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
use coraline::vectors;
use indicatif::{ProgressBar, ProgressStyle};
use tracing::{debug, info};

use clap::{Args, Parser, Subcommand};

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
    Query(QueryArgs),
    Context(ContextArgs),
    Callers(CallersArgs),
    Callees(CalleesArgs),
    Impact(ImpactArgs),
    Config(ConfigArgs),
    Hooks(HooksArgs),
    Serve(ServeArgs),
    /// Check for available updates on crates.io.
    Update,
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
    /// ONNX variant to download when using `--download` (default: `model_int8.onnx`).
    #[arg(long = "variant", default_value = "model_int8.onnx")]
    variant: String,
    /// Skip the automatic sync check before embedding.
    #[arg(long = "skip-sync")]
    skip_sync: bool,
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
        /// ONNX variant filename to download.
        #[arg(long = "variant", default_value = "model_int8.onnx")]
        variant: String,
        /// Re-download even if the file already exists.
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    /// Show which model files are present in the model directory.
    Status,
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
        Command::Install | Command::Update => None,
    };
    let project_root = resolve_project_root(project_root_hint);
    // Don't create .coraline/logs/ before the init command runs — that would
    // cause is_initialized() to return true and block a fresh init.
    let log_root =
        if matches!(command, Command::Init(_)) && !project_root.join(".coraline").is_dir() {
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
                let mut server = McpServer::new(args.path);
                if let Err(err) = server.start() {
                    eprintln!("Failed to start MCP server: {err}");
                    std::process::exit(1);
                }
            } else {
                println!("Use --mcp to start the MCP server.");
            }
        }
        Command::Update => run_update(),
        Command::AuditDocs(args) => run_audit_docs(args),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Embed(args) => run_embed(&args),
        #[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
        Command::Model(args) => run_model(args),
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_model(args: ModelArgs) {
    let project_root = resolve_project_root(args.path);
    let cfg = config::load_toml_config(&project_root).unwrap_or_default();
    let model_dir = cfg
        .vectors
        .model_dir
        .map_or_else(|| vectors::default_model_dir(&project_root), PathBuf::from);

    match args.action {
        ModelAction::Download { variant, force } => {
            #[cfg(feature = "embeddings")]
            {
                if !args.quiet {
                    println!("Downloading {variant} into {} ...", model_dir.display());
                }
                if let Err(e) = vectors::download_model(&model_dir, &variant, !force, args.quiet) {
                    eprintln!("Download failed: {e}");
                    std::process::exit(1);
                }
                if !args.quiet {
                    println!("Done. Run `coraline embed` to generate embeddings.");
                }
            }
            #[cfg(not(feature = "embeddings"))]
            {
                let _ = (variant, force); // suppress unused warnings
                eprintln!("Model download is not available in this build.");
                eprintln!(
                    "This binary was built with `embeddings-dynamic`, which loads ONNX Runtime at runtime."
                );
                eprintln!();
                eprintln!("To use embeddings, manually download the model files:");
                eprintln!(
                    "  1. Download tokenizer.json from: {}",
                    vectors::tokenizer_url()
                );
                eprintln!(
                    "  2. Download model_int8.onnx from: {}",
                    vectors::model_url("model_int8.onnx")
                );
                eprintln!("  3. Place both files in: {}", model_dir.display());
                std::process::exit(1);
            }
        }
        ModelAction::Status => {
            println!("Model directory: {}", model_dir.display());
            println!();
            for name in vectors::MODEL_PREFERENCE_ORDER {
                let p = model_dir.join(name);
                if let Ok(meta) = std::fs::metadata(&p) {
                    println!("  {name:<30}  {:>6} MB  [present]", meta.len() / 1_000_000);
                } else {
                    println!("  {name:<30}  (not present)");
                }
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
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn embedding_model_dir(project_root: &Path) -> PathBuf {
    let cfg = config::load_toml_config(project_root).unwrap_or_default();
    cfg.vectors
        .model_dir
        .map_or_else(|| vectors::default_model_dir(project_root), PathBuf::from)
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn run_embed(args: &EmbedArgs) {
    let project_root = resolve_project_root(args.path.clone());

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    // Auto-sync: ensure the index is up to date before embedding.
    if args.skip_sync {
        if !args.quiet {
            eprintln!("Skipping sync (--skip-sync). Embeddings may be stale.");
        }
    } else {
        auto_sync_before_embed(&project_root, args.quiet);
    }

    // Auto-download model files if requested.
    #[cfg(feature = "embeddings")]
    if args.download {
        let model_dir = embedding_model_dir(&project_root);
        if !args.quiet {
            println!(
                "Downloading {} into {} ...",
                args.variant,
                model_dir.display()
            );
        }
        if let Err(e) = vectors::download_model(&model_dir, &args.variant, true, args.quiet) {
            eprintln!("Download failed: {e}");
            std::process::exit(1);
        }
    }
    #[cfg(not(feature = "embeddings"))]
    if args.download {
        eprintln!("Model download is not available in this build (embeddings-dynamic).");
        eprintln!("Please download the model files manually. See: coraline model download --help");
        std::process::exit(1);
    }

    let mut vm = match load_vector_manager_with_indicator(&project_root, args.quiet) {
        Ok(vm) => vm,
        Err(err) => {
            let model_dir = embedding_model_dir(&project_root);
            // Check whether the error is due to missing model files specifically.
            let no_model = vectors::find_model_file(&model_dir, None).is_err();
            if no_model {
                #[cfg(feature = "embeddings")]
                {
                    eprintln!("No embedding model found in {}.", model_dir.display());
                    if prompt_yes_no("Download model now? [Y/n]") {
                        if !args.quiet {
                            println!(
                                "Downloading {} into {} …",
                                args.variant,
                                model_dir.display()
                            );
                        }
                        if let Err(e) =
                            vectors::download_model(&model_dir, &args.variant, true, args.quiet)
                        {
                            eprintln!("Download failed: {e}");
                            std::process::exit(1);
                        }
                    } else {
                        eprintln!(
                            "Hint: run `coraline embed --download` or `coraline model download` to fetch the model."
                        );
                        std::process::exit(1);
                    }
                }
                #[cfg(not(feature = "embeddings"))]
                {
                    eprintln!("No embedding model found in {}.", model_dir.display());
                    eprintln!(
                        "This build does not support automatic download. Download the model files manually:"
                    );
                    eprintln!("  tokenizer.json  — {}", vectors::tokenizer_url());
                    eprintln!(
                        "  model_int8.onnx — {}",
                        vectors::model_url("model_int8.onnx")
                    );
                    eprintln!("Place both files in: {}", model_dir.display());
                    std::process::exit(1);
                }
            } else {
                eprintln!("Failed to load model: {err}");
                std::process::exit(1);
            }
            // Retry loading after download (only reached in the embeddings feature path).
            match load_vector_manager_with_indicator(&project_root, args.quiet) {
                Ok(vm) => vm,
                Err(e) => {
                    eprintln!("Failed to load model after download: {e}");
                    std::process::exit(1);
                }
            }
        }
    };

    embed_all_nodes(&project_root, args, &mut vm);
}

/// Prompt the user with a yes/no question. Returns `true` if the user answers
/// yes (or presses Enter, accepting the default of yes).
fn prompt_yes_no(question: &str) -> bool {
    eprint!("{question} ");
    let mut buf = String::new();
    if std::io::stdin().read_line(&mut buf).is_err() {
        return false;
    }
    let answer = buf.trim().to_ascii_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn load_vector_manager_from_project(
    project_root: &Path,
) -> std::io::Result<vectors::VectorManager> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vectors::VectorManager::from_project(project_root)
    }));
    std::panic::set_hook(previous_hook);

    match result {
        Ok(result) => result,
        Err(panic) => {
            let panic_msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "unknown ONNX runtime panic".to_string());

            let hint = if panic_msg.contains("libonnxruntime.dylib")
                || panic_msg.contains("Failed to load ONNX Runtime dylib")
            {
                "ONNX Runtime dynamic library could not be loaded. On macOS, ensure libonnxruntime.dylib is installed and discoverable via DYLD_LIBRARY_PATH or placed in a default loader path.".to_string()
            } else {
                "ONNX Runtime initialization panicked while loading the embedding model."
                    .to_string()
            };

            Err(std::io::Error::other(format!(
                "{hint} Panic details: {panic_msg}"
            )))
        }
    }
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn load_vector_manager_with_indicator(
    project_root: &Path,
    quiet: bool,
) -> std::io::Result<vectors::VectorManager> {
    if quiet {
        return load_vector_manager_from_project(project_root);
    }

    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        eprintln!("Loading embedding model (ONNX runtime + tokenizer)...");
        return load_vector_manager_from_project(project_root);
    }

    let spinner = file_spinner(quiet);
    spinner.set_message("Loading embedding model (ONNX runtime + tokenizer)…".to_string());
    let result = load_vector_manager_from_project(project_root);
    spinner.finish_and_clear();
    result
}

#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn embed_all_nodes(project_root: &Path, args: &EmbedArgs, vm: &mut vectors::VectorManager) {
    let conn = db::open_database(project_root).unwrap_or_else(|err| {
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

    let bar = file_spinner(args.quiet);

    let mut ok = 0usize;
    let mut skipped = 0usize;

    for node in &nodes {
        let target = if node.file_path.is_empty() {
            node.qualified_name.as_str()
        } else {
            node.file_path.as_str()
        };
        bar.set_message(format!("Embedding: {target}"));

        let text = vectors::node_embed_text(
            &node.name,
            &node.qualified_name,
            node.docstring.as_deref(),
            node.signature.as_deref(),
        );

        match vm.embed(&text) {
            Ok(embedding) => {
                if let Err(_err) =
                    vectors::store_embedding(&conn, &node.id, &embedding, vm.model_name())
                {
                    skipped += 1;
                } else {
                    ok += 1;
                }
            }
            Err(err) => {
                debug!(node = %node.name, error = %err, "embed failed for node");
                skipped += 1;
            }
        }
    }

    bar.finish_and_clear();
    if !args.quiet {
        println!("Embedded {ok}/{total} nodes ({skipped} skipped)");
    }
}

/// Check whether the index is stale and run sync automatically before embedding.
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
fn auto_sync_before_embed(project_root: &Path, quiet: bool) {
    let mut cfg = match config::load_config(project_root) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };
    if let Ok(toml_cfg) = config::load_toml_config(project_root) {
        config::apply_toml_to_code_graph(&mut cfg, &toml_cfg);
    }

    if !quiet {
        print!("Checking index freshness…");
    }

    let status = extraction::needs_sync(project_root, &cfg).unwrap_or_else(|err| {
        eprintln!("\nFailed to check sync status: {err}");
        std::process::exit(1);
    });

    if !status.is_stale() {
        if !quiet {
            println!(" up to date.");
        }
        return;
    }

    if !quiet {
        let total_changes = status.files_added + status.files_modified + status.files_removed;
        println!(" {total_changes} change(s) detected, syncing…");
    }

    let bar = file_spinner(quiet);
    let bar_cb = bar.clone();
    let cb = move |p: extraction::IndexProgress| {
        let phase = match p.phase {
            extraction::IndexPhase::Scanning => "Scanning",
            extraction::IndexPhase::Parsing => "Parsing",
            extraction::IndexPhase::Storing => "Storing",
            extraction::IndexPhase::Resolving => "Resolving",
        };
        let msg = p
            .current_file
            .map_or_else(|| phase.to_owned(), |f| format!("{phase}: {f}"));
        bar_cb.set_message(msg);
    };
    let result = extraction::sync(project_root, &cfg, if quiet { None } else { Some(&cb) })
        .unwrap_or_else(|err| {
            eprintln!("Auto-sync failed: {err}");
            std::process::exit(1);
        });

    bar.finish_and_clear();
    if !quiet {
        let total_changes = result.files_added + result.files_modified + result.files_removed;
        println!("Synced {total_changes} files before embedding.");
        if result.files_added > 0 {
            println!("  Added: {}", result.files_added);
        }
        if result.files_modified > 0 {
            println!("  Modified: {}", result.files_modified);
        }
        if result.files_removed > 0 {
            println!("  Removed: {}", result.files_removed);
        }
    }
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

fn run_update() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Coraline v{version} — checking for updates...\n");

    match update::check_for_update() {
        Ok(status) => update::print_update_status(&status),
        Err(e) => {
            eprintln!("Failed to check for updates: {e}");
            eprintln!();
            eprintln!("You can manually check: https://crates.io/crates/coraline");
            std::process::exit(1);
        }
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

fn file_spinner(quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let spinner = ProgressBar::new_spinner();
    #[allow(clippy::literal_string_with_formatting_args)]
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(&["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"]),
    );
    spinner.enable_steady_tick(Duration::from_millis(90));
    spinner
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
            maybe_prompt_model_download(&project_root);
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

    let cfg = config::create_default_config(&project_root);
    if let Err(err) = config::save_config(&project_root, &cfg) {
        eprintln!("Failed to write config: {err}");
        std::process::exit(1);
    }

    if let Err(err) = config::write_toml_template(&project_root) {
        eprintln!("Warning: Failed to write config.toml template: {err}");
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
    maybe_prompt_model_download(&project_root);

    if args.index {
        run_index(IndexArgs {
            path: Some(project_root),
            force: false,
            quiet: false,
        });
    }
}

/// After a fresh `init`, offer to download the embedding model when stdin is a
/// terminal.  If the user declines (or is non-interactive), we print a hint and
/// continue — all non-embedding tools remain fully functional.
#[cfg(feature = "embeddings")]
fn maybe_prompt_model_download(project_root: &Path) {
    use std::io::Write as _;

    let cfg = config::load_toml_config(project_root).unwrap_or_default();
    let model_dir = cfg
        .vectors
        .model_dir
        .map_or_else(|| vectors::default_model_dir(project_root), PathBuf::from);

    // Nothing to do if any model variant is already present.
    if vectors::MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| model_dir.join(name).exists())
    {
        return;
    }

    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!(
            "Tip: run `coraline model download` then `coraline embed` to enable semantic search."
        );
        return;
    }

    eprint!("Download embedding model for semantic search? (~137 MB) [Y/n] ");
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return;
    }
    let answer = input.trim();
    if answer.is_empty() || answer.eq_ignore_ascii_case("y") {
        println!("Downloading model into {} ...", model_dir.display());
        match vectors::download_model(&model_dir, "model_int8.onnx", true, false) {
            Ok(()) => println!("Done. Run `coraline embed` to generate embeddings."),
            Err(e) => {
                eprintln!("Model download failed: {e}");
                eprintln!("You can retry later with: coraline model download");
            }
        }
    } else {
        println!("Skipped. Run `coraline model download` later to enable semantic search.");
    }
}

/// For embeddings-dynamic builds, we can't auto-download but we can point users
/// to manual download instructions.
#[cfg(all(feature = "embeddings-dynamic", not(feature = "embeddings")))]
fn maybe_prompt_model_download(project_root: &Path) {
    let cfg = config::load_toml_config(project_root).unwrap_or_default();
    let model_dir = cfg
        .vectors
        .model_dir
        .map_or_else(|| vectors::default_model_dir(project_root), PathBuf::from);

    // Nothing to do if any model variant is already present.
    if vectors::MODEL_PREFERENCE_ORDER
        .iter()
        .any(|name| model_dir.join(name).exists())
    {
        return;
    }

    eprintln!("Tip: To enable semantic search, download the model files manually:");
    eprintln!(
        "  1. Download tokenizer.json from: {}",
        vectors::tokenizer_url()
    );
    eprintln!(
        "  2. Download model_int8.onnx from: {}",
        vectors::model_url("model_int8.onnx")
    );
    eprintln!("  3. Place both files in: {}", model_dir.display());
    eprintln!("  4. Run `coraline embed` to generate embeddings.");
}

fn run_index(args: IndexArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let mut cfg = match config::load_config(&project_root) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };
    if let Ok(toml_cfg) = config::load_toml_config(&project_root) {
        config::apply_toml_to_code_graph(&mut cfg, &toml_cfg);
    }

    let bar = file_spinner(args.quiet);
    let bar_cb = bar.clone();
    let index_cb = move |p: extraction::IndexProgress| {
        let phase = match p.phase {
            extraction::IndexPhase::Scanning => "Scanning",
            extraction::IndexPhase::Parsing => "Parsing",
            extraction::IndexPhase::Storing => "Storing",
            extraction::IndexPhase::Resolving => "Resolving",
        };
        let msg = p
            .current_file
            .map_or_else(|| phase.to_owned(), |f| format!("{phase}: {f}"));
        bar_cb.set_message(msg);
    };

    let result = extraction::index_all(
        &project_root,
        &cfg,
        args.force,
        if args.quiet { None } else { Some(&index_cb) },
    )
    .unwrap_or_else(|err| {
        eprintln!("Indexing failed: {err}");
        std::process::exit(1);
    });

    bar.finish_and_clear();
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

    let mut cfg = match config::load_config(&project_root) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };
    if let Ok(toml_cfg) = config::load_toml_config(&project_root) {
        config::apply_toml_to_code_graph(&mut cfg, &toml_cfg);
    }

    let bar = file_spinner(args.quiet);
    let bar_cb = bar.clone();
    let sync_cb = move |p: extraction::IndexProgress| {
        let phase = match p.phase {
            extraction::IndexPhase::Scanning => "Scanning",
            extraction::IndexPhase::Parsing => "Parsing",
            extraction::IndexPhase::Storing => "Storing",
            extraction::IndexPhase::Resolving => "Resolving",
        };
        let msg = p
            .current_file
            .map_or_else(|| phase.to_owned(), |f| format!("{phase}: {f}"));
        bar_cb.set_message(msg);
    };

    let result = extraction::sync(
        &project_root,
        &cfg,
        if args.quiet { None } else { Some(&sync_cb) },
    )
    .unwrap_or_else(|err| {
        eprintln!("Sync failed: {err}");
        std::process::exit(1);
    });

    bar.finish_and_clear();
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

    let edges =
        db::get_edges_by_target(&conn, &args.node_id, Some(EdgeKind::Calls), args.limit * 2)
            .unwrap_or_else(|err| {
                eprintln!("Failed to get callers: {err}");
                std::process::exit(1);
            });

    if args.json {
        let results: Vec<_> = edges
            .iter()
            .filter_map(|e| {
                db::get_node_by_id(&conn, &e.source).ok().flatten().and_then(|caller| {
                    // Validate crate boundary
                    db::is_valid_call_edge(&conn, &caller, &node).ok().and_then(|valid| {
                        if valid {
                            Some(serde_json::json!({ "id": caller.id, "name": caller.name, "kind": caller.kind, "file": caller.file_path, "line": caller.start_line }))
                        } else {
                            None
                        }
                    })
                })
            })
            .take(args.limit)
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!("Callers of {} ({:?}):\n", node.name, node.kind);
    let mut printed = 0;
    for edge in &edges {
        if printed >= args.limit {
            break;
        }
        if let Ok(Some(caller)) = db::get_node_by_id(&conn, &edge.source) {
            // Validate crate boundary
            if matches!(db::is_valid_call_edge(&conn, &caller, &node), Ok(true)) {
                println!(
                    "  {:?} {} ({}:{})",
                    caller.kind, caller.name, caller.file_path, caller.start_line
                );
                printed += 1;
            }
        }
    }
    if printed == 0 {
        println!("  No callers found.");
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

    let edges =
        db::get_edges_by_source(&conn, &args.node_id, Some(EdgeKind::Calls), args.limit * 2)
            .unwrap_or_else(|err| {
                eprintln!("Failed to get callees: {err}");
                std::process::exit(1);
            });

    if args.json {
        let results: Vec<_> = edges
            .iter()
            .filter_map(|e| {
                db::get_node_by_id(&conn, &e.target).ok().flatten().and_then(|callee| {
                    // Validate crate boundary
                    db::is_valid_call_edge(&conn, &node, &callee).ok().and_then(|valid| {
                        if valid {
                            Some(serde_json::json!({ "id": callee.id, "name": callee.name, "kind": callee.kind, "file": callee.file_path, "line": callee.start_line }))
                        } else {
                            None
                        }
                    })
                })
            })
            .take(args.limit)
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!("Callees of {} ({:?}):\n", node.name, node.kind);
    let mut printed = 0;
    for edge in &edges {
        if printed >= args.limit {
            break;
        }
        if let Ok(Some(callee)) = db::get_node_by_id(&conn, &edge.target) {
            // Validate crate boundary
            if matches!(db::is_valid_call_edge(&conn, &node, &callee), Ok(true)) {
                println!(
                    "  {:?} {} ({}:{})",
                    callee.kind, callee.name, callee.file_path, callee.start_line
                );
                printed += 1;
            }
        }
    }
    if printed == 0 {
        println!("  No callees found.");
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
    let dir = project_root.join(".coraline");
    dir.is_dir()
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
