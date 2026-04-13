//! flaco-v2 — unified runtime binary.
//!
//! Usage:
//!   flaco-v2 serve           # start web + slack in one process (default)
//!   flaco-v2 tui             # run the shiny TUI
//!   flaco-v2 web             # start only the web UI
//!   flaco-v2 slack           # start only the Slack adapter
//!   flaco-v2 ask "<text>"    # one-shot: send message, print reply, exit
//!   flaco-v2 research "<t>"  # one-shot research
//!
//! All surfaces share a single Runtime backed by the same SQLite memory file
//! (default ~/infra/flaco.db, override with --db).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
use flaco_web::AppState;

#[derive(Debug, Parser)]
#[command(name = "flaco-v2", about = "flacoAi v2 — unified brain")]
struct Cli {
    /// Path to the SQLite memory database.
    #[arg(long, default_value = "~/infra/flaco.db")]
    db: String,

    /// Port for the web UI.
    #[arg(long, default_value_t = 3031)]
    web_port: u16,

    /// Default user id for single-user surfaces (TUI, web).
    #[arg(long, default_value = "chris")]
    user: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run web + slack together (default).
    Serve,
    /// Run only the web UI.
    Web,
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
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

fn build_runtime(db_path: &PathBuf, user: &str) -> Result<(Arc<Runtime>, Arc<Features>)> {
    let memory = Memory::open(db_path).with_context(|| format!("open {}", db_path.display()))?;
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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    reg.register(Arc::new(CreateShortcut::new(home.join("Downloads/flaco-shortcuts"))));
    reg.register(Arc::new(Remember { memory: memory.clone(), default_user: user.into() }));
    reg.register(Arc::new(Recall { memory: memory.clone(), default_user: user.into() }));
    reg.register(Arc::new(ListMemories { memory: memory.clone(), default_user: user.into() }));

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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,flaco_core=debug,flaco_web=debug,flaco_slack_v2=debug")),
        )
        .init();

    let cli = Cli::parse();
    let db_path = expand_home(&cli.db);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let (runtime, features) = build_runtime(&db_path, &cli.user)?;
    tracing::info!("flaco-v2 up · db={}", db_path.display());

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve_all(runtime, features, cli.web_port, cli.user).await,
        Command::Web => serve_web(runtime, features, cli.web_port, cli.user).await,
        Command::Slack => serve_slack(runtime, features).await,
        Command::Tui => flaco_tui_v2::run(runtime, features).await,
        Command::Ask { text } => {
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
        Command::Status => {
            let tools = runtime.tools.names();
            let memories = runtime.memory.all_facts(&cli.user, 10_000)?.len();
            let conversations = runtime.memory.list_conversations(10_000)?.len();
            let snap = serde_json::json!({
                "version": "flaco-v2",
                "model": runtime.ollama.model(),
                "db": db_path.to_string_lossy(),
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

async fn serve_all(
    runtime: Arc<Runtime>,
    features: Arc<Features>,
    web_port: u16,
    user: String,
) -> Result<()> {
    let web_handle = tokio::spawn(serve_web(runtime.clone(), features.clone(), web_port, user));
    let slack_handle = tokio::spawn(serve_slack(runtime, features));

    tokio::select! {
        r = web_handle => { r??; }
        r = slack_handle => { r??; }
    }
    Ok(())
}

async fn serve_web(
    runtime: Arc<Runtime>,
    features: Arc<Features>,
    port: u16,
    user: String,
) -> Result<()> {
    let state = AppState { runtime, features, default_user: user };
    let app = flaco_web::router(state);
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("flaco-web listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
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
