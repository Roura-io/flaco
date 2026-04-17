//! Agent registry — loads markdown-with-YAML-frontmatter agent files from
//! a directory at startup and exposes them by name for dispatch from slash
//! commands, mentions, and channel routing.
//!
//! Two frontmatter conventions are accepted:
//!
//! ### flacoAi native (used by `agents/homelab-sentinel.md`, `agents/code-reviewer.md`)
//!
//! ```markdown
//! ---
//! name: rust-reviewer
//! description: Reviews Rust code for idiomatic style, ownership, performance
//! tools: [bash, fs_read, grep]
//! model: qwen3:32b-q8_0
//! vetting: required
//! channels: [dev-*, code-review]
//! slash_commands: [/rust-review]
//! mention_patterns: [review this rust]
//! ---
//! body...
//! ```
//!
//! ### ECC / everything-claude-code style
//!
//! ```markdown
//! ---
//! name: rust-reviewer
//! description: Reviews Rust code for idiomatic style, ownership, performance
//! triggers:
//!   - slash_command: /rust-review
//!   - mention_pattern: review this rust
//!   - channel_pattern: dev-
//! tools: [read, grep, bash]
//! vet: true
//! ---
//! body...
//! ```
//!
//! The parser merges both conventions into a single [`Agent`] value. Any
//! combination is valid; fields from both shapes are additive.
//!
//! At startup, the server binary walks `~/.flaco/agents/` (and any additional
//! search paths it configures) and parses every `.md` file. Parse failures are
//! logged but don't abort startup — a single malformed agent shouldn't take
//! down the Slack bot.
//!
//! This module is intentionally decoupled from [`crate::gateway::ChannelPersona`]:
//! personas are keyed on channel routing ("what voice does this bot use in
//! this channel"), while agents are keyed on explicit invocation ("which
//! specialist handles this command"). A later refactor may unify the two
//! axes, but the pull-list goal for this cut is to ship the agent loader
//! without touching the persona dispatch path.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A declarative agent definition loaded from markdown+YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Unique name (used for dispatch, logging, and `/help` output).
    pub name: String,
    /// One-line description shown in agent catalog listings.
    pub description: String,
    /// Full prompt body — everything after the YAML frontmatter.
    /// Not part of the frontmatter itself, so it's skipped on (de)serialize.
    #[serde(skip)]
    pub prompt: String,
    /// Ways this agent can be triggered (union of all frontmatter shapes).
    #[serde(default)]
    pub triggers: Vec<AgentTrigger>,
    /// Tool names the agent is allowed to invoke (empty = no tool access).
    /// The runtime still enforces the global tool allowlist on top of this.
    #[serde(default)]
    pub tools: Vec<String>,
    /// How the claude_check vet layer should treat this agent's responses.
    #[serde(default)]
    pub vet: VetMode,
    /// Optional per-agent Ollama model override. When set, the gateway's
    /// tier dispatch honors this value instead of picking based on channel
    /// persona + user text length. Keeps the local-first routing in the
    /// user's control — independent of any ECC-style model-selection logic.
    #[serde(default)]
    pub model: Option<String>,
}

/// Vet layer policy for an agent. Richer than a bool so agents can opt
/// into "vet always", "vet only when the channel already vets", or "never
/// vet regardless of channel".
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum VetMode {
    /// Vet layer always runs on this agent's responses, regardless of
    /// channel persona. Use for correctness-sensitive agents (code review,
    /// security, infra).
    Required,
    /// Vet layer runs only when the channel persona opts in via
    /// `ChannelPersona::needs_vetting()`. Sensible default for agents
    /// that inherit the channel's policy.
    #[default]
    Optional,
    /// Vet layer never runs on this agent, even in channels that normally
    /// vet. Use for generative / conversational agents (planner, architect)
    /// where latency matters more than correctness cross-checking.
    Off,
}

/// How an agent can be invoked. ECC YAML expresses each variant as a
/// single-key map; the native flacoAi convention uses top-level `channels`,
/// `slash_commands`, and `mention_patterns` lists. Both are merged into
/// this typed form by [`parse_agent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTrigger {
    /// Slash command that dispatches to this agent (e.g. `/rust-review`).
    /// Leading `/` is optional and ignored at match time.
    SlashCommand(String),
    /// Case-insensitive substring matched against the user's message body.
    MentionPattern(String),
    /// Channel name prefix or exact match (e.g. `infra-` matches
    /// `infra-alerts`, `infra-status`). A trailing `*` is allowed and
    /// stripped — `dev-*` and `dev-` are equivalent.
    ChannelPattern(String),
}

/// Frontmatter shape — a superset of the two accepted conventions.
/// The parser accepts any field; fields from both shapes are merged.
///
/// Triggers come in via three channels:
/// - `triggers: [{slash_command: X}, ...]` (ECC style)
/// - `slash_commands: [X]`, `mention_patterns: [X]`, `channels: [X]` (flacoAi native)
///
/// The parser unions both into the `Agent::triggers` vector.
#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    name: String,
    description: String,
    // --- ECC-style trigger list (list of single-key maps) ---
    #[serde(default)]
    triggers: Vec<HashMap<String, String>>,
    // --- flacoAi-native top-level trigger lists ---
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    slash_commands: Vec<String>,
    #[serde(default)]
    mention_patterns: Vec<String>,
    // --- shared fields ---
    #[serde(default)]
    tools: Vec<String>,
    /// ECC-style boolean vet flag. Ignored if `vetting` is also present
    /// (the native enum takes precedence so the existing agent files can
    /// keep their `vetting: required|optional|off` convention).
    #[serde(default)]
    vet: Option<bool>,
    /// flacoAi-native vet enum: `required` | `optional` | `off`.
    /// Takes precedence over the ECC-style `vet` bool when both are set.
    #[serde(default)]
    vetting: Option<String>,
    /// Optional per-agent Ollama model override.
    #[serde(default)]
    model: Option<String>,
}

fn triggers_from_maps(raw: Vec<HashMap<String, String>>) -> Vec<AgentTrigger> {
    raw.into_iter()
        .filter_map(|m| {
            let (k, v) = m.into_iter().next()?;
            match k.as_str() {
                "slash_command" => Some(AgentTrigger::SlashCommand(v)),
                "mention_pattern" => Some(AgentTrigger::MentionPattern(v)),
                "channel_pattern" => Some(AgentTrigger::ChannelPattern(v)),
                other => {
                    tracing::warn!(
                        target: "agents",
                        key = other,
                        "unknown trigger key in agent frontmatter — skipping"
                    );
                    None
                }
            }
        })
        .collect()
}

/// Reconcile the two vet-config styles into a single `VetMode`.
///
/// Precedence: native `vetting` enum > ECC `vet` bool > default (`Optional`).
/// An unknown `vetting` string falls back to `Optional` with a warning so
/// typos don't silently drop an agent out of the vet pipeline.
fn vet_mode_from_fields(vet_bool: Option<bool>, vetting_str: Option<&str>) -> VetMode {
    if let Some(v) = vetting_str {
        return match v.to_ascii_lowercase().as_str() {
            "required" => VetMode::Required,
            "optional" => VetMode::Optional,
            "off" | "none" | "disabled" => VetMode::Off,
            other => {
                tracing::warn!(
                    target: "agents",
                    value = other,
                    "unknown `vetting` value — defaulting to optional"
                );
                VetMode::Optional
            }
        };
    }
    match vet_bool {
        Some(true) => VetMode::Required,
        Some(false) => VetMode::Off,
        None => VetMode::Optional,
    }
}

/// Normalize a channel pattern by stripping a trailing `*` so both
/// `dev-*` and `dev-` mean the same prefix match at dispatch time.
fn normalize_channel_pattern(p: &str) -> String {
    p.trim_end_matches('*').to_string()
}

/// Parse a single agent markdown-with-YAML-frontmatter file.
///
/// Format:
/// ```text
/// ---
/// <yaml frontmatter>
/// ---
/// <prompt body>
/// ```
///
/// Returns `Err` if:
/// - The file doesn't start with a `---` line
/// - The closing `---` line is missing
/// - The YAML frontmatter fails to parse
pub fn parse_agent(content: &str) -> Result<Agent, String> {
    let (yaml_chunk, body) = crate::frontmatter::split(content)?;

    let fm: AgentFrontmatter = serde_yml::from_str(yaml_chunk)
        .map_err(|e| format!("frontmatter YAML parse error: {e}"))?;

    // Merge triggers from all three input shapes.
    let mut triggers = triggers_from_maps(fm.triggers);
    for c in fm.channels {
        triggers.push(AgentTrigger::ChannelPattern(normalize_channel_pattern(&c)));
    }
    for s in fm.slash_commands {
        triggers.push(AgentTrigger::SlashCommand(s));
    }
    for m in fm.mention_patterns {
        triggers.push(AgentTrigger::MentionPattern(m));
    }
    // Normalize channel patterns that came in via `triggers: [{channel_pattern: X}]`
    // so the `*` suffix is stripped consistently with the native list form.
    for t in &mut triggers {
        if let AgentTrigger::ChannelPattern(p) = t {
            *p = normalize_channel_pattern(p);
        }
    }

    let vet = vet_mode_from_fields(fm.vet, fm.vetting.as_deref());

    Ok(Agent {
        name: fm.name,
        description: fm.description,
        prompt: body.trim_start().to_string(),
        triggers,
        tools: fm.tools,
        vet,
        model: fm.model,
    })
}

/// Walk a directory and load every `.md` file as an agent.
///
/// Parse failures are logged and skipped — a single malformed agent
/// shouldn't prevent the others from loading. Subdirectories are NOT
/// traversed; callers that want recursive loading should invoke this
/// function once per subdirectory.
#[must_use]
pub fn load_agents_from_dir(dir: &Path) -> HashMap<String, Agent> {
    let mut agents = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                target: "agents",
                dir = %dir.display(),
                error = %e,
                "agents dir not readable — starting with empty agent registry"
            );
            return agents;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "agents",
                    file = %path.display(),
                    error = %e,
                    "failed to read agent file"
                );
                continue;
            }
        };
        match parse_agent(&content) {
            Ok(agent) => {
                tracing::info!(
                    target: "agents",
                    name = %agent.name,
                    triggers = agent.triggers.len(),
                    tools = agent.tools.len(),
                    vet = ?agent.vet,
                    model = ?agent.model,
                    "loaded agent"
                );
                agents.insert(agent.name.clone(), agent);
            }
            Err(e) => {
                tracing::warn!(
                    target: "agents",
                    file = %path.display(),
                    error = %e,
                    "failed to parse agent file — skipping"
                );
            }
        }
    }
    agents
}

/// Look up the agent a slash command should dispatch to.
/// Returns `None` if no agent declares a matching `SlashCommand` trigger.
/// Matching is case-insensitive and ignores a leading `/`.
#[must_use]
pub fn agent_for_slash_command<'a, S: BuildHasher>(
    agents: &'a HashMap<String, Agent, S>,
    command: &str,
) -> Option<&'a Agent> {
    let normalized = command.trim_start_matches('/').to_ascii_lowercase();
    agents.values().find(|a| {
        a.triggers.iter().any(|t| match t {
            AgentTrigger::SlashCommand(s) => {
                s.trim_start_matches('/')
                    .eq_ignore_ascii_case(&normalized)
            }
            _ => false,
        })
    })
}

/// Look up the agent a channel name should auto-route to, if any.
/// Matches the first agent whose `ChannelPattern` is either an exact
/// match or a prefix of the channel name (case-insensitive).
#[must_use]
pub fn agent_for_channel<'a, S: BuildHasher>(
    agents: &'a HashMap<String, Agent, S>,
    channel_name: &str,
) -> Option<&'a Agent> {
    let n = channel_name.to_ascii_lowercase();
    agents.values().find(|a| {
        a.triggers.iter().any(|t| match t {
            AgentTrigger::ChannelPattern(p) => {
                let pl = p.to_ascii_lowercase();
                n == pl || n.starts_with(&pl)
            }
            _ => false,
        })
    })
}

/// Look up an agent by a mention pattern in free-text user input.
/// Returns the first agent whose `MentionPattern` substring appears in
/// the lowercased input.
#[must_use]
pub fn agent_for_mention<'a, S: BuildHasher>(
    agents: &'a HashMap<String, Agent, S>,
    user_text: &str,
) -> Option<&'a Agent> {
    let t = user_text.to_ascii_lowercase();
    agents.values().find(|a| {
        a.triggers.iter().any(|tr| match tr {
            AgentTrigger::MentionPattern(p) => t.contains(&p.to_ascii_lowercase()),
            _ => false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_agent() {
        let src = "---\nname: tiny\ndescription: smallest possible agent\n---\nYou do one thing.";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.name, "tiny");
        assert_eq!(a.description, "smallest possible agent");
        assert_eq!(a.prompt, "You do one thing.");
        assert!(a.triggers.is_empty());
        assert!(a.tools.is_empty());
        assert_eq!(a.vet, VetMode::Optional);
        assert!(a.model.is_none());
    }

    #[test]
    fn parses_ecc_style_triggers_and_tools() {
        let src = "---\n\
name: rust-reviewer\n\
description: Reviews Rust code for idiomatic style\n\
triggers:\n\
  - slash_command: /rust-review\n\
  - mention_pattern: review this rust\n\
tools:\n\
  - read\n\
  - grep\n\
vet: true\n\
---\n\
You are an expert Rust reviewer. Focus on ownership, lifetimes, and clippy.";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.name, "rust-reviewer");
        assert_eq!(a.triggers.len(), 2);
        assert_eq!(a.tools, vec!["read".to_string(), "grep".to_string()]);
        assert_eq!(a.vet, VetMode::Required);
        assert!(a.prompt.contains("ownership"));
    }

    #[test]
    fn parses_flacoai_native_channels_and_vetting() {
        let src = "---\n\
name: homelab-sentinel\n\
description: Answers homelab state questions\n\
tools: [bash, fs_read, web_fetch]\n\
model: qwen3:32b-q8_0\n\
vetting: required\n\
channels: [home-general, infra-alerts, network-*, home-*]\n\
---\n\
body";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.name, "homelab-sentinel");
        assert_eq!(a.vet, VetMode::Required);
        assert_eq!(a.model, Some("qwen3:32b-q8_0".to_string()));
        assert_eq!(a.tools.len(), 3);
        // 4 channel triggers, `*` stripped.
        let chan_count = a
            .triggers
            .iter()
            .filter(|t| matches!(t, AgentTrigger::ChannelPattern(_)))
            .count();
        assert_eq!(chan_count, 4);
        // `network-*` → `network-`
        assert!(a.triggers.iter().any(|t| matches!(
            t,
            AgentTrigger::ChannelPattern(s) if s == "network-"
        )));
    }

    #[test]
    fn parses_mixed_ecc_and_native_conventions() {
        let src = "---\n\
name: rust-reviewer\n\
description: mixed format\n\
channels: [dev-*]\n\
slash_commands: [/rust-review]\n\
mention_patterns: [review this rust]\n\
triggers:\n\
  - channel_pattern: code-review\n\
vetting: optional\n\
---\n\
body";
        let a = parse_agent(src).unwrap();
        // 1 channel from `channels:`, 1 slash from `slash_commands:`,
        // 1 mention from `mention_patterns:`, 1 channel from `triggers:`
        assert_eq!(a.triggers.len(), 4);
        assert_eq!(a.vet, VetMode::Optional);
        assert!(a.triggers.iter().any(|t| matches!(
            t,
            AgentTrigger::ChannelPattern(s) if s == "dev-"
        )));
        assert!(a.triggers.iter().any(|t| matches!(
            t,
            AgentTrigger::SlashCommand(s) if s == "/rust-review"
        )));
    }

    #[test]
    fn vetting_off_overrides_default_optional() {
        let src = "---\nname: planner\ndescription: d\nvetting: off\n---\nbody";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.vet, VetMode::Off);
    }

    #[test]
    fn vetting_enum_takes_precedence_over_vet_bool() {
        // `vetting: off` should win even if `vet: true` is also set.
        let src = "---\nname: a\ndescription: d\nvet: true\nvetting: off\n---\nbody";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.vet, VetMode::Off);
    }

    #[test]
    fn rejects_missing_frontmatter() {
        let src = "no frontmatter here";
        assert!(parse_agent(src).is_err());
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        let src = "---\nname: broken\ndescription: never closes\nbody body body";
        assert!(parse_agent(src).is_err());
    }

    #[test]
    fn accepts_eof_closer_without_trailing_newline() {
        let src = "---\nname: terse\ndescription: d\n---";
        let a = parse_agent(src).unwrap();
        assert_eq!(a.name, "terse");
        assert_eq!(a.prompt, "");
    }

    #[test]
    fn slash_command_dispatch_is_case_insensitive_and_slash_optional() {
        let mut agents = HashMap::new();
        let src = "---\nname: rust-reviewer\ndescription: d\ntriggers:\n  - slash_command: /rust-review\n---\nbody";
        let a = parse_agent(src).unwrap();
        agents.insert(a.name.clone(), a);

        assert!(agent_for_slash_command(&agents, "/rust-review").is_some());
        assert!(agent_for_slash_command(&agents, "/Rust-Review").is_some());
        assert!(agent_for_slash_command(&agents, "rust-review").is_some());
        assert!(agent_for_slash_command(&agents, "/unknown").is_none());
    }

    #[test]
    fn channel_pattern_dispatch_matches_prefix_and_exact() {
        let mut agents = HashMap::new();
        let src = "---\nname: infra-sentinel\ndescription: d\ntriggers:\n  - channel_pattern: infra-\n---\nbody";
        let a = parse_agent(src).unwrap();
        agents.insert(a.name.clone(), a);

        assert!(agent_for_channel(&agents, "infra-alerts").is_some());
        assert!(agent_for_channel(&agents, "infra-status").is_some());
        assert!(agent_for_channel(&agents, "INFRA-ALERTS").is_some());
        assert!(agent_for_channel(&agents, "home-general").is_none());
    }

    #[test]
    fn channel_pattern_star_suffix_is_equivalent_to_prefix() {
        let mut agents = HashMap::new();
        let src = "---\nname: a\ndescription: d\nchannels: [network-*]\n---\nbody";
        let a = parse_agent(src).unwrap();
        agents.insert(a.name.clone(), a);

        assert!(agent_for_channel(&agents, "network-alerts").is_some());
        assert!(agent_for_channel(&agents, "network-status").is_some());
    }

    #[test]
    fn mention_pattern_dispatch_is_substring_and_case_insensitive() {
        let mut agents = HashMap::new();
        let src = "---\nname: python-reviewer\ndescription: d\ntriggers:\n  - mention_pattern: review this python\n---\nbody";
        let a = parse_agent(src).unwrap();
        agents.insert(a.name.clone(), a);

        assert!(agent_for_mention(&agents, "Hey can you review this python for me?").is_some());
        assert!(agent_for_mention(&agents, "REVIEW THIS PYTHON please").is_some());
        assert!(agent_for_mention(&agents, "unrelated message").is_none());
    }

    #[test]
    fn load_agents_from_dir_gracefully_handles_missing_dir() {
        let agents = load_agents_from_dir(Path::new("/nonexistent/path/zzz"));
        assert!(agents.is_empty());
    }

    #[test]
    fn parses_real_baseline_agent_files() {
        // Smoke test — verify every `.md` file under `flacoAi/agents/`
        // parses with the dual-format parser. Path is relative to the
        // channels crate manifest dir, walking up to the flacoAi repo root.
        let manifest = env!("CARGO_MANIFEST_DIR");
        let dir = Path::new(manifest).join("../../../agents");
        if !dir.exists() {
            eprintln!("skipping: {} does not exist", dir.display());
            return;
        }

        // Count the `.md` files on disk so we can assert that every one
        // of them made it into the registry (a parse failure would be
        // logged and silently skipped by `load_agents_from_dir`).
        let on_disk_md: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        let agents = load_agents_from_dir(&dir);
        assert_eq!(
            agents.len(),
            on_disk_md.len(),
            "expected all {} .md files in {} to parse; got {}. Registry keys: {:?}",
            on_disk_md.len(),
            dir.display(),
            agents.len(),
            agents.keys().collect::<Vec<_>>()
        );

        // Every baseline agent we committed must be present by name.
        let required = [
            "homelab-sentinel",
            "code-reviewer",
            "rust-reviewer",
            "python-reviewer",
            "architect",
            "tdd-guide",
            "security-reviewer",
            "doc-updater",
            "planner",
            "chief-of-staff",
        ];
        for name in required {
            assert!(
                agents.contains_key(name),
                "missing expected agent `{name}`; registry keys: {:?}",
                agents.keys().collect::<Vec<_>>()
            );
        }

        // Spot-check specific fields on two agents to verify the
        // native-format + per-field parsing actually ran (not just that
        // the file loaded).
        let sentinel = &agents["homelab-sentinel"];
        assert_eq!(sentinel.vet, VetMode::Required);
        assert_eq!(sentinel.model.as_deref(), Some("qwen3:32b-q8_0"));
        assert!(
            sentinel.triggers.len() >= 4,
            "homelab-sentinel should have at least 4 channel triggers"
        );

        let rust = &agents["rust-reviewer"];
        assert_eq!(rust.vet, VetMode::Required);
        assert_eq!(rust.model.as_deref(), Some("qwen3:32b-q8_0"));
        assert!(
            rust.triggers.iter().any(|t| matches!(
                t,
                AgentTrigger::SlashCommand(s) if s == "/rust-review"
            )),
            "rust-reviewer should have /rust-review slash trigger"
        );
    }
}
