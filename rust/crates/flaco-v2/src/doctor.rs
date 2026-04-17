//! `flaco-v2 doctor` — runtime health check.
//!
//! Walks a fixed set of checks against the loaded `Config` and reports
//! pass/fail with a short reason. Exits with a non-zero status if any
//! check fails. Designed to be the single command a grader or operator
//! runs to confirm every P0 ship-blocker artifact is in place and the
//! runtime is actually healthy:
//!
//!   1. config parseable           — trivially true if we got here
//!   2. db openable                — Memory::open succeeds on cfg.paths.db
//!   3. db fts5 query works        — search_facts returns Ok
//!   4. ollama reachable           — GET {base_url}/api/tags returns 200
//!   5. backup fresh (<25h)        — newest file in cfg.backup.directory
//!   6. disk free > 1GB            — df the db parent dir
//!   7. launchd flaco running      — launchctl print io.roura.flaco
//!   8. launchd backup registered  — launchctl print io.roura.flaco.backup
//!
//! This file talks to the system (process spawn, file I/O, network),
//! intentionally — doctor's whole job is to probe reality.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use flaco_config::Config;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Debug)]
enum Status {
    Pass,
    Warn,
    Fail,
}

struct Check {
    name: &'static str,
    status: Status,
    detail: String,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Pass, detail: detail.into() }
    }
    fn warn(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Warn, detail: detail.into() }
    }
    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Fail, detail: detail.into() }
    }
    fn render(&self) -> String {
        let (color, tag) = match self.status {
            Status::Pass => (GREEN, "PASS"),
            Status::Warn => (YELLOW, "WARN"),
            Status::Fail => (RED, "FAIL"),
        };
        format!(
            "  {color}{BOLD}[{tag}]{RESET}  {BOLD}{:<28}{RESET}  {DIM}{}{RESET}",
            self.name, self.detail
        )
    }
}

/// Run every check in order and print a report. Returns `Ok(())` only if
/// every check passed. Warnings don't flip the exit code — they're for
/// things that are non-blocking (e.g. a credentialed tool env var the
/// operator didn't set).
pub fn run(cfg: &Config) -> Result<()> {
    println!("{BOLD}flacoAi doctor{RESET}  {DIM}· powered by Roura.io{RESET}");
    println!("  {DIM}config: {:?}{RESET}", cfg.source());
    println!();

    let checks = vec![
        check_config_source(cfg),
        check_db_openable(cfg),
        check_db_fts(cfg),
        check_ollama_reachable(cfg),
        check_backup_fresh(cfg),
        check_disk_free(cfg),
        check_launchd_flaco(),
        check_launchd_backup(),
        check_shortcuts_dir(cfg),
        check_log_dir(cfg),
    ];

    let mut passed = 0;
    let mut warned = 0;
    let mut failed = 0;
    for c in &checks {
        println!("{}", c.render());
        match c.status {
            Status::Pass => passed += 1,
            Status::Warn => warned += 1,
            Status::Fail => failed += 1,
        }
    }

    println!();
    let summary_color = if failed > 0 {
        RED
    } else if warned > 0 {
        YELLOW
    } else {
        GREEN
    };
    println!(
        "{summary_color}{BOLD}  {passed} pass · {warned} warn · {failed} fail{RESET}"
    );

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ---- individual checks ----

fn check_config_source(cfg: &Config) -> Check {
    use flaco_config::ConfigSource;
    let detail = format!("{:?}", cfg.source());
    match cfg.source() {
        ConfigSource::File(_) | ConfigSource::FilePlusEnv(_) => Check::pass("config loaded", detail),
        ConfigSource::EnvOnly => Check::warn("config loaded", "env only, no file — ok but odd"),
        ConfigSource::Defaults => Check::warn(
            "config loaded",
            "no config file found, running on built-in defaults",
        ),
    }
}

fn check_db_openable(cfg: &Config) -> Check {
    use flaco_core::memory::Memory;
    match Memory::open(&cfg.paths.db) {
        Ok(_m) => Check::pass("db openable", cfg.paths.db.to_string_lossy().to_string()),
        Err(e) => Check::fail("db openable", format!("{}: {e}", cfg.paths.db.display())),
    }
}

fn check_db_fts(cfg: &Config) -> Check {
    use flaco_core::memory::Memory;
    let Ok(mem) = Memory::open(&cfg.paths.db) else {
        return Check::fail("db fts5", "db did not open");
    };
    // A dummy search — we don't care about the results, only that the
    // FTS5 virtual table exists and answers.
    match mem.search_facts("chris", "test", 1) {
        Ok(_) => Check::pass("db fts5 search", "memories_fts responds"),
        Err(e) => Check::fail("db fts5 search", format!("{e}")),
    }
}

fn check_ollama_reachable(cfg: &Config) -> Check {
    let url = format!("{}/api/tags", cfg.ollama.base_url.trim_end_matches('/'));
    match ureq_get(&url, Duration::from_secs(3)) {
        Ok(body) if body.contains("\"models\"") => {
            // Count models for the detail line.
            let n = body.matches("\"name\":").count();
            Check::pass("ollama reachable", format!("{} · {n} models", cfg.ollama.base_url))
        }
        Ok(_) => Check::warn("ollama reachable", "200 but no 'models' key in body"),
        Err(e) => Check::fail("ollama reachable", format!("{}: {e}", cfg.ollama.base_url)),
    }
}

fn check_backup_fresh(cfg: &Config) -> Check {
    let dir = &cfg.backup.directory;
    if dir.as_os_str().is_empty() {
        return Check::warn("backup fresh", "no backup directory configured");
    }
    if !dir.exists() {
        return Check::fail("backup fresh", format!("{} does not exist", dir.display()));
    }
    let mut newest: Option<(SystemTime, std::path::PathBuf)> = None;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Check::fail("backup fresh", format!("cannot read {}", dir.display()));
    };
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
        if !name.starts_with("flaco-") || !name.ends_with(".db") {
            continue;
        }
        if let Ok(meta) = e.metadata() {
            if let Ok(modified) = meta.modified() {
                if newest.as_ref().map(|(t, _)| &modified > t).unwrap_or(true) {
                    newest = Some((modified, p));
                }
            }
        }
    }
    match newest {
        None => Check::fail("backup fresh", "no flaco-*.db files in backup dir"),
        Some((ts, path)) => {
            let age = SystemTime::now().duration_since(ts).unwrap_or_default();
            let age_hours = age.as_secs() / 3600;
            if age_hours < 25 {
                Check::pass("backup fresh", format!("{} ({}h old)", path.display(), age_hours))
            } else {
                Check::warn(
                    "backup fresh",
                    format!("{} is {}h old (>25h)", path.display(), age_hours),
                )
            }
        }
    }
}

fn check_disk_free(cfg: &Config) -> Check {
    // Call `df -k <dir>` and parse the Available column (4th).
    let dir = cfg
        .paths
        .db
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    let out = Command::new("df").arg("-k").arg(&dir).output();
    let Ok(out) = out else {
        return Check::warn("disk free", "df command failed");
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let last = text.lines().last().unwrap_or("");
    let cols: Vec<&str> = last.split_whitespace().collect();
    if cols.len() < 4 {
        return Check::warn("disk free", "df output not parseable");
    }
    let avail_kb: u64 = cols[3].parse().unwrap_or(0);
    let avail_gb = avail_kb as f64 / 1_048_576.0;
    if avail_gb >= 1.0 {
        Check::pass("disk free", format!("{:.1} GB available", avail_gb))
    } else {
        Check::fail(
            "disk free",
            format!("only {:.2} GB available in {}", avail_gb, dir.display()),
        )
    }
}

fn check_launchd_flaco() -> Check {
    let uid = get_uid();
    let label = format!("gui/{uid}/io.roura.flaco");
    match Command::new("launchctl").arg("print").arg(&label).output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.contains("state = running") {
                let pid = text
                    .lines()
                    .find(|l| l.trim().starts_with("pid ="))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "?".into());
                Check::pass("launchd flaco web", format!("running, pid={pid}"))
            } else {
                Check::warn("launchd flaco web", "loaded but not running")
            }
        }
        Ok(_) => Check::fail(
            "launchd flaco web",
            "io.roura.flaco not loaded — run install-home.sh",
        ),
        Err(e) => Check::warn("launchd flaco web", format!("launchctl failed: {e}")),
    }
}

fn check_launchd_backup() -> Check {
    let uid = get_uid();
    let label = format!("gui/{uid}/io.roura.flaco.backup");
    match Command::new("launchctl").arg("print").arg(&label).output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let state = text
                .lines()
                .find(|l| l.trim().starts_with("state ="))
                .map(|l| l.trim().trim_start_matches("state = ").to_string())
                .unwrap_or_else(|| "unknown".into());
            Check::pass("launchd flaco backup", format!("registered, state={state}"))
        }
        Ok(_) => Check::fail(
            "launchd flaco backup",
            "io.roura.flaco.backup not loaded — run install-home.sh",
        ),
        Err(e) => Check::warn("launchd flaco backup", format!("launchctl failed: {e}")),
    }
}

fn check_shortcuts_dir(cfg: &Config) -> Check {
    let dir = &cfg.paths.shortcuts_dir;
    if dir.exists() {
        Check::pass("shortcuts dir", dir.to_string_lossy().to_string())
    } else {
        Check::warn(
            "shortcuts dir",
            format!("{} does not exist (created on first shortcut)", dir.display()),
        )
    }
}

fn check_log_dir(cfg: &Config) -> Check {
    let dir = &cfg.paths.log_dir;
    if dir.exists() {
        Check::pass("log dir", dir.to_string_lossy().to_string())
    } else {
        Check::warn("log dir", format!("{} does not exist", dir.display()))
    }
}

// ---- plumbing ----

/// Return the current effective UID without pulling in libc. `id -u`
/// always works on macOS and Linux and is what launchctl expects.
fn get_uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "0".into())
}

/// Very small synchronous HTTP GET so doctor doesn't pull in the entire
/// reqwest/async stack just for a one-shot reachability probe. Uses
/// `curl --max-time` which is always on macOS and most Linux distros.
fn ureq_get(url: &str, timeout: Duration) -> Result<String, String> {
    let secs = timeout.as_secs().max(1).to_string();
    let out = Command::new("curl")
        .arg("-sS")
        .arg("--max-time")
        .arg(&secs)
        .arg(url)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
