//! Pre-tool-call vet layer — a second opinion from Claude Haiku on whether
//! a tool call proposed by the local Ollama model makes sense given the
//! user's latest message.
//!
//! The existing `channels::inference::claude_check` vets **final text
//! replies** *post hoc* and only runs on mission-critical Slack channels.
//! That leaves a gap: a local model can invoke a tool (e.g. `find .` on a
//! greeting) long before any text reply exists to vet. This module closes
//! the gap for every surface that uses a `ToolExecutor` by intercepting
//! the tool call **before** it runs.
//!
//! # Modes
//!
//! Driven by `FLACO_VET_TOOLS` (case-insensitive):
//!   - `off`           — never vet
//!   - `destructive`   — vet all destructive calls (default)
//!   - `all`           — vet every call, sampling reads at
//!                       `FLACO_VET_SAMPLE_RATE` (default `0.2`)
//!
//! # Fail-open
//!
//! If `ANTHROPIC_API_KEY` is unset or Claude is unreachable, the call is
//! allowed through and tagged `Unavailable`. The vet is a safety *addition*
//! — it must never become a new source of false negatives.
//!
//! # Deny format
//!
//! A denial is returned to the runtime as a `ToolError` so the model sees
//! the rejection as a normal tool failure, reads the reason, and retries
//! with a plain text reply or a different approach.

use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

// =====================================================================
// Public types
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VetMode {
    Off,
    DestructiveOnly,
    All,
}

impl VetMode {
    /// Parse `FLACO_VET_TOOLS`. Unrecognised values fall through to the
    /// safe default (`DestructiveOnly`) instead of failing loudly — the
    /// vet is a safety feature and should not brick a session on typos.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var("FLACO_VET_TOOLS")
            .ok()
            .map(|raw| raw.trim().to_ascii_lowercase())
            .map_or(Self::DestructiveOnly, |value| match value.as_str() {
                "off" | "disabled" | "0" | "false" | "no" => Self::Off,
                "all" | "every" | "full" => Self::All,
                _ => Self::DestructiveOnly,
            })
    }
}

#[derive(Debug, Clone)]
pub struct VetConfig {
    pub mode: VetMode,
    /// When `mode == All`, non-destructive tool calls are sampled at this
    /// rate. `0.0` means never vet reads; `1.0` means always vet reads.
    pub read_sample_rate: f32,
    pub model: String,
    pub api_key: Option<String>,
}

impl VetConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let mode = VetMode::from_env();
        let read_sample_rate = std::env::var("FLACO_VET_SAMPLE_RATE")
            .ok()
            .and_then(|raw| raw.trim().parse::<f32>().ok())
            .map_or(0.2, |value| value.clamp(0.0, 1.0));
        let model = std::env::var("FLACO_VET_MODEL")
            .ok()
            .filter(|raw| !raw.trim().is_empty())
            .unwrap_or_else(|| "claude-haiku-4-5".to_string());
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|raw| !raw.trim().is_empty());
        Self {
            mode,
            read_sample_rate,
            model,
            api_key,
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, VetMode::Off)
    }
}

impl Default for VetConfig {
    fn default() -> Self {
        Self {
            mode: VetMode::DestructiveOnly,
            read_sample_rate: 0.2,
            model: "claude-haiku-4-5".to_string(),
            api_key: None,
        }
    }
}

/// Shared mutable slot holding the user's most recent message. The runtime
/// sets this at the start of each turn; the executor reads it before each
/// tool call. Wrapped in `Arc<ToolVetContext>` so both the CLI frontend and
/// the moved-in `ToolExecutor` can reach the same slot.
#[derive(Debug, Default)]
pub struct ToolVetContext {
    last_user_message: Mutex<Option<String>>,
}

impl ToolVetContext {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_last_user_message(&self, text: impl Into<String>) {
        if let Ok(mut guard) = self.last_user_message.lock() {
            *guard = Some(text.into());
        }
    }

    #[must_use]
    pub fn last_user_message(&self) -> Option<String> {
        self.last_user_message
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }
}

/// Outcome of a single vet call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VetDecision {
    /// Claude said the tool call makes sense.
    Approve,
    /// Claude said no, with a human-readable reason and an optional
    /// alternative the model should try instead.
    Deny {
        reason: String,
        suggestion: Option<String>,
    },
    /// The vet was skipped — the mode or sampling policy said we didn't
    /// need a check for this call. Treated as an implicit approve.
    Skipped,
    /// The vet tried but could not reach Claude. Treated as an implicit
    /// approve; the reason is logged so we can tell "working as intended"
    /// apart from "silently degraded".
    Unavailable(String),
}

impl VetDecision {
    /// Returns the human-facing deny message the executor should surface to
    /// the model, or `None` if the call should proceed.
    #[must_use]
    pub fn deny_message(&self) -> Option<String> {
        match self {
            Self::Deny { reason, suggestion } => {
                let mut message = format!(
                    "Tool call rejected by vet layer: {reason}. Reply to the user in plain text without calling a tool, or rethink your approach."
                );
                if let Some(alt) = suggestion {
                    if !alt.trim().is_empty() {
                        message.push_str(&format!(" Suggestion: {alt}"));
                    }
                }
                Some(message)
            }
            _ => None,
        }
    }
}

// =====================================================================
// Process-wide handles
// =====================================================================

/// The single process-wide `ToolVetContext`. Frontends (CLI, TUI, server)
/// share this handle so a `ToolExecutor` buried inside a moved
/// `ConversationRuntime` can still see the user's latest message without
/// plumbing a parameter through every call site.
#[must_use]
pub fn process_context() -> &'static ToolVetContext {
    static CTX: OnceLock<ToolVetContext> = OnceLock::new();
    CTX.get_or_init(ToolVetContext::new)
}

/// The process-wide `VetConfig`, loaded from env the first time it's
/// requested. Env reads happen once per process — changing
/// `FLACO_VET_TOOLS` after startup has no effect, which matches the usual
/// expectation for CLI env-var config.
#[must_use]
pub fn process_config() -> &'static VetConfig {
    static CFG: OnceLock<VetConfig> = OnceLock::new();
    CFG.get_or_init(VetConfig::from_env)
}

// =====================================================================
// Destructive-tool classification
// =====================================================================

/// Tool names that always mutate state. Matched case-insensitively.
const ALWAYS_DESTRUCTIVE_TOOLS: &[&str] = &[
    "write_file",
    "write",
    "edit_file",
    "edit",
    "notebookedit",
    "notebook_edit",
    "delete",
    "remove",
    "rm",
    "patch",
    "apply_patch",
    "exit_plan_mode",
];

/// Substrings in tool names that strongly imply a mutating action.
const DESTRUCTIVE_NAME_HINTS: &[&str] = &[
    "delete", "remove", "drop", "destroy", "truncate", "push", "publish",
    "deploy", "install", "uninstall", "upgrade", "migrate", "mutate",
    "send", "post_", "post-", "write", "update", "revoke", "reset",
];

/// Substrings inside a bash command line that mark it as destructive.
/// Conservative on purpose — false positives here just mean "Claude gets
/// one more yes/no call", while false negatives skip the safety check.
const DESTRUCTIVE_BASH_PATTERNS: &[&str] = &[
    // filesystem mutation
    " rm ", " rm\t", "rm -", " mv ", " cp -", " dd ", " chmod ", " chown ",
    " ln -", " mkfs", " truncate ", " rsync ", "> ", ">> ", " tee ",
    // process lifecycle
    " kill ", " killall ", " pkill ",
    // network-side effects
    " curl ", " wget ", " scp ", " ssh ",
    // scheduler / services
    " systemctl ", " launchctl ", " cron", " at ",
    // package managers
    " brew install", " brew uninstall", " brew upgrade", " brew tap",
    " apt-get", " apt ", " yum ", " dnf ", " pacman ", " pip install",
    " pip uninstall", " npm install", " npm uninstall", " npm publish",
    " yarn add", " yarn remove", " cargo install", " cargo publish",
    " gem install", " gem uninstall", " go install",
    // git side-effects
    " git push", " git reset --hard", " git clean ", " git rebase ",
    " git checkout --", " git restore --", " git branch -d",
    " git branch -D", " git tag -d", " git stash drop",
    " gh release create", " gh release delete", " gh pr merge",
    " gh pr close", " gh issue close",
    // db / infra
    " psql", " mysql", " sqlite3", " redis-cli", " terraform ",
    " kubectl delete", " kubectl apply", " docker rm", " docker rmi",
    " docker stop", " docker kill", " docker system prune",
    // sudo is always destructive
    " sudo ",
];

#[must_use]
pub fn is_destructive(tool_name: &str, input: &Value) -> bool {
    let lower = tool_name.to_ascii_lowercase();
    if ALWAYS_DESTRUCTIVE_TOOLS.contains(&lower.as_str()) {
        return true;
    }
    if DESTRUCTIVE_NAME_HINTS.iter().any(|hint| lower.contains(hint)) {
        return true;
    }

    if matches!(lower.as_str(), "bash" | "run_bash" | "shell" | "exec") {
        if let Some(command) = input.get("command").and_then(Value::as_str) {
            let padded = format!(" {} ", command.to_ascii_lowercase());
            return DESTRUCTIVE_BASH_PATTERNS
                .iter()
                .any(|pattern| padded.contains(&pattern.to_ascii_lowercase()));
        }
    }

    false
}

/// Given the mode + tool call, decide whether to actually dispatch a vet
/// call. Pure function — the sampling RNG comes from the caller so tests
/// can seed it deterministically.
#[must_use]
pub fn should_vet(
    config: &VetConfig,
    tool_name: &str,
    input: &Value,
    roll: f32,
) -> bool {
    match config.mode {
        VetMode::Off => false,
        VetMode::DestructiveOnly => is_destructive(tool_name, input),
        VetMode::All => {
            if is_destructive(tool_name, input) {
                true
            } else {
                roll < config.read_sample_rate
            }
        }
    }
}

// =====================================================================
// The Claude call
// =====================================================================

const VET_PROMPT_INSTRUCTIONS: &str = "\
You are a safety gate between a small local AI (running on Ollama) and a \
tool it wants to invoke. Your job is to catch tool calls that make no \
sense given what the user actually asked for.

You approve by default. Only deny when the tool call is clearly wrong for \
the user's request — e.g. running `find` or `bash` on a greeting/small \
talk, editing a file when the user only asked a question, deleting data \
on a read-style request, pushing/publishing without explicit user intent. \
Reading the project, running tests, grep/glob/read-file on an engineering \
question is fine — approve.

Respond with EXACTLY one of these formats:

APPROVED

or:

DENIED: <one short sentence explaining why this tool call doesn't match the user's intent>
SUGGEST: <short recommended next move — usually 'reply in plain text' or a different tool>

No preamble. No markdown outside the SUGGEST line. Do not explain your reasoning.";

/// Blocking HTTP call to Anthropic. Synchronous on purpose: the
/// `ToolExecutor::execute` trait is sync, and the CLI's one shared Tokio
/// runtime is already parked inside `block_on` for the model stream, so
/// nesting another `block_on` is both ugly and fragile. `reqwest::blocking`
/// with rustls gives us a clean, isolated HTTP path.
pub fn vet_tool_call(
    config: &VetConfig,
    user_message: &str,
    tool_name: &str,
    tool_input: &Value,
) -> VetDecision {
    let start = std::time::Instant::now();

    let Some(api_key) = config.api_key.clone() else {
        let result = VetDecision::Unavailable("not-configured".to_string());
        log_vet_decision(user_message, tool_name, tool_input, &result, start.elapsed());
        return result;
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(error) => {
            let result = VetDecision::Unavailable(format!("client: {error}"));
            log_vet_decision(user_message, tool_name, tool_input, &result, start.elapsed());
            return result;
        }
    };

    let prompt = build_vet_prompt(user_message, tool_name, tool_input);
    let body = json!({
        "model": config.model,
        "max_tokens": 512,
        "messages": [{"role": "user", "content": prompt}],
    });

    let response = match client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
    {
        Ok(r) => r,
        Err(error) => {
            let result = VetDecision::Unavailable(format!("network: {error}"));
            log_vet_decision(user_message, tool_name, tool_input, &result, start.elapsed());
            return result;
        }
    };

    if !response.status().is_success() {
        let code = response.status();
        let body_text = response.text().unwrap_or_default();
        let reason = match code.as_u16() {
            401 | 403 => format!("auth: HTTP {code}: {body_text}"),
            429 => format!("quota: HTTP {code}: {body_text}"),
            500..=599 => format!("network: HTTP {code}: {body_text}"),
            _ => format!("unexpected HTTP {code}: {body_text}"),
        };
        let result = VetDecision::Unavailable(reason);
        log_vet_decision(user_message, tool_name, tool_input, &result, start.elapsed());
        return result;
    }

    let parsed: Value = match response.json() {
        Ok(v) => v,
        Err(error) => {
            let result = VetDecision::Unavailable(format!("parse: {error}"));
            log_vet_decision(user_message, tool_name, tool_input, &result, start.elapsed());
            return result;
        }
    };

    let text = parsed
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let decision = parse_vet_response(&text);
    log_vet_decision(user_message, tool_name, tool_input, &decision, start.elapsed());
    decision
}

#[must_use]
pub fn build_vet_prompt(user_message: &str, tool_name: &str, tool_input: &Value) -> String {
    let input_rendered = serde_json::to_string_pretty(tool_input)
        .unwrap_or_else(|_| tool_input.to_string());
    format!(
        "{instructions}\n\nUser message:\n{user_message}\n\nProposed tool call:\ntool = {tool_name}\ninput = {input_rendered}\n",
        instructions = VET_PROMPT_INSTRUCTIONS,
        user_message = clip(user_message, 2_000),
        input_rendered = clip(&input_rendered, 2_000),
    )
}

#[must_use]
pub fn parse_vet_response(text: &str) -> VetDecision {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return VetDecision::Unavailable("empty-response".to_string());
    }
    if trimmed.starts_with("APPROVED") {
        return VetDecision::Approve;
    }
    if let Some(rest) = trimmed.strip_prefix("DENIED:") {
        let (reason_part, suggestion_part) = match rest.find("\nSUGGEST:") {
            Some(idx) => (&rest[..idx], Some(rest[idx + "\nSUGGEST:".len()..].trim())),
            None => (rest, None),
        };
        let reason = reason_part.trim().to_string();
        if reason.is_empty() {
            return VetDecision::Unavailable(format!("denied-without-reason: {trimmed}"));
        }
        return VetDecision::Deny {
            reason,
            suggestion: suggestion_part.map(ToString::to_string).filter(|s| !s.is_empty()),
        };
    }
    VetDecision::Unavailable(format!("unparseable: {trimmed}"))
}

// =====================================================================
// Sampling RNG — deterministic per (tool, input) so retries converge
// =====================================================================

/// Cheap, seedless sampler: hash the tool name and input together into a
/// [0.0, 1.0) float. Deterministic, so the same tool call always gets the
/// same roll and we don't flap between vetting and not vetting if the
/// model retries the exact same call.
#[must_use]
pub fn deterministic_roll(tool_name: &str, input: &Value) -> f32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tool_name.hash(&mut hasher);
    input.to_string().hash(&mut hasher);
    let digest = hasher.finish();
    // Take the low 24 bits and map to [0, 1) — plenty of resolution for
    // a 0.01-granular sample rate.
    let bits = (digest & 0x00FF_FFFF) as f32;
    bits / 16_777_216.0_f32
}

// =====================================================================
// Logging — best-effort JSONL to ~/.flaco/tool-vet-decisions.jsonl
// =====================================================================

fn log_vet_decision(
    user_message: &str,
    tool_name: &str,
    tool_input: &Value,
    decision: &VetDecision,
    latency: Duration,
) {
    let (verdict, reason, suggestion, unavailable_reason) = match decision {
        VetDecision::Approve => ("APPROVED", None, None, None),
        VetDecision::Deny { reason, suggestion } => {
            ("DENIED", Some(reason.as_str()), suggestion.as_deref(), None)
        }
        VetDecision::Skipped => ("SKIPPED", None, None, None),
        VetDecision::Unavailable(reason) => ("UNAVAILABLE", None, None, Some(reason.as_str())),
    };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let entry = json!({
        "ts_epoch": ts,
        "verdict": verdict,
        "latency_ms": latency.as_millis(),
        "tool_name": tool_name,
        "tool_input": tool_input,
        "user_message": clip(user_message, 2_000),
        "deny_reason": reason,
        "deny_suggestion": suggestion,
        "unavailable_reason": unavailable_reason,
    });

    let Ok(home) = std::env::var("HOME") else { return };
    let path = std::path::PathBuf::from(home).join(".flaco/tool-vet-decisions.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{entry}");
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max).collect();
    truncated.push_str("…");
    truncated
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // `with_env_var` sets/unsets env vars for the duration of a test, then
    // restores the previous value. Tests that touch env vars use the
    // runtime-wide lock (`crate::test_env_lock`) so they don't race.
    fn with_env_var<R>(key: &str, value: Option<&str>, body: impl FnOnce() -> R) -> R {
        let _guard = crate::test_env_lock();
        let previous = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        let result = body();
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        result
    }

    #[test]
    fn vet_mode_defaults_to_destructive_only() {
        with_env_var("FLACO_VET_TOOLS", None, || {
            assert_eq!(VetMode::from_env(), VetMode::DestructiveOnly);
        });
    }

    #[test]
    fn vet_mode_recognises_off_synonyms() {
        for raw in ["off", "OFF", "disabled", "0", "false", "no"] {
            with_env_var("FLACO_VET_TOOLS", Some(raw), || {
                assert_eq!(VetMode::from_env(), VetMode::Off, "raw={raw}");
            });
        }
    }

    #[test]
    fn vet_mode_recognises_all_synonyms() {
        for raw in ["all", "ALL", "every", "full"] {
            with_env_var("FLACO_VET_TOOLS", Some(raw), || {
                assert_eq!(VetMode::from_env(), VetMode::All, "raw={raw}");
            });
        }
    }

    #[test]
    fn vet_mode_falls_back_on_garbage() {
        with_env_var("FLACO_VET_TOOLS", Some("banana"), || {
            assert_eq!(VetMode::from_env(), VetMode::DestructiveOnly);
        });
    }

    #[test]
    fn write_and_edit_are_destructive() {
        for name in ["Write", "write_file", "Edit", "edit_file", "NotebookEdit"] {
            assert!(
                is_destructive(name, &json!({})),
                "{name} should be destructive"
            );
        }
    }

    #[test]
    fn read_like_tools_are_not_destructive() {
        for name in ["Read", "read_file", "Glob", "glob_search", "Grep", "grep_search", "WebSearch", "web_search"] {
            assert!(
                !is_destructive(name, &json!({})),
                "{name} should NOT be destructive"
            );
        }
    }

    #[test]
    fn bash_rm_is_destructive() {
        assert!(is_destructive("bash", &json!({"command": "rm -rf /tmp/foo"})));
        assert!(is_destructive("Bash", &json!({"command": "sudo rm -rf /etc"})));
    }

    #[test]
    fn bash_git_push_is_destructive() {
        assert!(is_destructive("bash", &json!({"command": "git push origin main"})));
        assert!(is_destructive("bash", &json!({"command": "git reset --hard HEAD"})));
    }

    #[test]
    fn bash_find_and_grep_are_not_destructive() {
        assert!(!is_destructive(
            "bash",
            &json!({"command": "find . -name '*.rs'"})
        ));
        assert!(!is_destructive("bash", &json!({"command": "ls -la"})));
        assert!(!is_destructive(
            "bash",
            &json!({"command": "git status --short"})
        ));
    }

    #[test]
    fn bash_shell_redirect_is_destructive() {
        assert!(is_destructive(
            "bash",
            &json!({"command": "echo hi > /tmp/out"})
        ));
        assert!(is_destructive(
            "bash",
            &json!({"command": "cat a.txt >> b.txt"})
        ));
    }

    #[test]
    fn should_vet_respects_off_mode() {
        let config = VetConfig {
            mode: VetMode::Off,
            read_sample_rate: 1.0,
            ..VetConfig::default()
        };
        assert!(!should_vet(
            &config,
            "bash",
            &json!({"command": "rm -rf /"}),
            0.0
        ));
    }

    #[test]
    fn should_vet_destructive_only_gates_on_classification() {
        let config = VetConfig {
            mode: VetMode::DestructiveOnly,
            read_sample_rate: 1.0,
            ..VetConfig::default()
        };
        assert!(should_vet(
            &config,
            "bash",
            &json!({"command": "git push"}),
            0.99
        ));
        assert!(!should_vet(
            &config,
            "bash",
            &json!({"command": "ls"}),
            0.0
        ));
    }

    #[test]
    fn should_vet_all_mode_samples_reads() {
        let config = VetConfig {
            mode: VetMode::All,
            read_sample_rate: 0.5,
            ..VetConfig::default()
        };
        // destructive always vetted regardless of roll
        assert!(should_vet(&config, "Write", &json!({}), 0.99));
        // read with roll below threshold: vet
        assert!(should_vet(&config, "Read", &json!({}), 0.4));
        // read with roll above threshold: skip
        assert!(!should_vet(&config, "Read", &json!({}), 0.6));
    }

    #[test]
    fn parse_approved_trims_whitespace() {
        assert_eq!(parse_vet_response("  APPROVED  "), VetDecision::Approve);
        assert_eq!(
            parse_vet_response("APPROVED\nnonsense trailing"),
            VetDecision::Approve
        );
    }

    #[test]
    fn parse_denied_extracts_reason_and_suggestion() {
        let text = "DENIED: greeting does not warrant a bash call\nSUGGEST: reply in plain text";
        assert_eq!(
            parse_vet_response(text),
            VetDecision::Deny {
                reason: "greeting does not warrant a bash call".to_string(),
                suggestion: Some("reply in plain text".to_string()),
            }
        );
    }

    #[test]
    fn parse_denied_without_suggestion_is_still_a_deny() {
        let text = "DENIED: the user asked a factual question about Rust";
        assert_eq!(
            parse_vet_response(text),
            VetDecision::Deny {
                reason: "the user asked a factual question about Rust".to_string(),
                suggestion: None,
            }
        );
    }

    #[test]
    fn parse_denied_without_reason_falls_to_unavailable() {
        assert!(matches!(
            parse_vet_response("DENIED:"),
            VetDecision::Unavailable(_)
        ));
    }

    #[test]
    fn parse_empty_or_garbage_is_unavailable() {
        assert!(matches!(
            parse_vet_response(""),
            VetDecision::Unavailable(_)
        ));
        assert!(matches!(
            parse_vet_response("sure, let me think..."),
            VetDecision::Unavailable(_)
        ));
    }

    #[test]
    fn deny_message_includes_suggestion_when_present() {
        let deny = VetDecision::Deny {
            reason: "greeting".to_string(),
            suggestion: Some("reply in text".to_string()),
        };
        let msg = deny.deny_message().expect("deny produces message");
        assert!(msg.contains("greeting"));
        assert!(msg.contains("reply in text"));
    }

    #[test]
    fn approve_produces_no_deny_message() {
        assert_eq!(VetDecision::Approve.deny_message(), None);
        assert_eq!(VetDecision::Skipped.deny_message(), None);
        assert_eq!(
            VetDecision::Unavailable("foo".to_string()).deny_message(),
            None
        );
    }

    #[test]
    fn deterministic_roll_is_stable_for_same_input() {
        let input = json!({"command": "rm"});
        assert_eq!(
            deterministic_roll("bash", &input),
            deterministic_roll("bash", &input)
        );
    }

    #[test]
    fn deterministic_roll_differs_for_different_inputs() {
        // Extremely unlikely to collide on 24-bit bucket.
        let a = deterministic_roll("Read", &json!({"path": "/a"}));
        let b = deterministic_roll("Read", &json!({"path": "/b"}));
        assert!((a - b).abs() > f32::EPSILON);
    }

    #[test]
    fn tool_vet_context_stores_and_returns_latest_message() {
        let ctx = ToolVetContext::new();
        assert_eq!(ctx.last_user_message(), None);
        ctx.set_last_user_message("hi");
        assert_eq!(ctx.last_user_message().as_deref(), Some("hi"));
        ctx.set_last_user_message("hello");
        assert_eq!(ctx.last_user_message().as_deref(), Some("hello"));
    }

    #[test]
    fn vet_config_without_api_key_returns_unavailable() {
        // Ensure the offline path short-circuits without touching the network.
        with_env_var("ANTHROPIC_API_KEY", None, || {
            let config = VetConfig::from_env();
            assert!(config.api_key.is_none());
            let decision = vet_tool_call(
                &config,
                "hi",
                "bash",
                &json!({"command": "find ."}),
            );
            match decision {
                VetDecision::Unavailable(reason) => {
                    assert_eq!(reason, "not-configured");
                }
                other => panic!("expected Unavailable, got {other:?}"),
            }
        });
    }

    #[test]
    fn build_vet_prompt_contains_user_message_and_tool_info() {
        let prompt = build_vet_prompt(
            "how do I list files",
            "bash",
            &json!({"command": "ls -la"}),
        );
        assert!(prompt.contains("how do I list files"));
        assert!(prompt.contains("tool = bash"));
        assert!(prompt.contains("ls -la"));
        assert!(prompt.contains("APPROVED"));
    }
}
