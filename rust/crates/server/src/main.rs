use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use channels::agents::{load_agents_from_dir, Agent};
use channels::commands::{load_commands_from_dir, Command};
use channels::gateway::{ChannelPersona, Gateway, GatewayConfig};
use channels::inference::{call_ollama, claude_check, needs_web_search, web_search, CheckResult};
use channels::rules::{load_rules_from_dir, Rule};
use channels::skills::{load_skills_from_dir, Skill};

/// Single-host PID lock. Written on startup, removed on graceful shutdown
/// via `Drop`. If the lock already exists AND the PID is still alive, we
/// exit with a clear error instead of starting a second flacoai-server on
/// the same host — which was the root cause of the morning's "two PIDs
/// racing for Slack events" bug. Cross-host duplication is prevented by
/// deployment hygiene (only launchd starts the binary, one host per deploy).
struct LockFile {
    path: PathBuf,
}

impl LockFile {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        // If a lock already exists, check whether the holder is still alive.
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    if is_pid_alive(pid) {
                        return Err(format!(
                            "flacoai-server already running with PID {pid} (lock: {})",
                            path.display()
                        ));
                    }
                }
            }
            // Stale lock (PID is dead). Clean it up and continue.
            let _ = std::fs::remove_file(&path);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, std::process::id().to_string())
            .map_err(|e| format!("write lock: {e}"))?;
        Ok(Self { path })
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// POSIX-y "is this PID alive" check via `kill -0 <pid>`.
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn lock_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".flaco/server.lock")
}

/// Load the agent registry at startup. Preferred source is
/// `$HOME/.flaco/agents/` so users can override or add agents without
/// rebuilding the binary. If that directory does not exist, fall back to
/// the in-repo `$CARGO_MANIFEST_DIR/../../../agents/` folder so a clean
/// dev checkout picks up the baseline agents from the repo without any
/// per-host setup. If neither exists (shipped binary on a fresh host
/// with no `~/.flaco/agents/` yet), start with an empty registry and
/// warn — a missing registry should not abort startup.
fn load_agent_registry() -> HashMap<String, Agent> {
    // 1. User-controlled override directory.
    if let Ok(home) = std::env::var("HOME") {
        let user_dir = PathBuf::from(home).join(".flaco/agents");
        if user_dir.is_dir() {
            tracing::info!(
                target: "server",
                dir = %user_dir.display(),
                "loading agents from user directory"
            );
            return load_agents_from_dir(&user_dir);
        }
    }

    // 2. Fallback: in-repo baseline agents, so a clean dev checkout works.
    let repo_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../agents");
    if repo_dir.is_dir() {
        tracing::info!(
            target: "server",
            dir = %repo_dir.display(),
            "loading agents from in-repo fallback directory (no $HOME/.flaco/agents)"
        );
        return load_agents_from_dir(&repo_dir);
    }

    tracing::warn!(
        target: "server",
        "no agent directory found (checked $HOME/.flaco/agents and repo fallback) — \
         starting with empty agent registry"
    );
    HashMap::new()
}

fn load_registry<T>(
    kind: &str,
    subdir: &str,
    loader: fn(&Path) -> HashMap<String, T>,
) -> HashMap<String, T> {
    if let Ok(home) = std::env::var("HOME") {
        let user_dir = PathBuf::from(home).join(format!(".flaco/{subdir}"));
        if user_dir.is_dir() {
            tracing::info!(target: "server", dir = %user_dir.display(), "loading {kind} from user directory");
            return loader(&user_dir);
        }
    }
    let repo_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../{subdir}"));
    if repo_dir.is_dir() {
        tracing::info!(target: "server", dir = %repo_dir.display(), "loading {kind} from repo fallback");
        return loader(&repo_dir);
    }
    HashMap::new()
}

/// First-run setup wizard. Called when ~/.config/flaco/config doesn't exist
/// or when --setup is passed explicitly.
fn run_setup() {
    use std::io::{self, Write, BufRead};
    
    let config_dir = dirs::home_dir()
        .map(|h| h.join(".config/flaco"))
        .unwrap_or_else(|| std::path::PathBuf::from(".config/flaco"));
    let config_path = config_dir.join("config");
    
    println!();
    println!("  \x1b[1;36m━━━ flacoAi Setup ━━━\x1b[0m");
    println!();
    println!("  Let's configure flacoAi. Press Enter to accept defaults.");
    println!();
    
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    
    // 1. Ollama URL
    print!("  Ollama URL [\x1b[2mhttp://localhost:11434\x1b[0m]: ");
    io::stdout().flush().unwrap();
    let mut ollama_url = String::new();
    reader.read_line(&mut ollama_url).unwrap();
    let ollama_url = ollama_url.trim();
    let ollama_url = if ollama_url.is_empty() { "http://localhost:11434" } else { ollama_url };
    
    // 2. Test connection and list models
    println!("  \x1b[2mTesting connection to {ollama_url}...\x1b[0m");
    let models = test_ollama_connection(ollama_url);
    
    let (model, model_small, model_large, model_coder) = if let Some(ref models) = models {
        println!("  \x1b[32m✓\x1b[0m Connected. {} models available:", models.len());
        for (i, m) in models.iter().enumerate() {
            println!("    {}. {}", i + 1, m);
        }
        println!();
        
        // Pick models
        let model = pick_model(&mut reader, "Default model (medium tier)", models, 
            models.first().map(|s| s.as_str()));
        let model_small = pick_model(&mut reader, "Small/fast model (short messages)", models,
            models.iter().find(|m| m.contains("flash") || m.contains("mini")).map(|s| s.as_str()));
        let model_large = pick_model(&mut reader, "Large model (complex tasks)", models,
            models.iter().find(|m| m.contains("70b") || m.contains("r1")).map(|s| s.as_str()));
        let model_coder = pick_model(&mut reader, "Coder model (code tasks)", models,
            models.iter().find(|m| m.contains("coder")).map(|s| s.as_str()));
        
        (model, model_small, model_large, model_coder)
    } else {
        println!("  \x1b[33m⚠\x1b[0m Could not connect to Ollama. You can configure models manually later.");
        println!();
        
        print!("  Default model name: ");
        io::stdout().flush().unwrap();
        let mut model = String::new();
        reader.read_line(&mut model).unwrap();
        let model = model.trim().to_string();
        
        (
            if model.is_empty() { String::new() } else { model },
            String::new(),
            String::new(), 
            String::new(),
        )
    };
    
    // 3. Anthropic API key
    println!();
    println!("  \x1b[1mClaude Validation (recommended)\x1b[0m");
    println!("  Every response is validated by Claude to catch hallucinations.");
    println!("  Get a key at: https://console.anthropic.com/settings/keys");
    println!();
    print!("  Anthropic API key [\x1b[2mskip\x1b[0m]: ");
    io::stdout().flush().unwrap();
    let mut api_key = String::new();
    reader.read_line(&mut api_key).unwrap();
    let api_key = api_key.trim();
    
    // 4. Write config
    let _ = std::fs::create_dir_all(&config_dir);
    let mut config = String::new();
    config.push_str("# flacoAi Configuration\n");
    config.push_str(&format!("OLLAMA_BASE_URL={ollama_url}\n"));
    if !model.is_empty() { config.push_str(&format!("FLACO_MODEL={model}\n")); }
    if !model_small.is_empty() { config.push_str(&format!("FLACO_MODEL_SMALL={model_small}\n")); }
    if !model_large.is_empty() { config.push_str(&format!("FLACO_MODEL_LARGE={model_large}\n")); }
    if !model_coder.is_empty() { config.push_str(&format!("FLACO_MODEL_CODER={model_coder}\n")); }
    if !api_key.is_empty() { config.push_str(&format!("ANTHROPIC_API_KEY={api_key}\n")); }
    
    std::fs::write(&config_path, &config).expect("Failed to write config");
    
    println!();
    println!("  \x1b[32m✓\x1b[0m Config saved to {}", config_path.display());
    println!("  \x1b[2mEdit anytime: nano {}\x1b[0m", config_path.display());
    println!();
    
    // Re-export the vars so the current process picks them up
    std::env::set_var("OLLAMA_BASE_URL", ollama_url);
    if !model.is_empty() { std::env::set_var("FLACO_MODEL", &model); }
    if !model_small.is_empty() { std::env::set_var("FLACO_MODEL_SMALL", &model_small); }
    if !model_large.is_empty() { std::env::set_var("FLACO_MODEL_LARGE", &model_large); }
    if !model_coder.is_empty() { std::env::set_var("FLACO_MODEL_CODER", &model_coder); }
    if !api_key.is_empty() { std::env::set_var("ANTHROPIC_API_KEY", api_key); }
}

fn test_ollama_connection(url: &str) -> Option<Vec<String>> {
    let rt = tokio::runtime::Handle::current();
    let url = url.to_string();
    std::thread::spawn(move || {
        rt.block_on(async {
            let client = reqwest::Client::new();
            let resp = client
                .get(format!("{url}/api/tags"))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
                .ok()?;
            let body: serde_json::Value = resp.json().await.ok()?;
            let models: Vec<String> = body["models"]
                .as_array()?
                .iter()
                .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                .collect();
            Some(models)
        })
    })
    .join()
    .ok()?
}

fn pick_model(reader: &mut impl std::io::BufRead, label: &str, models: &[String], default: Option<&str>) -> String {
    let default_display = default.unwrap_or("none");
    print!("  {label} [\x1b[2m{default_display}\x1b[0m]: ");
    { use std::io::Write; std::io::stdout().flush().unwrap(); }
    let mut input = String::new();
    reader.read_line(&mut input).unwrap();
    let input = input.trim();
    
    if input.is_empty() {
        default.unwrap_or("").to_string()
    } else if let Ok(idx) = input.parse::<usize>() {
        models.get(idx - 1).cloned().unwrap_or_else(|| input.to_string())
    } else {
        input.to_string()
    }
}


#[tokio::main]
async fn main() {
    let repl_mode = std::env::args().any(|a| a == "--repl");

    // First-run setup: check if config exists when in REPL mode
    let setup_mode = std::env::args().any(|a| a == "--setup");
    if repl_mode || setup_mode {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".config/flaco/config"))
            .unwrap_or_default();
        if setup_mode || !config_path.exists() {
            run_setup();
        }
    }
    
    if repl_mode {
        // REPL: suppress log output to keep the terminal clean
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .init();
    } else {
        tracing_subscriber::fmt::init();
    }

    // Acquire the single-host PID lock for Slack mode only. The lock
    // prevents two Slack servers from racing on Socket Mode events (the
    // original bug). REPL mode is an interactive session the user
    // explicitly starts, so it skips the lock and coexists with a
    // running Slack server.
    let _lock = if repl_mode {
        None
    } else {
        match LockFile::acquire(lock_path()) {
            Ok(l) => Some(l),
            Err(e) => {
                eprintln!("Refusing to start: {e}");
                eprintln!("If you're sure no other instance is running, delete the lock file and retry.");
                std::process::exit(1);
            }
        }
    };

    // Ollama config is shared between both modes.
    let ollama_url = std::env::var("OLLAMA_BASE_URL")
        .or_else(|_| std::env::var("OLLAMA_HOST"))
        .ok();
    let model = std::env::var("FLACO_MODEL").ok();
    let model_small = std::env::var("FLACO_MODEL_SMALL")
        .ok()
        .filter(|s| !s.is_empty());
    let model_large = std::env::var("FLACO_MODEL_LARGE")
        .ok()
        .filter(|s| !s.is_empty());
    let model_coder = std::env::var("FLACO_MODEL_CODER")
        .ok()
        .filter(|s| !s.is_empty());

    // Load registries (shared between both modes).
    let agents = load_agent_registry();
    let skills = load_registry::<Skill>("skills", "skills", load_skills_from_dir);
    let commands = load_registry::<Command>("commands", "commands", load_commands_from_dir);
    let rules = load_registry::<Rule>("rules", "rules", load_rules_from_dir);
    tracing::info!(
        target: "server",
        agents = agents.len(),
        skills = skills.len(),
        commands = commands.len(),
        rules = rules.len(),
        "registries loaded"
    );

    if repl_mode {
        run_repl(ollama_url, model, model_small, model_large, model_coder, agents, skills, commands, rules).await;
    } else {
        run_slack(ollama_url, model, model_small, model_large, model_coder, agents, skills, commands, rules).await;
    }

    // _lock drops here on clean exit, releasing the lock file.
}

/// Slack Socket Mode — the existing production path.
async fn run_slack(
    ollama_url: Option<String>,
    model: Option<String>,
    model_small: Option<String>,
    model_large: Option<String>,
    model_coder: Option<String>,
    agents: HashMap<String, Agent>,
    skills: HashMap<String, Skill>,
    commands: HashMap<String, channels::commands::Command>,
    rules: HashMap<String, Rule>,
) {
    // Load tokens from environment
    let app_token = std::env::var("SLACK_APP_TOKEN").unwrap_or_else(|_| {
        eprintln!("Missing SLACK_APP_TOKEN (xapp-...)");
        eprintln!("Set it in ~/.zshrc: export SLACK_APP_TOKEN=\"xapp-...\"");
        std::process::exit(1);
    });

    let bot_token = std::env::var("SLACK_BOT_TOKEN").unwrap_or_else(|_| {
        eprintln!("Missing SLACK_BOT_TOKEN (xoxb-...)");
        eprintln!("Set it in ~/.zshrc: export SLACK_BOT_TOKEN=\"xoxb-...\"");
        std::process::exit(1);
    });

    // Discover our bot_id once at startup so socket_mode doesn't have to
    // hardcode it. If auth.test fails, we continue with None — the self-
    // filter will be disabled but the bot will still run.
    let our_bot_id = channels::socket_mode::fetch_bot_id(&bot_token).await;
    match &our_bot_id {
        Some(id) => tracing::info!(target: "server", %id, "discovered our bot_id via auth.test"),
        None => tracing::warn!(
            target: "server",
            "auth.test failed — our_bot_id is None, self-filter disabled"
        ),
    }

    let gateway_config = GatewayConfig {
        model,
        model_small,
        model_large,
        model_coder,
        ollama_url,
        our_bot_id,
        personas: vec![ChannelPersona::slack()],
        agents,
        skills,
        commands,
        rules,
    };

    let gateway = Arc::new(Gateway::new(gateway_config));

    println!();
    println!("  \x1b[1;36mflacoAi\x1b[0m Slack server (Socket Mode)");
    println!("  Model (medium): {}", gateway.model());
    println!("  Model (small):  {}", gateway.model_small().unwrap_or("<unset>"));
    println!("  Model (large):  {}", gateway.model_large().unwrap_or("<unset>"));
    println!("  Model (coder):  {}", gateway.model_coder().unwrap_or("<unset>"));
    println!("  Ollama: {}", gateway.ollama_url());
    println!("  Bot ID: {}", gateway.our_bot_id());
    println!("  Agents:   {} loaded", gateway.agents().len());
    println!("  Skills:   {} loaded", gateway.skills().len());
    println!("  Commands: {} loaded", gateway.commands().len());
    println!("  Rules:    {} loaded", gateway.rules().len());
    println!("  PID: {}", std::process::id());
    println!();
    println!("  Connecting to Slack via Socket Mode...");
    println!("  Messages to your bot will be handled automatically.");
    println!("  Press Ctrl+C to stop.");
    println!();

    if let Err(e) = channels::socket_mode::run_socket_mode(&app_token, &bot_token, gateway).await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Terminal REPL mode — interactive local chat with the same inference
/// pipeline (Ollama + Claude vet) as Slack, but without Slack connectivity.
async fn run_repl(
    ollama_url: Option<String>,
    model: Option<String>,
    model_small: Option<String>,
    model_large: Option<String>,
    model_coder: Option<String>,
    agents: HashMap<String, Agent>,
    skills: HashMap<String, Skill>,
    commands: HashMap<String, channels::commands::Command>,
    rules: HashMap<String, Rule>,
) {
    let terminal_persona = ChannelPersona {
        channel: "terminal".into(),
        prompt_overlay: String::new(),
    };

    let gateway_config = GatewayConfig {
        model,
        model_small,
        model_large,
        model_coder,
        ollama_url,
        our_bot_id: None,
        personas: vec![terminal_persona.clone()],
        agents,
        skills,
        commands,
        rules,
    };

    let gateway = Gateway::new(gateway_config);
    let http = reqwest::Client::new();
    let ollama_url = gateway.ollama_url().trim_end_matches("/v1").to_string();

    println!();
    println!("  \x1b[1;36mflacoAi\x1b[0m Terminal REPL");
    println!("  Model (medium): {}", gateway.model());
    println!("  Model (small):  {}", gateway.model_small().unwrap_or("<unset>"));
    println!("  Model (large):  {}", gateway.model_large().unwrap_or("<unset>"));
    println!("  Model (coder):  {}", gateway.model_coder().unwrap_or("<unset>"));
    println!("  Ollama: {}", ollama_url);
    println!("  Agents:   {} loaded", gateway.agents().len());
    println!("  Skills:   {} loaded", gateway.skills().len());
    println!("  Commands: {} loaded", gateway.commands().len());
    println!("  Rules:    {} loaded", gateway.rules().len());
    println!("  PID: {}", std::process::id());
    println!();
    println!("  Type a message and press Enter. Ctrl+C to exit.");
    println!();
    {
        use std::io::Write;
        print!("flacoAi> ");
        let _ = std::io::stdout().flush();
    }


    let stdin = std::io::stdin();
    let reader = std::io::BufReader::new(stdin.lock());
    use std::io::BufRead;

    for line in reader.lines() {
        let input = match line {
            Ok(l) => l,
            Err(_) => break, // EOF or read error — exit cleanly
        };
        let trimmed = input.trim();
        if trimmed.is_empty() {
            print!("flacoAi> ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            continue;
        }

        // Pick model based on content
        let chosen_model = gateway.pick_model(&terminal_persona, trimmed);
        {
            use std::io::Write;
            print!("\x1b[2m  thinking...\x1b[0m");
            let _ = std::io::stdout().flush();
        }
        let today = chrono::Local::now().format("%A, %B %-d, %Y").to_string();
        let mut system_prompt = format!(
            "You are flacoAi, a local AI assistant running on elGordo's homelab. \
             You are powered by {chosen_model} via Ollama. Today is {today}. \
             Be helpful, concise, and accurate. If you don't know something, \
             say so — don't make up answers."
        );

        // Web search grounding for current events / sports / news
        // Check for video generation request
        if let Some((topic, duration, style, notes)) = channels::inference::is_video_request(trimmed) {
            print!("\r\x1b[K");
            println!("  \x1b[2mGenerating video script...\x1b[0m");
            match channels::inference::trigger_video_generation(&http, &topic, &duration, &style, &notes).await {
                Ok(msg) => {
                    println!("\n\x1b[32m  \u{2713}\x1b[0m {msg}");
                    print!("flacoAi> ");
                    { use std::io::Write; let _ = std::io::stdout().flush(); }
                    continue;
                }
                Err(e) => {
                    println!("\n\x1b[31m  \u{2717}\x1b[0m {e}");
                    print!("flacoAi> ");
                    { use std::io::Write; let _ = std::io::stdout().flush(); }
                    continue;
                }
            }
        }

        // Try live sports APIs first, then fall back to web search
        let mut sports_found = false;
        for sports_info in [
            channels::inference::sports_data(&http, trimmed).await,
            channels::inference::nhl_data(&http, trimmed).await,
            channels::inference::epl_data(&http, trimmed).await,
        ] {
            if let Some(info) = sports_info {
                system_prompt.push_str(&format!(
                    "\n\n{info}\n\nUse this live data to answer the question directly. State the specific facts — teams, times, scores, pitchers."
                ));
                sports_found = true;
                break;
            }
        }
        if !sports_found {
        if let Some(query) = needs_web_search(trimmed) {
            match web_search(&http, &query).await {
                Ok(results) => {
                    system_prompt.push_str(&format!(
                        "\n\nCurrent information from web search for '{query}':\n{results}\n\nUse the search results above to answer the question directly. Extract specific facts (dates, times, scores, names) from the results and present them confidently. Do NOT say you do not have information, do NOT tell the user to check a website, do NOT say you recommend visiting anything, do NOT say your training data is outdated, do NOT defer to external links when the answer is in the search results. If the search results contain the answer, STATE IT. If they genuinely do not contain the answer, say what you DID find."
                    ));
                    println!("  \x1b[2m(searched: {query})\x1b[0m");
                }
                Err(e) => {
                    tracing::warn!(target: "repl", %query, error = %e, "web search failed");
                }
            }
        }
        } // end if !sports_found

        // Call Ollama
        let local_result = call_ollama(&http, &ollama_url, &chosen_model, &system_prompt, trimmed).await;
        let local_reply = match local_result {
            Ok(reply) => reply,
            Err(e) => {
                println!("\n\x1b[31merror:\x1b[0m {e}\n");
                print!("flacoAi> ");
                use std::io::Write;
                let _ = std::io::stdout().flush();
                continue;
            }
        };

        // Always vet through Claude
        let vet_result = claude_check(
            &http,
            trimmed,
            "",               // no channel context for terminal
            &local_reply,
            &chosen_model,
            &terminal_persona,
        )
        .await;

        match vet_result {
            CheckResult::Approved => {
                println!("\n\x1b[32m\u{2713}\x1b[0m {local_reply}\n");
            }
            CheckResult::Corrected(corrected) => {
                println!("\n\x1b[33m\u{2713} (corrected)\x1b[0m {corrected}\n");
            }
            CheckResult::Unavailable(reason) => {
                tracing::debug!(target: "repl", %reason, "vet unavailable");
                println!("\n\x1b[33m\u{26a0} unvetted\x1b[0m {local_reply}\n");
            }
        }

        print!("flacoAi> ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}
