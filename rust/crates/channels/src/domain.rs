//! v1 copy of flaco-core::domain — intentionally duplicated so the
//! v1 `channels` crate does NOT grow a dependency on `flaco-core`.
//! The "v1 untouched" safety rail matters more than the ~200 lines
//! of duplication, because v1 is what's currently connected to
//! Slack and powering production while v2 is being staged.
//!
//! The module mirrors flaco-core's domain module in behavior:
//!
//!   1. `classify_message(text)` picks one of 6 Domain variants by
//!      keyword matching (zero-cost, 0ms, no LLM).
//!   2. `preflight(domain)` checks required env vars and returns an
//!      `Err(String)` if any are missing so the caller can short-
//!      circuit before burning an LLM call.
//!   3. `build_context(domain)` assembles the domain-specific system
//!      prompt stanza + auto-reads any ground-truth files into a
//!      single string ready to prepend to the LLM prompt.
//!
//! The keyword tables and stanzas are kept in lockstep with
//! `flaco-core::domain` — if you add a phrasing there, add it here
//! too. This is deliberate symmetry, not a refactor target.

use std::fmt;

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
}

#[derive(Debug)]
pub struct DomainSpec {
    pub name: &'static str,
    pub env_keys: &'static [&'static str],
    pub system_prompt_stanza: &'static str,
    pub ground_truth_files: &'static [&'static str],
}

const UNIFI: DomainSpec = DomainSpec {
    name: "unifi",
    env_keys: &["UNIFI_API_KEY"],
    system_prompt_stanza: r#"
## UniFi / home network

The user's home network is managed by UniFi. When they ask about
"my network", "my wifi", "my UniFi", "my router", "my UDM", or
anything infrastructure-shaped, use the UniFi CLOUD API at
api.ui.com — NOT a local controller and NOT an interactive admin
password flow. Never ask the user for credentials.

AUTH: every request needs header `X-API-KEY: $UNIFI_API_KEY`.
The key is ALREADY in your env as `UNIFI_API_KEY`.

CORE ENDPOINTS:
- curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/sites
- curl -s -H "X-API-KEY: $UNIFI_API_KEY" https://api.ui.com/ea/hosts

The site stats object has counts for: criticalNotification,
offlineDevice, offlineGatewayDevice, offlineWifiDevice,
pendingUpdateDevice, totalDevice, wifiClient, wiredClient.
Any non-zero on "offline*" or "*Notification" is a real finding.

GROUND TRUTH: `~/Documents/pi-projects/infra/network-state.yaml`
is the user's declared "known-good" network state. The runtime has
already read it — see the "Source of truth" section below — before
reporting drift.

WHAT "MY X" MEANS:
- "my network" / "my wifi" / "my unifi" → the Default site
- "my gateway" / "my UDM"               → UDM-SE-Rouraio host
- "my NAS" / "my unas"                  → UNAS-Pro-8 host
"#,
    ground_truth_files: &["~/Documents/pi-projects/infra/network-state.yaml"],
};

const JIRA: DomainSpec = DomainSpec {
    name: "jira",
    env_keys: &["JIRA_API_TOKEN", "JIRA_EMAIL", "JIRA_URL"],
    system_prompt_stanza: r#"
## Jira / dev tickets

When the user asks about "my tickets", "my sprint", "my backlog",
"my plate", or "standup", use the Jira REST API at `$JIRA_URL`.

AUTH: Basic base64 of `$JIRA_EMAIL:$JIRA_API_TOKEN`. Already in env.

Always use `assignee = currentUser()` — never ask the user's
username or accountId. The API resolves currentUser from auth.

ENDPOINT:
  curl -s -u "$JIRA_EMAIL:$JIRA_API_TOKEN" \
    -H "Accept: application/json" \
    "$JIRA_URL/rest/api/3/search?jql=<encoded-jql>&fields=summary,status,priority,duedate"
"#,
    ground_truth_files: &[],
};

const GITHUB: DomainSpec = DomainSpec {
    name: "github",
    env_keys: &["GITHUB_TOKEN"],
    system_prompt_stanza: r#"
## GitHub

When the user asks about "my PRs", "my repos", "my branches",
"my CI", or "my actions", use the GitHub REST API.

AUTH: `Authorization: Bearer $GITHUB_TOKEN` header. Already in env.

KEY ENDPOINTS:
- My open PRs:  curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/search/issues?q=is:pr+is:open+author:@me"
- My reviews:   curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/search/issues?q=is:pr+is:open+review-requested:@me"
- My repos:     curl -s -H "Authorization: Bearer $GITHUB_TOKEN" \
    "https://api.github.com/user/repos?affiliation=owner&sort=updated&per_page=20"

Always use `@me` for "my" — never ask the username.
"#,
    ground_truth_files: &[],
};

const FIGMA: DomainSpec = DomainSpec {
    name: "figma",
    env_keys: &["FIGMA_ACCESS_TOKEN"],
    system_prompt_stanza: r#"
## Figma

When the user asks about "my design", "my figma file", or "my
mockup", use the Figma REST API.

AUTH: `X-Figma-Token: $FIGMA_ACCESS_TOKEN` header. Already in env.

KEY ENDPOINTS:
- curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" https://api.figma.com/v1/me
- curl -s -H "X-Figma-Token: $FIGMA_ACCESS_TOKEN" https://api.figma.com/v1/files/$FILE_KEY?depth=1

If the user gives a figma.com URL, parse out the file key (after
`/file/` or `/design/`) and hit the metadata endpoint.
"#,
    ground_truth_files: &[],
};

const HOMELAB: DomainSpec = DomainSpec {
    name: "homelab",
    env_keys: &[],
    system_prompt_stanza: r#"
## Homelab (Pi / Mac server / VPS / UNAS)

When the user asks about "my pi", "my mac server", "my vps", "my
homelab", or anything about the physical nodes, use SSH (aliases
are configured in ~/.ssh/config).

SSH ALIASES (via bash):
- `ssh pi`         → Raspberry Pi at pi.home / 10.0.1.4
- `ssh mac-server` → M1 Pro Mac at mac.home / 10.0.1.3
- `ssh vps`        → Hostinger VPS at 72.60.173.8

REACHABLE SERVICES (no SSH, just curl):
- http://pi.home:5678      → n8n
- http://pi.home:3002      → Grafana
- http://pi.home:3001      → Uptime Kuma
- http://pi.home:9090      → Prometheus
- http://pi.home:8123      → Home Assistant
- http://mac.home:3033     → flaco-v2 web UI
- http://mac.home:11434    → Ollama

`flaco doctor` on mac-server is the fastest health readback.
"#,
    ground_truth_files: &["~/Documents/pi-projects/infra/network-state.yaml"],
};

const UNAS_SAVE: DomainSpec = DomainSpec {
    name: "unas_save",
    env_keys: &[],
    system_prompt_stanza: r#"
## Saving artifacts to the UNAS

When the user asks you to "save this", "put this in my unas", "put this
in my folder", or wants a file written somewhere persistent, write it to
the UNAS shared drive under the user's own folder. Directory scheme:

  /Volumes/Roura.io/<user-folder>/flaco/<category>/<filename>

WHO IS "MY FOLDER":
Folder names follow the scheme <first-initial><lastname-lowercase>
with two hardcoded exceptions for the Chris/Carolay collision:
- Christopher Roura (cjroura@roura.io)  → cjroura   (exception)
- Carolay Roura                         → caroroura (exception)
- Walter Roura                          → wroura
- Susan Roura                           → sroura
- any other user                        → first initial + lowercased
                                          last name (e.g. Jane Smith
                                          → jsmith). Fallback is
                                          `shared` if no real name.

CATEGORIES (pick the best fit):
- shortcuts  — generated Siri Shortcut .shortcut files
- research   — cited research writeups, .md
- scaffolds  — project scaffolds, READMEs, CLAUDE.md
- notes      — personal notes, to-dos, reminders
- drafts     — draft emails, draft messages
- other      — fallback

PROCEDURE (using the bash tool):
  1. Verify the mount exists:
       `test -d /Volumes/Roura.io && echo ok || echo missing`
     If missing, tell the user the UNAS isn't mounted on mac-server
     and to run the mount helper — do NOT write to a local path
     as a fallback. That loses data. Fail visibly.
  2. Pick the destination folder:
       DEST=/Volumes/Roura.io/<user-folder>/flaco/<category>
  3. Create it:
       `mkdir -p "$DEST"`
  4. Write the content (use a heredoc so the shell quoting is safe):
       `cat > "$DEST/<filename>" <<'EOF' ... EOF`
  5. Return a Finder-friendly path AND a Files.app SMB URL, like:
       "Saved to /Volumes/Roura.io/<user>/flaco/<category>/<name>.md
        — SMB URL: smb://10.0.1.2/Roura.io/<user>/flaco/<category>/<name>.md"

NEVER use a local path on mac-server as a fallback when the UNAS is
down. Tell the user and stop. Data that lands on mac-server local disk
under the impression it's on the UNAS is silent data loss.
"#,
    ground_truth_files: &[],
};

const GENERAL: DomainSpec = DomainSpec {
    name: "general",
    env_keys: &[],
    system_prompt_stanza: "",
    ground_truth_files: &[],
};

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

pub fn classify_message(text: &str) -> Domain {
    let lower = text.to_ascii_lowercase();
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
/// primary intent. The caller can stack the `UnasSave` stanza on top
/// of whatever primary domain fired — e.g. "research X and save it
/// to my unas" gets both the research context AND the save recipe.
pub fn also_wants_save(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    any_contains(&lower, UNAS_SAVE_KEYWORDS)
}

fn any_contains(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

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

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
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
    }

    #[test]
    fn classify_github_basic() {
        assert_eq!(classify_message("what are my prs"), Domain::GitHub);
        assert_eq!(classify_message("how's github today"), Domain::GitHub);
    }

    #[test]
    fn classify_figma_basic() {
        assert_eq!(classify_message("pull my design"), Domain::Figma);
    }

    #[test]
    fn classify_homelab_basic() {
        assert_eq!(classify_message("ssh into my pi"), Domain::Homelab);
        assert_eq!(classify_message("is my mac server up"), Domain::Homelab);
    }

    #[test]
    fn classify_save_to_unas_standalone() {
        assert_eq!(classify_message("save this to my unas"), Domain::UnasSave);
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
        assert_eq!(
            classify_message("check my unifi and save it to my unas"),
            Domain::Unifi
        );
        assert!(also_wants_save("check my unifi and save it to my unas"));
    }

    #[test]
    fn classify_general_fallthrough() {
        assert_eq!(classify_message("hello"), Domain::General);
        assert_eq!(classify_message("write me a limerick"), Domain::General);
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(classify_message("CHECK MY UNIFI"), Domain::Unifi);
    }

    #[test]
    fn preflight_general_always_ok() {
        assert!(preflight(Domain::General).is_ok());
    }

    #[test]
    fn preflight_flags_missing_env() {
        let key = "UNIFI_API_KEY";
        let saved = std::env::var(key).ok();
        std::env::remove_var(key);

        let result = preflight(Domain::Unifi);
        assert!(result.is_err());
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
        std::env::set_var(key, "test-value");

        assert!(preflight(Domain::Unifi).is_ok());

        if let Some(v) = saved {
            std::env::set_var(key, v);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn unifi_stanza_mentions_cloud_api_not_admin_password() {
        // This is the critical anti-regression test — the whole reason
        // we're backporting is that v1 was asking for admin passwords.
        // The stanza must explicitly tell the model to use the cloud
        // API with X-API-KEY and NEVER ask for an admin password.
        let stanza = UNIFI.system_prompt_stanza;
        assert!(stanza.contains("UNIFI_API_KEY"));
        assert!(stanza.contains("api.ui.com"));
        assert!(stanza.to_lowercase().contains("never ask"));
    }

    #[test]
    fn build_context_general_is_empty() {
        assert!(build_context(Domain::General).is_empty());
    }

    #[test]
    fn build_context_unifi_has_stanza() {
        let ctx = build_context(Domain::Unifi);
        assert!(ctx.contains("UNIFI_API_KEY"));
        assert!(ctx.contains("api.ui.com"));
    }

    #[test]
    fn expand_home_replaces_tilde() {
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_home("~/a/b"), "/Users/test/a/b");
        assert_eq!(expand_home("/abs"), "/abs");
        if let Some(v) = saved {
            std::env::set_var("HOME", v);
        }
    }
}
