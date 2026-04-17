//! Natural-language intent router — the Jarvis layer.
//!
//! Every surface (Slack, TUI, Web, CLI) calls `intent::detect(text)` on
//! each incoming user utterance **before** dispatching to the LLM chat
//! loop. If a known intent matches, the surface calls `intent::dispatch`
//! with the matched intent plus the runtime/features/surface/user_id and
//! gets back the reply string directly — no LLM round-trip.
//!
//! Intents are deliberately narrow: only the handful of actions where
//!
//!   a) the LLM can't do it correctly from tools alone (e.g. `reset` —
//!      there is no "reset conversation" tool because resetting is a
//!      session-level concept, not a memory-level one), OR
//!
//!   b) the LLM *could* do it via tools but the round-trip is slow and
//!      unreliable (e.g. `morning brief` taking 60-120s because qwen3
//!      meanders through its recall/search/tool-loop when the user just
//!      wanted the brief).
//!
//! Everything else — research, remember, recall, scaffold, shortcut,
//! chit-chat — falls through to the normal chat path and is answered by
//! the LLM with its tool registry. The router is an *optimisation*, not
//! a replacement for the chat brain.
//!
//! # Reset — the motivating case
//!
//! Chris reported that typing `/clear` or `clear` in Slack did nothing.
//! Root cause: Slack's slash commands require a server-side manifest
//! registration at api.slack.com/apps, and without it, `/clear` is
//! eaten by Slack's autocomplete UI before it ever reaches the bot.
//! The intent router sidesteps this entirely: **any** phrasing that
//! means "reset this conversation" works without a manifest.

use std::sync::Arc;

use crate::error::Result;
use crate::features::Features;
use crate::persona::PersonaRegistry;
use crate::runtime::{Runtime, Surface};
use crate::session::Session;

/// A recognized intent. Ordered roughly by priority — earlier variants
/// are more specific and checked first by `detect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// "reset", "clear", "/clear", "start over", "new conversation", etc.
    Reset,
    /// "help", "commands", "what can you do".
    Help,
    /// "status", "how are you", "are you up".
    Status,
    /// "brief", "morning brief", "what's on my plate", "my day".
    Brief,
    /// "memories", "what do you remember", "show memories".
    ListMemories,
    /// "tools", "what tools do you have", "list tools".
    Tools,
}

/// Normalize a user utterance for pattern matching:
///   - strip leading/trailing whitespace
///   - strip any leading Slack user mention `<@USERID>` tokens
///   - strip a leading `/` so `/clear` and `clear` match the same patterns
///   - lowercase
///   - collapse internal whitespace to single spaces
///   - strip trailing `?`, `!`, `.`
fn normalize(text: &str) -> String {
    let mut s = text.trim().to_string();

    // Strip Slack user mentions like "<@U09JCPF9FJB> clear" → "clear".
    // Only removes the LEADING mention; anything after it is user content.
    while s.starts_with('<') {
        if let Some(end) = s.find('>') {
            s = s[end + 1..].trim().to_string();
        } else {
            break;
        }
    }

    // Lowercase + collapse whitespace.
    let lowered = s.to_ascii_lowercase();
    let collapsed: String = lowered
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Strip a single trailing punctuation mark so "clear?" == "clear".
    let trimmed = collapsed.trim_end_matches(['?', '!', '.', ',']).to_string();

    // Strip a leading slash so "/clear" == "clear".
    trimmed.trim_start_matches('/').trim().to_string()
}

/// Detect an intent from a raw user utterance. Returns `None` if nothing
/// matches — the caller should then fall through to normal chat.
pub fn detect(text: &str) -> Option<Intent> {
    let t = normalize(text);
    if t.is_empty() {
        return None;
    }

    // ---- Reset — widest match, highest priority ----
    const RESET_EXACT: &[&str] = &[
        "reset",
        "clear",
        "new",
        "forget",
        "wipe",
        "restart",
        "start over",
        "start fresh",
        "new chat",
        "new conversation",
        "new thread",
        "clear chat",
        "clear this",
        "clear this chat",
        "clear this conversation",
        "clear the chat",
        "clear the conversation",
        "reset chat",
        "reset this",
        "reset this chat",
        "reset this conversation",
        "reset the chat",
        "reset the conversation",
        "forget this",
        "forget that",
        "forget this conversation",
        "forget the conversation",
        "forget everything",
        "wipe this",
        "wipe the chat",
        "wipe the conversation",
        "hey flaco reset",
        "hey flaco clear",
        "flaco reset",
        "flaco clear",
    ];
    if RESET_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::Reset);
    }

    // ---- Help ----
    const HELP_EXACT: &[&str] = &[
        "help",
        "commands",
        "what can you do",
        "show commands",
        "list commands",
        "what commands do you have",
        "how do i use you",
        "how do i use this",
    ];
    if HELP_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::Help);
    }

    // ---- Status ----
    const STATUS_EXACT: &[&str] = &[
        "status",
        "health",
        "how are you",
        "you up",
        "are you up",
        "are you alive",
        "are you online",
        "ping",
    ];
    if STATUS_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::Status);
    }

    // ---- Brief / Morning ----
    const BRIEF_EXACT: &[&str] = &[
        "brief",
        "morning brief",
        "morning",
        "good morning",
        "my day",
        "my morning",
        "what is on my plate",
        "whats on my plate",
        "what's on my plate",
        "what is on my plate today",
        "whats on my plate today",
        "what's on my plate today",
        "what should i focus on",
        "what should i focus on today",
        "what do i have today",
        "what do i have going on",
        "give me my brief",
        "give me my morning brief",
        "give me the brief",
    ];
    if BRIEF_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::Brief);
    }

    // ---- ListMemories ----
    const MEMORIES_EXACT: &[&str] = &[
        "memories",
        "list memories",
        "show memories",
        "my memories",
        "show my memories",
        "what do you remember",
        "what do you remember about me",
        "what do you know about me",
    ];
    if MEMORIES_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::ListMemories);
    }

    // ---- Tools ----
    const TOOLS_EXACT: &[&str] = &[
        "tools",
        "list tools",
        "show tools",
        "what tools",
        "what tools do you have",
        "available tools",
    ];
    if TOOLS_EXACT.iter().any(|p| *p == t) {
        return Some(Intent::Tools);
    }

    None
}

/// Execute a matched intent and return the reply the surface should
/// send back to the user. Replies are plain-text-with-markdown: Slack
/// will render them as mrkdwn, web will run them through pulldown-cmark,
/// TUI displays them verbatim.
pub async fn dispatch(
    intent: Intent,
    runtime: &Arc<Runtime>,
    features: &Arc<Features>,
    surface: &Surface,
    user_id: &str,
) -> Result<String> {
    match intent {
        Intent::Reset => {
            // Start a brand-new conversation row in memory. The next call
            // to runtime.session() for this (surface, user) will pick up
            // the new row because it orders by updated_at DESC. We don't
            // delete anything — prior conversations remain recallable from
            // the sidebar / `/memories` path.
            let personas = PersonaRegistry::defaults();
            let _ = Session::start(
                runtime.memory.clone(),
                &personas,
                surface.as_str(),
                user_id,
                None,
            )?;
            Ok(
                "Conversation reset. Starting fresh — what's up?".to_string(),
            )
        }

        Intent::Help => Ok(HELP_TEXT.to_string()),

        Intent::Status => {
            let mem_count = runtime
                .memory
                .all_facts(user_id, 10_000)
                .map(|v| v.len())
                .unwrap_or(0);
            let tool_count = runtime.tools.names().len();
            let conv_count = runtime
                .memory
                .list_conversations(10_000)
                .map(|v| v.len())
                .unwrap_or(0);
            Ok(format!(
                "flacoAi online · model `{}` · {tool_count} tools · {mem_count} memories · {conv_count} conversations",
                runtime.ollama.model()
            ))
        }

        Intent::Brief => match features.morning_brief(user_id).await {
            Ok(b) => Ok(b.markdown),
            Err(e) => Ok(format!("Couldn't generate a brief: {e}")),
        },

        Intent::ListMemories => {
            let mems = runtime.memory.all_facts(user_id, 50).unwrap_or_default();
            if mems.is_empty() {
                Ok("(no memories yet — tell me something and I'll remember it)".to_string())
            } else {
                let lines: Vec<String> = mems
                    .iter()
                    .map(|m| format!("• `[{}]` {}", m.kind, m.content))
                    .collect();
                Ok(format!(
                    "**Memories ({}):**\n{}",
                    mems.len(),
                    lines.join("\n")
                ))
            }
        }

        Intent::Tools => {
            let names = runtime.tools.names();
            Ok(format!(
                "**{} tools available:**\n{}",
                names.len(),
                names
                    .iter()
                    .map(|n| format!("• `{n}`"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        }
    }
}

/// Help text shown for the `Help` intent. Kept as a single const so the
/// tests can assert stable copy and surfaces can show the same thing.
pub const HELP_TEXT: &str = "\
**flacoAi — powered by Roura.io**

You can talk to me normally — I'll use my tools to answer. Or use these natural shortcuts:

• **reset / clear / new** — wipe this conversation and start fresh
• **help / commands** — show this message
• **status / how are you** — model + memory + tool counts
• **brief / morning / what's on my plate** — your Morning Brief from Jira + memory
• **memories / what do you remember** — everything I know about you
• **tools / what tools** — list every tool I have

Slash-command versions (`/reset`, `/brief`, etc.) work identically. So does \
`@flaco reset`. Anything else gets answered by the chat brain with my full tool \
registry.";

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_mentions_and_punctuation() {
        assert_eq!(normalize("<@U09JCPF9FJB> clear"), "clear");
        assert_eq!(normalize("Clear!"), "clear");
        assert_eq!(normalize("  /reset  "), "reset");
        assert_eq!(normalize("RESET?"), "reset");
        assert_eq!(normalize("Clear   this   chat."), "clear this chat");
        assert_eq!(normalize("<@U123> <@U456> clear"), "clear");
    }

    #[test]
    fn detects_reset_in_every_phrasing() {
        let phrasings = [
            "/reset",
            "/clear",
            "/new",
            "/forget",
            "reset",
            "Clear",
            "CLEAR",
            "new",
            "forget",
            "wipe",
            "start over",
            "Start Fresh",
            "new conversation",
            "new chat",
            "new thread",
            "clear chat",
            "clear this",
            "clear this chat",
            "clear this conversation",
            "reset chat",
            "reset this",
            "reset this chat",
            "reset this conversation",
            "forget this",
            "forget everything",
            "forget that",
            "wipe this",
            "wipe the conversation",
            "<@U09JCPF9FJB> clear",
            "<@U09JCPF9FJB> reset",
            "hey flaco reset",
            "flaco clear",
            "clear!",
            "reset?",
            "Reset.",
        ];
        for p in phrasings {
            assert_eq!(detect(p), Some(Intent::Reset), "failed on phrasing: {p:?}");
        }
    }

    #[test]
    fn detects_help() {
        assert_eq!(detect("help"), Some(Intent::Help));
        assert_eq!(detect("/help"), Some(Intent::Help));
        assert_eq!(detect("what can you do?"), Some(Intent::Help));
        assert_eq!(detect("commands"), Some(Intent::Help));
    }

    #[test]
    fn detects_status() {
        assert_eq!(detect("status"), Some(Intent::Status));
        assert_eq!(detect("how are you?"), Some(Intent::Status));
        assert_eq!(detect("you up?"), Some(Intent::Status));
        assert_eq!(detect("ping"), Some(Intent::Status));
    }

    #[test]
    fn detects_brief() {
        assert_eq!(detect("brief"), Some(Intent::Brief));
        assert_eq!(detect("/brief"), Some(Intent::Brief));
        assert_eq!(detect("morning"), Some(Intent::Brief));
        assert_eq!(detect("morning brief"), Some(Intent::Brief));
        assert_eq!(detect("what's on my plate?"), Some(Intent::Brief));
        assert_eq!(detect("Whats On My Plate Today"), Some(Intent::Brief));
        assert_eq!(detect("what should I focus on today"), Some(Intent::Brief));
    }

    #[test]
    fn detects_memories() {
        assert_eq!(detect("memories"), Some(Intent::ListMemories));
        assert_eq!(detect("/memories"), Some(Intent::ListMemories));
        assert_eq!(detect("what do you remember"), Some(Intent::ListMemories));
        assert_eq!(detect("what do you know about me?"), Some(Intent::ListMemories));
    }

    #[test]
    fn detects_tools() {
        assert_eq!(detect("tools"), Some(Intent::Tools));
        assert_eq!(detect("/tools"), Some(Intent::Tools));
        assert_eq!(detect("what tools do you have?"), Some(Intent::Tools));
    }

    #[test]
    fn falls_through_on_unrelated_text() {
        assert_eq!(detect("what is the weather today"), None);
        assert_eq!(detect("write a SwiftUI view"), None);
        assert_eq!(detect("research quantum computing"), None);
        assert_eq!(detect("remember that I love Rust"), None);
        assert_eq!(detect(""), None);
        assert_eq!(detect("   "), None);
    }

    #[test]
    fn reset_does_not_overmatch_on_substrings() {
        // "reset" alone matches, but "reset the database" should NOT —
        // it's a content request, not a meta-command.
        assert_eq!(detect("reset the database"), None);
        assert_eq!(detect("clear the log file"), None);
        assert_eq!(detect("can you help me reset my password"), None);
    }

    #[test]
    fn help_text_mentions_reset_and_branding() {
        assert!(HELP_TEXT.contains("reset"));
        assert!(HELP_TEXT.contains("flacoAi"));
        assert!(HELP_TEXT.contains("Roura.io"));
        assert!(HELP_TEXT.contains("brief"));
    }
}
