use std::path::{Path, PathBuf};

use coraline::config;
use coraline::context;
use coraline::db;
use coraline::extraction;
use coraline::logging;
use coraline::mcp::McpServer;
use coraline::memory;
use coraline::sync::GitHooksManager;
use coraline::types::NodeKind;
use coraline::types::{BuildContextOptions, ContextFormat};
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
        Command::Install => None,
    };
    let project_root = resolve_project_root(project_root_hint);
    let _log_guard = logging::init(Some(&project_root));
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
        db::get_edges_by_target(&conn, &args.node_id, None, args.limit).unwrap_or_else(|err| {
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

    let edges =
        db::get_edges_by_source(&conn, &args.node_id, None, args.limit).unwrap_or_else(|err| {
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

    // Handle --set section.key=value
    if let Some(set_expr) = &args.set {
        let parts: Vec<&str> = set_expr.splitn(2, '=').collect();
        if parts.len() != 2 {
            eprintln!("Invalid --set format. Expected: section.key=value");
            std::process::exit(1);
        }
        let (path_part, value_str) = (parts[0], parts[1]);
        let path_parts: Vec<&str> = path_part.splitn(2, '.').collect();
        if path_parts.len() != 2 {
            eprintln!(
                "Invalid --set path. Expected: section.key=value (e.g. indexing.batch_size=50)"
            );
            std::process::exit(1);
        }
        let (section, key) = (path_parts[0], path_parts[1]);

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
