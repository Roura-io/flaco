//! flaco-v2 — unified runtime binary.
//!
//! Usage:
//!   flaco-v2 serve           # start slack + tui in one process (default)
//!   flaco-v2 tui             # run the shiny TUI
//!   flaco-v2 slack           # start only the Slack adapter
//!   flaco-v2 ask "<text>"    # one-shot: send message, print reply, exit
//!   flaco-v2 research "<t>"  # one-shot research
//!
//! All surfaces share a single Runtime backed by the same SQLite memory file
//! (default ~/infra/flaco.db, override with --db).
//!
//! NOTE: 2026-04-14 — web UI dropped. elGordo simplified scope to TUI + remote
//! TUI (via SSH) + Slack. Removing the web surface means one less thing to
//! maintain, one less port to expose, one less attack surface, and one less
//! set of conventions to keep in sync. The `flaco-web` crate still exists in
//! the tree but is excluded from the workspace and not built by any binary.

mod doctor;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flaco_config::Config;
use flaco_core::features::Features;
use flaco_core::memory::Memory;
use flaco_core::ollama::OllamaClient;
use flaco_core::persona::PersonaRegistry;
use flaco_core::runtime::{Runtime, Surface};
use flaco_core::tools::bash::Bash;
use flaco_core::tools::fs_rw::{FsRead, FsWrite};
use flaco_core::tools::github::{GitHubClient, GithubCreatePr};
use flaco_core::tools::jira::{JiraClient, JiraCreateIssue};
use flaco_core::tools::memory_tool::{ListMemories, Recall, Remember};
use flaco_core::tools::research::Research;
use flaco_core::tools::scaffold::Scaffold;
use flaco_core::tools::shortcut::CreateShortcut;
use flaco_core::tools::slack_post::SlackPost;
use flaco_core::tools::weather::Weather;
use flaco_core::tools::web::{WebFetch, WebSearch};
use flaco_core::tools::ToolRegistry;

#[derive(Debug, Parser)]
#[command(name = "flaco-v2", about = "flacoAi — powered by Roura.io")]
struct Cli {
    /// Path to the TOML config file. Falls back to $FLACO_CONFIG_PATH,
    /// /opt/homebrew/etc/flaco/config.toml, ~/.config/flaco/config.toml,
    /// then built-in defaults.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override the config's SQLite path.
    #[arg(long)]
    db: Option<String>,

    /// Default user id for single-user surfaces (TUI).
    #[arg(long, default_value = "chris")]
    user: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the Slack adapter (default — TUI is now invoked explicitly).
    Serve,
    /// Run only the Slack Socket Mode adapter.
    Slack,
    /// Run the shiny TUI.
    Tui,
    /// Send a single message and print the reply.
    Ask { text: String },
    /// Run a one-shot research query.
    Research { topic: String },
    /// Generate a Siri Shortcut.
    Shortcut { name: String, description: String },
    /// Scaffold a new project from an idea.
    Scaffold {
        idea: String,
        #[arg(long, default_value = "FLACO")]
        project_key: String,
    },
    /// Generate a Morning Brief from memory + open Jira tickets.
    Brief,
    /// Import markdown memory files as flaco facts.
    /// Points at any directory of *.md files (one fact per file) and inserts
    /// each body as a memory under the default user.
    ImportMemory {
        #[arg(long, default_value = "~/infra/flaco-memory-seed")]
        dir: String,
    },
    /// Print a JSON snapshot of the runtime state (tools, model, counts).
    Status,
    /// Snapshot the SQLite memory db via VACUUM INTO into cfg.backup.directory.
    /// Prunes snapshots older than cfg.backup.retention_days. Verifies the new
    /// snapshot opens cleanly before returning. Intended to be driven by the
    /// io.roura.flaco.backup launchd agent but also safe to run by hand.
    Backup,
    /// Run a health check: config, db, FTS5, Ollama, backup freshness,
    /// disk free space, launchd supervisors. Exit non-zero on any failure.
    Doctor,
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

fn build_runtime(cfg: &Config, user: &str) -> Result<(Arc<Runtime>, Arc<Features>)> {
    let db_path = &cfg.paths.db;
    let memory = Memory::open(db_path).with_context(|| format!("open {}", db_path.display()))?;
    // Ollama client honors FLACO_OLLAMA_URL + FLACO_MODEL env vars (which
    // Config::load already copied from the env). Pass config-driven values
    // via env so the OllamaClient constructor picks them up.
    std::env::set_var("OLLAMA_HOST", cfg.ollama.base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split(':').next().unwrap_or("127.0.0.1"));
    if let Some(port) = cfg.ollama.base_url.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
        std::env::set_var("OLLAMA_PORT", port.to_string());
    }
    std::env::set_var("FLACO_MODEL", &cfg.ollama.default_model);
    let ollama = OllamaClient::from_env();
    let personas = PersonaRegistry::defaults();

    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(Bash::new()));
    reg.register(Arc::new(FsRead));
    reg.register(Arc::new(FsWrite));
    reg.register(Arc::new(WebSearch::new()));
    reg.register(Arc::new(WebFetch::new()));
    reg.register(Arc::new(Weather::new()));
    reg.register(Arc::new(Research::new(ollama.clone())));
    if let Some(sp) = SlackPost::from_env() {
        reg.register(Arc::new(sp));
    }
    if let Some(jira) = JiraClient::from_env() {
        reg.register(Arc::new(JiraCreateIssue { client: jira.clone() }));
        reg.register(Arc::new(Scaffold { jira: Some(jira) }));
    } else {
        reg.register(Arc::new(Scaffold { jira: None }));
        tracing::warn!("Jira not configured — jira_create_issue disabled");
    }
    if let Some(gh) = GitHubClient::from_env() {
        reg.register(Arc::new(GithubCreatePr { client: gh }));
    } else {
        tracing::warn!("GitHub not configured — github_create_pr disabled");
    }
    reg.register(Arc::new(CreateShortcut::new(&cfg.paths.shortcuts_dir)));
    reg.register(Arc::new(Remember { memory: memory.clone(), default_user: user.into() }));
    reg.register(Arc::new(Recall { memory: memory.clone(), default_user: user.into() }));
    reg.register(Arc::new(ListMemories { memory: memory.clone(), default_user: user.into() }));
    reg.register(Arc::new(flaco_core::tools::save_to_unas::SaveToUnas));

    tracing::info!("registered tools: {:?}", reg.names());

    let runtime = Arc::new(Runtime::new(memory.clone(), ollama.clone(), reg, personas));
    let features = Arc::new(Features::new(memory, ollama));
    Ok((runtime, features))
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenv_load();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,flaco_core=debug,flaco_slack_v2=debug")),
        )
        .init();

    let cli = Cli::parse();

    // Load config first. Precedence: --config flag > FLACO_CONFIG_PATH env
    // > /opt/homebrew/etc/flaco/config.toml > ~/.config/flaco/config.toml
    // > built-in defaults. Per-field env vars (FLACO_DB_PATH, FLACO_WEB_PORT,
    // FLACO_OLLAMA_URL, FLACO_MODEL, FLACO_TIER) override the file.
    let mut cfg = Config::load(cli.config.as_deref())
        .context("failed to load flaco-config")?;

    // CLI flags override env + file, for one-shot debugging.
    if let Some(db) = &cli.db {
        cfg.paths.db = expand_home(db);
    }

    // Ensure the DB dir exists before we try to open it.
    if let Some(parent) = cfg.paths.db.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Lightweight subcommands that don't need a runtime/Memory handle —
    // specifically, anything that would cause Rust and an external
    // sqlite3 process to fight over the same db connection, or anything
    // that needs to probe the system rather than serve it.
    if matches!(cli.command, Some(Command::Backup)) {
        return run_backup(&cfg);
    }
    if matches!(cli.command, Some(Command::Doctor)) {
        return doctor::run(&cfg);
    }

    let (runtime, features) = build_runtime(&cfg, &cli.user)?;
    tracing::info!(
        "flaco-v2 up · db={} · source={:?}",
        cfg.paths.db.display(),
        cfg.source()
    );

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve_slack(runtime, features).await,
        Command::Slack => serve_slack(runtime, features).await,
        Command::Tui => flaco_tui_v2::run(runtime, features).await,
        Command::Ask { text } => {
            // Intent router first — `flaco-v2 ask "clear"` and similar
            // natural-language meta-commands skip the LLM loop.
            if let Some(intent) = flaco_core::intent::detect(&text) {
                let reply = flaco_core::intent::dispatch(
                    intent,
                    &runtime,
                    &features,
                    &Surface::Cli,
                    &cli.user,
                )
                .await?;
                println!("{reply}");
                return Ok(());
            }
            let session = runtime.session(&Surface::Cli, &cli.user)?;
            let reply = runtime.handle_turn(&session, &text, None).await?;
            println!("{reply}");
            Ok(())
        }
        Command::Research { topic } => {
            let r = features.research(&topic).await?;
            println!("{}", r.to_markdown());
            Ok(())
        }
        Command::Shortcut { name, description } => {
            let r = features.create_shortcut(&name, &description).await?;
            println!("{}", r.output);
            Ok(())
        }
        Command::Scaffold { idea, project_key } => {
            let r = features.scaffold(&idea, &project_key, None).await?;
            println!("{}", r.output);
            Ok(())
        }
        Command::Brief => {
            let b = features.morning_brief(&cli.user).await?;
            println!("{}", b.markdown);
            if !b.issues.is_empty() {
                println!();
                println!("Open tickets ({}):", b.issues.len());
                for i in &b.issues {
                    let pri = i.priority.as_deref().unwrap_or("—");
                    println!("  {} [{}] ({}, {}) — {}", i.key, i.kind, i.status, pri, i.summary);
                }
            }
            Ok(())
        }
        Command::ImportMemory { dir } => import_memory(&dir, &runtime, &cli.user).await,
        Command::Backup => unreachable!("Backup handled before build_runtime"),
        Command::Doctor => unreachable!("Doctor handled before build_runtime"),
        Command::Status => {
            let tools = runtime.tools.names();
            let memories = runtime.memory.all_facts(&cli.user, 10_000)?.len();
            let conversations = runtime.memory.list_conversations(10_000)?.len();
            let snap = serde_json::json!({
                "version": "flaco-v2",
                "model": runtime.ollama.model(),
                "db": cfg.paths.db.to_string_lossy(),
                "shortcuts_dir": cfg.paths.shortcuts_dir.to_string_lossy(),
                "ollama_base": cfg.ollama.base_url,
                "tier": cfg.tools.tier.as_str(),
                "config_source": format!("{:?}", cfg.source()),
                "default_user": cli.user,
                "tools": tools,
                "memories": memories,
                "conversations": conversations,
            });
            println!("{}", serde_json::to_string_pretty(&snap)?);
            Ok(())
        }
    }
}

async fn import_memory(dir: &str, runtime: &Arc<Runtime>, user: &str) -> Result<()> {
    let path = expand_home(dir);
    if !path.exists() {
        anyhow::bail!("seed dir not found: {}", path.display());
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(&path)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("md") { continue; }
        if p.file_name().and_then(|s| s.to_str()) == Some("MEMORY.md") { continue; }
        let content = std::fs::read_to_string(&p)?;
        // Strip frontmatter + use everything else as the fact body.
        let body = strip_frontmatter(&content).trim().to_string();
        if body.is_empty() { continue; }
        let kind = kind_from_filename(p.file_name().and_then(|s| s.to_str()).unwrap_or(""));
        let id = runtime.memory.remember_fact(user, &kind, &body, None)?;
        count += 1;
        println!("seeded #{id} [{kind}] {}", p.file_name().unwrap().to_string_lossy());
    }
    println!("total: {count} memories seeded for user '{user}'");
    Ok(())
}

fn strip_frontmatter(text: &str) -> &str {
    if !text.starts_with("---") { return text; }
    let rest = &text[3..];
    if let Some(end) = rest.find("---") {
        return &rest[end + 3..];
    }
    text
}

fn kind_from_filename(name: &str) -> String {
    if name.starts_with("user_") { "user".into() }
    else if name.starts_with("feedback_") { "preference".into() }
    else if name.starts_with("project_") { "project".into() }
    else if name.starts_with("reference_") { "reference".into() }
    else { "fact".into() }
}

async fn serve_slack(runtime: Arc<Runtime>, features: Arc<Features>) -> Result<()> {
    match flaco_slack_v2::SlackConfig::from_env() {
        Ok(cfg) => {
            let adapter = flaco_slack_v2::SlackAdapter::new(runtime, features, cfg);
            adapter.run().await?;
            Ok(())
        }
        Err(e) => {
            tracing::warn!("slack disabled (missing env: {e})");
            // sleep forever so the tokio::select on serve_all doesn't immediately
            // fail when slack isn't configured
            std::future::pending::<()>().await;
            Ok(())
        }
    }
}

/// `flaco-v2 backup` implementation.
///
/// Shells out to the system `sqlite3` binary for `VACUUM INTO` because:
///   1. It's always installed on macOS.
///   2. `VACUUM INTO` is atomic, online, takes a shared lock that doesn't
///      block the running flaco-v2 web process, and produces a compact copy.
///   3. Keeping the backup logic out of the Rust binary means it still runs
///      even if the main process is wedged or broken.
///
/// After writing the snapshot, we prune files older than
/// `cfg.backup.retention_days` (0 disables pruning) and then re-open the
/// new file with `SELECT count(*) FROM memories` to prove it's not corrupt.
fn run_backup(cfg: &flaco_config::Config) -> Result<()> {
    use std::process::Command as PC;
    use std::time::{SystemTime, UNIX_EPOCH};

    let db = &cfg.paths.db;
    let dest_dir = &cfg.backup.directory;
    if !db.exists() {
        anyhow::bail!("db path does not exist: {}", db.display());
    }
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("mkdir -p {}", dest_dir.display()))?;

    // flaco-YYYYMMDD-HHMMSS.db — sortable and human-readable.
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let tm = time_fmt(now);
    let snapshot = dest_dir.join(format!("flaco-{tm}.db"));

    tracing::info!("backup: VACUUM INTO {}", snapshot.display());
    // Retry the VACUUM INTO a handful of times if SQLite is momentarily busy.
    // Happens most often during a WAL checkpoint right after a write burst.
    let mut attempts = 0;
    let out = loop {
        attempts += 1;
        // Make sure stale artefacts from a failed previous attempt are gone,
        // otherwise sqlite3 refuses to overwrite the destination.
        let _ = std::fs::remove_file(&snapshot);
        let journal = snapshot.with_extension("db-journal");
        let _ = std::fs::remove_file(&journal);

        let result = PC::new("sqlite3")
            .arg(db)
            .arg(format!(
                "PRAGMA busy_timeout=15000; VACUUM INTO '{}'",
                snapshot.display()
            ))
            .output()
            .context("run sqlite3 VACUUM INTO")?;
        if result.status.success() {
            break result;
        }
        let err = String::from_utf8_lossy(&result.stderr);
        if attempts < 5 && (err.contains("database is locked") || err.contains("SQLITE_BUSY")) {
            tracing::warn!("backup: sqlite3 busy (attempt {attempts}/5), retrying in 2s");
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        anyhow::bail!("sqlite3 VACUUM INTO failed after {attempts} attempts: {err}");
    };
    let _ = out;  // we only needed the status

    // Verify the new snapshot opens cleanly.
    let verify = PC::new("sqlite3")
        .arg(&snapshot)
        .arg("SELECT count(*) FROM memories")
        .output()
        .context("verify snapshot")?;
    if !verify.status.success() {
        anyhow::bail!(
            "verify read from snapshot failed: {}",
            String::from_utf8_lossy(&verify.stderr)
        );
    }
    let row_count = String::from_utf8_lossy(&verify.stdout).trim().to_string();

    // Prune files older than retention_days.
    let mut pruned = 0usize;
    if cfg.backup.retention_days > 0 {
        let cutoff_secs = cfg.backup.retention_days as u64 * 86_400;
        for entry in std::fs::read_dir(dest_dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
            if !name.starts_with("flaco-") || !name.ends_with(".db") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = SystemTime::now().duration_since(modified) {
                        if age.as_secs() > cutoff_secs {
                            let _ = std::fs::remove_file(&path);
                            pruned += 1;
                        }
                    }
                }
            }
        }
    }

    println!("backup ok: {} ({} memories, pruned {})", snapshot.display(), row_count, pruned);
    Ok(())
}

/// Format a UNIX timestamp as `YYYYMMDD-HHMMSS` without pulling in `chrono`.
/// Good enough for filenames; not timezone-aware (UTC).
fn time_fmt(unix_secs: u64) -> String {
    // Days since 1970-01-01, then Zeller-ish arithmetic. Shorter than it looks.
    let secs = unix_secs % 60;
    let mins = (unix_secs / 60) % 60;
    let hours = (unix_secs / 3600) % 24;
    let mut days = unix_secs / 86_400;
    let mut year: u64 = 1970;
    loop {
        let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_lengths = [31u64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && days >= month_lengths[month] {
        days -= month_lengths[month];
        month += 1;
    }
    let day = days + 1;
    format!("{year:04}{:02}{day:02}-{hours:02}{mins:02}{secs:02}", month + 1)
}

/// Minimal .env loader — just enough so the binary can be run from any
/// directory without needing an external tool. Checks CWD, the canonical
/// ~/infra/flaco-v2.env install path, and the flacoAi project parents.
fn dotenv_load() -> Result<()> {
    let mut candidates: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from(".env"),
        std::path::PathBuf::from("../.env"),
        std::path::PathBuf::from("../../.env"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        candidates.push(home.join("infra/flaco-v2.env"));
        candidates.push(home.join(".flaco.env"));
    }
    for candidate in candidates {
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    if std::env::var_os(k).is_none() {
                        std::env::set_var(k, v);
                    }
                }
            }
            break;
        }
    }
    Ok(())
}
