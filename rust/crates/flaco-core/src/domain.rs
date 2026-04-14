//! Domain context routing.
//!
//! The "my X" pattern: when the user says "my network", "my tickets",
//! "my PRs", etc., flaco needs to know exactly which API, which auth,
//! which source-of-truth file, and which tools are relevant. Teaching
//! that once per domain — centrally, in this module — means users
//! never have to spell out credentials, endpoints, or file paths in
//! their prompts. They say `"check my unifi"` and it works.
//!
//! Each `DomainSpec` declares:
//!
//! - `name` — short identifier, also used for log lines.
//! - `env_keys` — required env vars (preflight fails if missing).
//! - `optional_env` — nice-to-have vars, no preflight gate.
//! - `system_prompt_stanza` — the domain-specific context pasted into
//!   the model's system prompt at turn-start.
//! - `ground_truth_files` — files auto-read + injected into the prompt
//!   so the model has the source of truth before it reasons.
//! - `preferred_tools` — subset of the registry the runtime passes to
//!   the LLM for this domain (empty = all tools).
//!
//! `classify_message` does cheap keyword routing first (zero-cost, 0ms),
//! falling back to an LLM only for ambiguous cases (not yet wired —
//! keyword coverage is enough for the launch set). `preflight` checks
//! the env and returns a user-friendly refusal string if a required
//! secret isn't available, so we can short-circuit before burning an
//! LLM call.

use std::fmt;

/// All domains flaco knows how to pre-load context for. `General` is
/// the fall-through for everything that doesn't match a specific
/// domain — chit-chat, creative writing, one-off questions — and it
/// gets no special stanza.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Unifi,
    Jira,
    GitHub,
    Figma,
    Homelab,
    UnasSave,
    General,
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.spec().name)
    }
}

impl Domain {
    pub fn spec(&self) -> &'static DomainSpec {
        match self {
            Domain::Unifi => &UNIFI,
            Domain::Jira => &JIRA,
            Domain::GitHub => &GITHUB,
            Domain::Figma => &FIGMA,
            Domain::Homelab => &HOMELAB,
            Domain::UnasSave => &UNAS_SAVE,
            Domain::General => &GENERAL,
        }
    }

    /// All domains in iteration order. Used for doctor / grading /
    /// listing in the help card.
    pub fn all() -> &'static [Domain] {
        &[
            Domain::Unifi,
            Domain::Jira,
            Domain::GitHub,
            Domain::Figma,
            Domain::Homelab,
            Domain::UnasSave,
            Domain::General,
        ]
    }
}

#[derive(Debug)]
pub struct DomainSpec {
    pub name: &'static str,
    pub env_keys: &'static [&'static str],
    pub optional_env: &'static [&'static str],
    pub system_prompt_stanza: &'static str,
    pub ground_truth_files: &'static [&'static str],
    pub preferred_tools: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// Individual domain specs
// ---------------------------------------------------------------------------

const UNIFI: DomainSpec = DomainSpec {
    name: "unifi",
    env_keys: &["UNIFI_API_KEY"],
    optional_env: &["UNIFI_SITE_ID", "UNIFI_HOST_ID"],
    system_prompt_stanza: r#"
## UniFi / home network

The user's home network is managed by UniFi. When they ask about
"my network", "my wifi", "my UniFi", "my router", "my UDM", "my APs",
or anything infrastructure-shaped, use the UniFi cloud API.

AUTH: every request needs header `X-API-KEY: $UNIFI_API_KEY`.
The key is ALREADY in your env as `UNIFI_API_KEY`. Never ask the user
for it. Never tell them what it is.

CORE ENDPOINTS:
- List sites:   curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/sites
- List hosts:   curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/hosts
- Site detail:  curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/sites/$UNIFI_SITE_ID
- Host detail:  curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/hosts/$UNIFI_HOST_ID

The site stats object has counts for: `criticalNotification`,
`offlineDevice`, `offlineGatewayDevice`, `offlineWifiDevice`,
`pendingUpdateDevice`, `totalDevice`, `wifiClient`, `wiredClient`.
Any non-zero value on the "offline*" or "*Notification" fields is
a real finding worth reporting.

GROUND TRUTH: `~/Documents/pi-projects/infra/network-state.yaml` is
the user's declared "known-good" network state. The runtime has
already read it — see the "source of truth" section below — before
reporting drift. Anything in UniFi that isn't in the YAML (or vice
versa) is drift worth calling out.

WHAT "MY X" MEANS:
- "my network" / "my wifi" / "my unifi"   → the Default site
- "my gateway" / "my UDM"                 → UDM-SE-Rouraio host
- "my storage" / "my NAS" / "my unas"     → UNAS-Pro-8 host
"#,
    // Relative paths get $HOME prepended at read time so the same
    // spec works on both the user's daily Mac (home = roura.io) and
    // mac-server (home = roura.io.server) without hardcoding usernames.
    ground_truth_files: &["~/Documents/pi-projects/infra/network-state.yaml"],
    preferred_tools: &["bash", "fs_read"],
};

const JIRA: DomainSpec = DomainSpec {
    name: "jira",
    env_keys: &["JIRA_API_TOKEN", "JIRA_EMAIL", "JIRA_URL"],
    optional_env: &[],
    system_prompt_stanza: r#"
## Jira / dev tickets

The user's Jira lives at `$JIRA_URL` (currently https://rouraio.atlassian.net).
When they ask about "my tickets", "my sprint", "my backlog", "my plate",
"my standup", or anything ticket-shaped, use the Jira REST API.

AUTH: Basic auth with base64 of `$JIRA_EMAIL:$JIRA_API_TOKEN`.
Both values are ALREADY in your env. Never ask the user.

KEY JQL:
- My open issues:    `assignee = currentUser() AND statusCategory != Done`
- My today:          `assignee = currentUser() AND updated >= -1d`
- My overdue:        `assignee = currentUser() AND due < now()`

Always use `assignee = currentUser()` — never ask the user's username
or accountId. The API resolves "currentUser" from the auth token.

ENDPOINT PATTERN:
  curl -s -u "$JIRA_EMAIL:$JIRA_API_TOKEN" \
    -H "Accept: application/json" \
    "$JIRA_URL/rest/api/3/search?jql=<encoded-jql>&fields=summary,status,priority,duedate"

WHAT "MY X" MEANS:
- "my tickets" / "my backlog" / "my plate" → my open (non-Done) issues
- "my sprint"                              → my issues in the active sprint
"#,
    ground_truth_files: &[],
    preferred_tools: &["bash"],
};

const GITHUB: DomainSpec = DomainSpec {
    name: "github",
    env_keys: &["GITHUB_TOKEN"],
    optional_env: &[],
    system_prompt_stanza: r#"
## GitHub

When the user asks about "my PRs", "my repos", "my branches", "my CI",
"my actions", or anything GitHub-shaped, use the GitHub REST API.

AUTH: `Authorization: Bearer $GITHUB_TOKEN` header. Already in env.

KEY ENDPOINTS:
- My open PRs (authored): curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/search/issues?q=is:pr+is:open+author:@me"
- My PRs needing review:   curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/search/issues?q=is:pr+is:open+review-requested:@me"
- My repos:                curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/user/repos?affiliation=owner&sort=updated&per_page=20"
- Workflow runs on a repo: curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/repos/$OWNER/$REPO/actions/runs?per_page=10"

Always use `@me` for "my" — never ask the username.

WHAT "MY X" MEANS:
- "my PRs" / "my pulls"  → search is:pr is:open author:@me
- "my repos"             → /user/repos?affiliation=owner
- "my reviews"           → search review-requested:@me
"#,
    ground_truth_files: &[],
    preferred_tools: &["bash"],
};

const FIGMA: DomainSpec = DomainSpec {
    name: "figma",
    env_keys: &["FIGMA_ACCESS_TOKEN"],
    optional_env: &[],
    system_prompt_stanza: r#"
## Figma

When the user asks about "my design", "my figma file", "my mockup",
or "my prototype", use the Figma REST API.

AUTH: `X-Figma-Token: $FIGMA_ACCESS_TOKEN` header. Already in env.

KEY ENDPOINTS:
- My teams:        curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" \
                     "https://api.figma.com/v1/me"
- Team projects:   curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" \
                     "https://api.figma.com/v1/teams/$TEAM_ID/projects"
- Project files:   curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" \
                     "https://api.figma.com/v1/projects/$PROJECT_ID/files"
- File metadata:   curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" \
                     "https://api.figma.com/v1/files/$FILE_KEY?depth=1"

If the user gives you a figma.com URL, parse out the file key (the
segment after `/file/` or `/design/`) and hit the metadata endpoint.
"#,
    ground_truth_files: &[],
    preferred_tools: &["bash"],
};

const HOMELAB: DomainSpec = DomainSpec {
    name: "homelab",
    env_keys: &[],
    optional_env: &[],
    system_prompt_stanza: r#"
## Homelab (Pi / Mac server / VPS / UNAS)

When the user asks about "my pi", "my mac server", "my vps", "my homelab",
"my infrastructure", or anything about the physical nodes, use SSH
(aliases are configured in ~/.ssh/config).

SSH ALIASES (all available via `bash`):
- `ssh pi`         → Raspberry Pi at pi.home / 10.0.1.4 (rouraio user)
- `ssh mac-server` → M1 Pro Mac at mac.home / 10.0.1.3 (roura.io.server user)
- `ssh vps`        → Hostinger VPS at 72.60.173.8 (root)

REACHABLE SERVICES (no SSH needed, just curl):
- `http://pi.home:5678`  → n8n
- `http://pi.home:3002`  → Grafana
- `http://pi.home:3001`  → Uptime Kuma
- `http://pi.home:9090`  → Prometheus
- `http://pi.home:8123`  → Home Assistant
- `http://mac.home:3033` → flaco-v2 web UI
- `http://mac.home:11434` → Ollama
- `http://ollama.home:11434` → Ollama (alias)

GROUND TRUTH:
- `~/Documents/pi-projects/infra/network-state.yaml` — declared state
  of all nodes, IPs, and services
- `~/Documents/pi-projects/PROJECT.md` — vision
- `~/Documents/pi-projects/ARCHITECTURE.md` — current spec
- `~/Documents/pi-projects/scripts/smoke-test.sh` — a
  19-check health probe the user runs before/after any infra change.
  You can invoke it and report the output.

`flaco doctor` on mac-server is the fastest health readback (10 checks).
For anything unusual, run it first.
"#,
    ground_truth_files: &["~/Documents/pi-projects/infra/network-state.yaml"],
    preferred_tools: &["bash", "fs_read"],
};

const UNAS_SAVE: DomainSpec = DomainSpec {
    name: "unas_save",
    env_keys: &[],
    optional_env: &["FLACO_UNAS_MOUNT", "FLACO_UNAS_USER_MAP"],
    system_prompt_stanza: r#"
## Saving artifacts to the UNAS

When the user asks you to "save this", "put this in my unas", or wants
a file written somewhere persistent, use the `save_to_unas` typed tool.
Do NOT use bash + mkdir/cat for saves — the tool is safer (path
sanitization, per-user folder routing, structured output) and more
auditable.

Tool call shape:
  save_to_unas(
    user_id: "<canonical id of the requesting user>",
    category: one of [shortcuts, research, scaffolds, notes, drafts, other],
    filename: "<name-with-extension>",
    content: "<full text/markdown body>"
  )

Per-user folder routing happens automatically via FLACO_UNAS_USER_MAP.
You only supply the user_id; the tool resolves it to the right folder
on the shared drive (cjroura, wroura, etc). Picked categories will
land under /Volumes/Roura.io/<folder>/flaco/<category>/<filename> and
the tool returns a Finder-friendly path plus an smb:// URL the user
can open from any Apple device.

If the tool reports the mount is missing, NEVER fall back to writing
to the local mac-server filesystem. Tell the user the UNAS mount is
down and surface the error. Silent local fallback = silent data loss.
"#,
    ground_truth_files: &[],
    preferred_tools: &["save_to_unas"],
};

const GENERAL: DomainSpec = DomainSpec {
    name: "general",
    env_keys: &[],
    optional_env: &[],
    system_prompt_stanza: "",
    ground_truth_files: &[],
    preferred_tools: &[],
};

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify a user message into a domain. Cheap keyword matching —
/// 0ms, no LLM, no tokens. Covers the high-signal phrases that matter
/// for the v1 launch; ambiguous messages fall through to `General`
/// which keeps today's behavior (no stanza injected, full tool set,
/// no ground-truth auto-read).
pub fn classify_message(text: &str) -> Domain {
    let lower = text.to_ascii_lowercase();

    // Order matters — Homelab has to come before any generic infra match,
    // and Unifi has to come before any generic "network" match, because
    // the more specific domain wins.
    if any_contains(&lower, UNIFI_KEYWORDS) {
        return Domain::Unifi;
    }
    if any_contains(&lower, JIRA_KEYWORDS) {
        return Domain::Jira;
    }
    if any_contains(&lower, GITHUB_KEYWORDS) {
        return Domain::GitHub;
    }
    if any_contains(&lower, FIGMA_KEYWORDS) {
        return Domain::Figma;
    }
    if any_contains(&lower, HOMELAB_KEYWORDS) {
        return Domain::Homelab;
    }
    if any_contains(&lower, UNAS_SAVE_KEYWORDS) {
        return Domain::UnasSave;
    }
    Domain::General
}

/// Returns true if the message intends a UNAS save alongside its
/// primary intent. Callers should stack the `UnasSave` stanza on top
/// of whatever primary domain fired — e.g. "research X and save it
/// to my unas" gets both the research context AND the save recipe.
pub fn also_wants_save(text: &str) -> bool {
    any_contains(&text.to_ascii_lowercase(), UNAS_SAVE_KEYWORDS)
}

fn any_contains(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

// Keyword lists are at module scope so the test module can assert
// on the exact phrases each domain claims — prevents silent drift
// when new phrasings are added or old ones are removed.

const UNIFI_KEYWORDS: &[&str] = &[
    "unifi",
    "udm",
    "my network",
    "my wifi",
    "my wi-fi",
    "my router",
    "my ap ",
    "my aps",
    "my access point",
    "my lan",
    "my vlan",
    "my gateway",
    "my ssid",
];

const JIRA_KEYWORDS: &[&str] = &[
    "jira",
    "my tickets",
    "my ticket",
    "my sprint",
    "my backlog",
    "my plate",
    "my standup",
    "standup from",
    "issue tracker",
];

const GITHUB_KEYWORDS: &[&str] = &[
    "github",
    "my prs",
    "my pr ",
    "my pull requests",
    "my pulls",
    "my repos",
    "my repositories",
    "my reviews",
    "my ci",
    "my actions",
    "my workflows",
];

const FIGMA_KEYWORDS: &[&str] = &[
    "figma",
    "my design",
    "my mockup",
    "my prototype",
    "figma.com",
];

const HOMELAB_KEYWORDS: &[&str] = &[
    "my pi",
    "my raspberry",
    "my mac server",
    "my macserver",
    "my vps",
    "my homelab",
    "my home lab",
    "my infrastructure",
    "my infra",
    "pi.home",
    "mac.home",
    "ollama.home",
];

const UNAS_SAVE_KEYWORDS: &[&str] = &[
    "save this",
    "save it to",
    "save to my unas",
    "save to the unas",
    "save to unas",
    "save to my folder",
    "put this in my unas",
    "put it in my unas",
    "put in my unas",
    "write to my unas",
    "write this to my unas",
    "stash this in",
    "drop this in my unas",
    "save as a file",
    "save as md",
    "save as markdown",
    "my unas",
    "my nas",
];

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

/// Check whether the current process env has everything a domain
/// requires. Returns `Ok(())` if ready, or `Err(message)` with a
/// human-readable string explaining what's missing. The runtime short-
/// circuits the LLM call and replies with that string directly.
pub fn preflight(domain: Domain) -> Result<(), String> {
    let spec = domain.spec();
    let missing: Vec<&str> = spec
        .env_keys
        .iter()
        .filter(|k| std::env::var(k).map(|v| v.is_empty()).unwrap_or(true))
        .copied()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "I can't do {} work right now — {} {} not set in my environment. \
         Ask me something else, or restart flaco with {} exported.",
        spec.name,
        missing.join(", "),
        if missing.len() == 1 { "is" } else { "are" },
        missing.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// System-prompt assembly
// ---------------------------------------------------------------------------

/// Expand a leading `~/` to the current user's home directory. Leaves
/// everything else alone. Used so `DomainSpec` can ship portable paths
/// like `~/Documents/...` that work across every Mac / Linux box this
/// agent runs on without hardcoding usernames.
fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

/// Read the ground-truth files for a domain (if any) and truncate each
/// to a reasonable size. Returns a single concatenated string ready to
/// paste into the system prompt under a "Source of truth" header.
/// Reading errors are silently dropped — missing files are fine, we
/// just don't inject that context.
pub fn read_ground_truth(domain: Domain) -> String {
    let spec = domain.spec();
    if spec.ground_truth_files.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n## Source of truth\n");
    for path in spec.ground_truth_files {
        let resolved = expand_home(path);
        if let Ok(content) = std::fs::read_to_string(&resolved) {
            let truncated = truncate_at_char_boundary(&content, 2500);
            out.push_str(&format!("\n### {resolved}\n```\n{truncated}\n```\n"));
        }
    }
    out
}

fn truncate_at_char_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n\n[... truncated, {} total bytes]", &s[..cut], s.len())
}

/// Full context builder: given a classified domain, return everything
/// the runtime should paste into the system prompt — the stanza plus
/// the ground-truth file contents. Empty string if the domain is
/// `General` or has no content to inject.
pub fn build_context(domain: Domain) -> String {
    let spec = domain.spec();
    let mut out = String::new();
    if !spec.system_prompt_stanza.is_empty() {
        out.push_str(spec.system_prompt_stanza);
    }
    out.push_str(&read_ground_truth(domain));
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Classification ----

    #[test]
    fn classify_unifi_basic() {
        assert_eq!(classify_message("check my unifi"), Domain::Unifi);
        assert_eq!(classify_message("is my network healthy?"), Domain::Unifi);
        assert_eq!(classify_message("what's wrong with my wifi"), Domain::Unifi);
        assert_eq!(classify_message("my UDM has a problem"), Domain::Unifi);
    }

    #[test]
    fn classify_jira_basic() {
        assert_eq!(classify_message("what are my tickets today"), Domain::Jira);
        assert_eq!(classify_message("write my standup"), Domain::Jira);
        assert_eq!(classify_message("show me my backlog"), Domain::Jira);
        assert_eq!(classify_message("what's on my plate"), Domain::Jira);
    }

    #[test]
    fn classify_github_basic() {
        assert_eq!(classify_message("what are my prs"), Domain::GitHub);
        assert_eq!(
            classify_message("anything waiting on my reviews"),
            Domain::GitHub
        );
        assert_eq!(classify_message("how's github today"), Domain::GitHub);
    }

    #[test]
    fn classify_figma_basic() {
        assert_eq!(
            classify_message("pull my design for the onboarding"),
            Domain::Figma
        );
        assert_eq!(
            classify_message("https://www.figma.com/design/abc/title"),
            Domain::Figma
        );
    }

    #[test]
    fn classify_homelab_basic() {
        assert_eq!(classify_message("ssh into my pi"), Domain::Homelab);
        assert_eq!(classify_message("is my mac server up"), Domain::Homelab);
        assert_eq!(classify_message("reboot my vps"), Domain::Homelab);
        assert_eq!(classify_message("my homelab status"), Domain::Homelab);
    }

    #[test]
    fn classify_save_to_unas_standalone() {
        assert_eq!(classify_message("save this to my unas"), Domain::UnasSave);
        assert_eq!(classify_message("put this in my unas"), Domain::UnasSave);
        assert_eq!(classify_message("stash this in my unas"), Domain::UnasSave);
    }

    #[test]
    fn also_wants_save_detects_mixed_intent() {
        assert!(also_wants_save("research this and save it to my unas"));
        assert!(also_wants_save("check my network and save it to my unas"));
        assert!(!also_wants_save("just a quick question"));
    }

    #[test]
    fn primary_domain_wins_when_mixed_with_save() {
        // "check my unifi and save it to my unas" should classify as
        // Unifi (the primary action) but also_wants_save should return
        // true so the caller can stack the UnasSave stanza on top.
        assert_eq!(
            classify_message("check my unifi and save it to my unas"),
            Domain::Unifi
        );
        assert!(also_wants_save("check my unifi and save it to my unas"));
    }

    #[test]
    fn unas_save_spec_mentions_save_to_unas_tool() {
        // Anti-regression: the UnasSave stanza must direct the model to
        // use the typed `save_to_unas` tool, NOT ad-hoc bash writes.
        let stanza = UNAS_SAVE.system_prompt_stanza;
        assert!(stanza.contains("save_to_unas"));
        assert!(stanza.contains("Per-user folder routing"));
        assert!(stanza.to_lowercase().contains("never"));
    }

    #[test]
    fn classify_general_fallthrough() {
        assert_eq!(classify_message("hello"), Domain::General);
        assert_eq!(classify_message("write me a limerick"), Domain::General);
        assert_eq!(
            classify_message("who won the yankees game"),
            Domain::General
        );
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(classify_message("CHECK MY UNIFI"), Domain::Unifi);
        assert_eq!(classify_message("MY Jira"), Domain::Jira);
    }

    #[test]
    fn unifi_beats_general_network_words() {
        // "network" alone is too generic and shouldn't trigger Unifi —
        // only the specific phrasings do.
        assert_eq!(classify_message("help me with a network test"), Domain::General);
        assert_eq!(classify_message("my network"), Domain::Unifi);
    }

    // ---- Preflight ----

    #[test]
    fn preflight_general_always_ok() {
        // Even with no env set, General requires nothing.
        assert!(preflight(Domain::General).is_ok());
    }

    #[test]
    fn preflight_flags_missing_env() {
        // Save + clear + restore so this test doesn't leak env.
        let key = "UNIFI_API_KEY";
        let saved = std::env::var(key).ok();
        std::env::remove_var(key);

        let result = preflight(Domain::Unifi);
        assert!(result.is_err(), "preflight should fail when key is missing");
        let err = result.unwrap_err();
        assert!(err.contains("UNIFI_API_KEY"));
        assert!(err.contains("unifi"));

        if let Some(v) = saved {
            std::env::set_var(key, v);
        }
    }

    #[test]
    fn preflight_passes_when_env_set() {
        let key = "UNIFI_API_KEY";
        let saved = std::env::var(key).ok();
        std::env::set_var(key, "test-key-value");

        let result = preflight(Domain::Unifi);
        assert!(result.is_ok());

        if let Some(v) = saved {
            std::env::set_var(key, v);
        } else {
            std::env::remove_var(key);
        }
    }

    // ---- DomainSpec contents ----

    #[test]
    fn every_domain_has_a_spec() {
        for d in Domain::all() {
            let spec = d.spec();
            assert!(!spec.name.is_empty());
        }
    }

    #[test]
    fn unifi_stanza_mentions_key_and_endpoint() {
        let stanza = UNIFI.system_prompt_stanza;
        assert!(stanza.contains("UNIFI_API_KEY"));
        assert!(stanza.contains("api.ui.com"));
        assert!(stanza.contains("network-state.yaml"));
    }

    #[test]
    fn jira_stanza_uses_current_user() {
        assert!(JIRA.system_prompt_stanza.contains("currentUser()"));
    }

    #[test]
    fn github_stanza_uses_at_me() {
        assert!(GITHUB.system_prompt_stanza.contains("@me"));
    }

    #[test]
    fn homelab_mentions_ssh_aliases() {
        let stanza = HOMELAB.system_prompt_stanza;
        assert!(stanza.contains("ssh pi"));
        assert!(stanza.contains("ssh mac-server"));
        assert!(stanza.contains("ssh vps"));
    }

    // ---- Context assembly ----

    #[test]
    fn build_context_general_is_empty() {
        assert!(build_context(Domain::General).is_empty());
    }

    #[test]
    fn build_context_unifi_has_stanza_even_without_ground_truth() {
        // The ground-truth file may or may not exist in the test env,
        // but the stanza is hard-coded and should always appear.
        let ctx = build_context(Domain::Unifi);
        assert!(ctx.contains("UniFi"));
        assert!(ctx.contains("UNIFI_API_KEY"));
    }

    #[test]
    fn expand_home_replaces_tilde() {
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/someone");
        assert_eq!(
            expand_home("~/Documents/a.md"),
            "/Users/someone/Documents/a.md"
        );
        assert_eq!(expand_home("/absolute/path"), "/absolute/path");
        assert_eq!(expand_home("relative/path"), "relative/path");
        if let Some(v) = saved {
            std::env::set_var("HOME", v);
        }
    }

    #[test]
    fn truncate_respects_char_boundary() {
        // A string long enough to trigger truncation with multibyte chars.
        let s = "café ".repeat(1000);
        let out = truncate_at_char_boundary(&s, 500);
        assert!(out.len() >= 500);
        assert!(out.contains("truncated"));
        // Should still be valid UTF-8 — the char_boundary check matters.
        assert!(out.chars().next().is_some());
    }
}
