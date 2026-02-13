use std::path::{Path, PathBuf};

use coraline::config;
use coraline::context;
use coraline::db;
use coraline::extraction;
use coraline::mcp::McpServer;
use coraline::memory;
use coraline::sync::GitHooksManager;
use coraline::types::NodeKind;
use coraline::types::{BuildContextOptions, ContextFormat};

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
    Query(QueryArgs),
    Context(ContextArgs),
    Hooks(HooksArgs),
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    path: Option<PathBuf>,
    #[arg(short = 'i', long = "index")]
    index: bool,
    #[arg(long = "no-hooks")]
    no_hooks: bool,
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

fn main() {
    let cli = Cli::parse();
    if matches!(cli.command, None | Some(Command::Install)) {
        run_installer();
        return;
    }

    let Some(command) = cli.command else {
        return;
    };

    match command {
        Command::Install => run_installer(),
        Command::Init(args) => run_init(args),
        Command::Index(args) => run_index(args),
        Command::Sync(args) => run_sync(args),
        Command::Status(args) => run_status(args),
        Command::Query(args) => run_query(args),
        Command::Context(args) => run_context(args),
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
    }
}

fn run_installer() {
    println!("Coraline installer (Rust rewrite)\n");
    let checks = [
        ("claude", "Claude Code"),
        ("openai", "OpenAI CLI"),
        ("oa", "OpenAI CLI (alt)"),
    ];

    for (bin, label) in checks {
        if which(bin) {
            println!("- {label}: found ({bin})");
        } else {
            println!("- {label}: not found ({bin})");
            print_install_hint(bin, label);
        }
    }

    if which("gh") {
        println!("- GitHub CLI: found (gh)");
        println!("  Check Copilot: run 'gh copilot --help'");
    } else if which("copilot-cli") {
        println!("- Copilot CLI: found (copilot-cli)");
    } else {
        println!("- Copilot CLI: not found (gh copilot or copilot-cli)");
        print_install_hint("copilot-cli", "Copilot CLI");
    }

    println!("\nInstaller actions will be added in the next pass.");
}

fn run_init(args: InitArgs) {
    let project_root = resolve_project_root(args.path);

    if is_initialized(&project_root) {
        eprintln!("Coraline already initialized in {}", project_root.display());
        return;
    }

    if let Err(err) = create_codegraph_dir(&project_root) {
        eprintln!("Failed to create .coraline directory: {err}");
        std::process::exit(1);
    }

    let cfg = config::create_default_config(&project_root);
    if let Err(err) = config::save_config(&project_root, &cfg) {
        eprintln!("Failed to write config: {err}");
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

    if args.index {
        run_index(IndexArgs {
            path: Some(project_root),
            force: false,
            quiet: false,
        });
    }
}

fn run_index(args: IndexArgs) {
    let project_root = resolve_project_root(args.path);

    if !is_initialized(&project_root) {
        eprintln!("Coraline not initialized in {}", project_root.display());
        std::process::exit(1);
    }

    let cfg = match config::load_config(&project_root) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };

    if !args.quiet {
        println!("Indexing project...\n");
    }

    let result = extraction::index_all(
        &project_root,
        &cfg,
        args.force,
        if args.quiet {
            None
        } else {
            Some(&print_progress)
        },
    )
    .unwrap_or_else(|err| {
        eprintln!("Indexing failed: {err}");
        std::process::exit(1);
    });

    if !args.quiet {
        clear_progress_line();
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

    let cfg = match config::load_config(&project_root) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("Failed to load config: {err}");
            std::process::exit(1);
        }
    };

    let result = extraction::sync(
        &project_root,
        &cfg,
        if args.quiet {
            None
        } else {
            Some(&print_progress)
        },
    )
    .unwrap_or_else(|err| {
        eprintln!("Sync failed: {err}");
        std::process::exit(1);
    });

    if !args.quiet {
        clear_progress_line();
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
    let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

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

fn resolve_project_root(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn is_initialized(project_root: &Path) -> bool {
    let dir = project_root.join(".coraline");
    dir.is_dir()
}

fn create_codegraph_dir(project_root: &Path) -> std::io::Result<()> {
    let dir = project_root.join(".coraline");
    std::fs::create_dir_all(&dir)?;
    let gitignore_path = dir.join(".gitignore");
    if !gitignore_path.exists() {
        let content = "# Coraline data files\n# These are local to each machine and should not be committed\n\n# Database\n*.db\n*.db-wal\n*.db-shm\n\n# Cache\ncache/\n\n# Logs\n*.log\n";
        std::fs::write(gitignore_path, content)?;
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn print_progress(progress: extraction::IndexProgress) {
    let phase = match progress.phase {
        extraction::IndexPhase::Scanning => "Scanning",
        extraction::IndexPhase::Parsing => "Parsing",
        extraction::IndexPhase::Storing => "Storing",
        extraction::IndexPhase::Resolving => "Resolving",
    };
    let file = progress
        .current_file
        .as_ref()
        .map(|f| format!(" {f}"))
        .unwrap_or_default();
    print!("\r{phase}: {}/{}{}", progress.current, progress.total, file);
}

fn clear_progress_line() {
    println!();
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

fn print_install_hint(bin: &str, label: &str) {
    match bin {
        "claude" => println!("  Install {label}: https://claude.ai/code"),
        "copilot-cli" => println!("  Install {label}: https://github.com/github/copilot-cli"),
        "openai" => println!("  Install {label}: https://github.com/openai/openai-cli"),
        _ => println!("  Install {label}: check vendor docs."),
    }
}
