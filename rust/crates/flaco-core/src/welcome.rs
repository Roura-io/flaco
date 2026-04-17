//! One-time welcome banners shown when a user first encounters flaco on a
//! given surface. Storage lives in `memory::user_state` — see
//! `Memory::has_seen_flag` / `mark_flag`. The flag namespace is versioned
//! (`welcome_v1`) so we can bump it to re-onboard users after a major
//! rework without touching the old rows.
//!
//! Usage:
//!   - In a surface adapter, after resolving the user id, call
//!     `welcome::maybe_show(memory, user_id, Surface::Slack)`.
//!   - If the returned `Option<String>` is `Some(msg)`, render it once,
//!     then never again for that (user, surface).
//!   - If it's `None`, proceed as normal.
//!
//! Both banners are deliberately short enough that Walter can read them
//! without scrolling and deliberately specific enough about what to try
//! that a new tech-friend grokking the repo can see the point in 30s.

use crate::memory::Memory;
use crate::runtime::Surface;

pub const FLAG: &str = "welcome_v1";

/// Slack flavor of the welcome banner. Uses Slack mrkdwn syntax
/// (`*bold*`, backticks, `>`). Kept under ~1500 chars so it fits in a
/// single Slack message block without threading.
pub const SLACK_WELCOME: &str = "\
:wave: *Hi, I'm flaco — your local AI assistant.*\n\
I run entirely on your Mac via Ollama. No cloud, no \
API keys leaving the house. Anything you say to me in Slack, the TUI, \
or the web UI is the same brain with the same memory.\n\
\n\
*Things I'm good at — try any of these:*\n\
• `/brief` — a real 3-section morning brief: focus, on your plate, heads up (pulls from Jira + memory)\n\
• `/research <topic>` — Perplexity-style answer with real citations, 100% local\n\
• `/shortcut <name> <english description>` — writes a real Siri Shortcut plist you can AirDrop to iPhone\n\
• `/scaffold <idea>` — turns \"I want to build X\" into a Jira epic + stories + git branch + starter code\n\
• just talk to me — I remember everything across every surface\n\
\n\
*Shortcuts that always work, no matter how you phrase them:*\n\
• `clear`, `reset`, `new chat`, `start over` → instant new conversation (zero LLM round-trip)\n\
• `what can you do?` → this help card any time\n\
• `how are you?` → status: model, tool count, memory size\n\
\n\
*Why it's useful:* I can look up live sports/news (I know today's date, I don't hallucinate last year's schedule), \
draft emails, fact-check stories before you act on them, scaffold projects end-to-end, and hold a coherent \
conversation across your phone, laptop, and terminal because there's one SQLite brain behind all of it.\n\
\n\
You'll only see this message once. Go ahead and try something.";

/// TUI flavor — same spirit but plain text, no Slack mrkdwn, trimmed to
/// fit in a typical 80×24 terminal without wrapping into a wall.
pub const TUI_WELCOME: &str = "\
──────────────────────────────────────────────────────────────\n\
  flacoAi  ·  local AI, one brain, three surfaces\n\
──────────────────────────────────────────────────────────────\n\
\n\
Welcome. I run on your Mac (Ollama + qwen3) and share memory with\n\
the web UI and Slack. Whatever you tell me here, I remember there.\n\
\n\
Try one of these right now:\n\
\n\
  /brief                 → morning brief (Jira + memory → 3 sections)\n\
  /research <topic>      → web search with real citations, local only\n\
  /shortcut <name> <en>  → write a real Siri .shortcut file\n\
  /scaffold <idea>       → Jira epic + git branch + starter code\n\
  /memories              → list what I remember about you\n\
  /status                → model, tools, counts\n\
\n\
Natural language works too:\n\
  \"what can you do?\"\n\
  \"clear this chat\"\n\
  \"what's on my plate today?\"\n\
  \"who do the yankees play today?\"\n\
\n\
Anything I can't answer from memory, I'll use a tool for. Every\n\
tool call is logged in the Tool Log. Ctrl-C exits.\n\
\n\
You'll only see this once. Go ahead.\n";

/// Check-and-set for the welcome flag. Returns `Some(banner)` the first
/// time a user touches a surface, and `None` on every subsequent call.
/// The write is atomic (`INSERT OR IGNORE`) so concurrent calls from the
/// same user don't double-fire.
pub fn maybe_show(memory: &Memory, user_id: &str, surface: Surface) -> Option<String> {
    let surface_str = surface.as_str();
    // mark_flag returns true iff the INSERT actually happened — i.e. this
    // is the first time. Using it as the gate (instead of has_seen_flag)
    // guarantees a race-free check-and-set.
    match memory.mark_flag(user_id, surface_str, FLAG, None) {
        Ok(true) => match surface {
            Surface::Slack => Some(SLACK_WELCOME.to_string()),
            Surface::Tui => Some(TUI_WELCOME.to_string()),
            // Web has its own rendered-HTML landing page and doesn't
            // need a chat-injected banner. CLI is one-shot and would be
            // noisy. Only Slack + TUI get the welcome right now.
            Surface::Web | Surface::Cli => None,
        },
        // Flag already set (second+ encounter) OR DB error — either way,
        // don't block the user. If the DB is sick the doctor command
        // will surface it.
        Ok(false) | Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_welcome_fires_exactly_once() {
        let m = Memory::open_in_memory().unwrap();
        let first = maybe_show(&m, "U123", Surface::Slack);
        let second = maybe_show(&m, "U123", Surface::Slack);
        let third = maybe_show(&m, "U123", Surface::Slack);
        assert!(first.is_some(), "first call should return the banner");
        assert!(second.is_none(), "second call should return None");
        assert!(third.is_none(), "third call should return None");
    }

    #[test]
    fn tui_welcome_fires_exactly_once() {
        let m = Memory::open_in_memory().unwrap();
        let first = maybe_show(&m, "chris", Surface::Tui);
        let second = maybe_show(&m, "chris", Surface::Tui);
        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn slack_and_tui_are_independent() {
        let m = Memory::open_in_memory().unwrap();
        // Seeing Slack first should NOT count as seeing TUI.
        let slack = maybe_show(&m, "U123", Surface::Slack);
        let tui = maybe_show(&m, "U123", Surface::Tui);
        assert!(slack.is_some());
        assert!(tui.is_some(), "TUI welcome should still fire after Slack");
    }

    #[test]
    fn web_and_cli_never_banner() {
        let m = Memory::open_in_memory().unwrap();
        assert!(maybe_show(&m, "chris", Surface::Web).is_none());
        assert!(maybe_show(&m, "chris", Surface::Cli).is_none());
    }

    #[test]
    fn different_users_each_get_their_own_banner() {
        let m = Memory::open_in_memory().unwrap();
        let chris = maybe_show(&m, "chris", Surface::Slack);
        let walter = maybe_show(&m, "U0AS9PLFLCD", Surface::Slack);
        assert!(chris.is_some());
        assert!(walter.is_some());
    }

    #[test]
    fn slack_welcome_fits_in_one_message() {
        // Slack blocks have a 3000-char text limit. The banner needs to
        // fit well under that to leave room for Slack's own chrome and
        // future additions.
        assert!(
            SLACK_WELCOME.len() < 2800,
            "slack welcome is {} chars, should be < 2800",
            SLACK_WELCOME.len()
        );
    }
}
