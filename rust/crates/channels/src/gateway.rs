//! Central message gateway — routes messages from channels through the AI
//! with per-sender conversation state and persona overlays.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agents::{self, Agent};
use crate::commands::Command;
use crate::rules::Rule;
use crate::skills::Skill;

/// Timeout for conversation state (2 hours).
const CONVERSATION_TIMEOUT: Duration = Duration::from_secs(7200);

// Signal tables for `is_code_content`. Module-scoped so clippy's
// `items_after_statements` stays quiet and so they're reusable if another
// caller ever wants the same classification.

/// Code-shape tokens with a leading space so we don't match inside words
/// like "function" or "inputs". Any hit flips `is_code_content` to true.
const CODE_TOKENS: &[&str] = &[
    " fn ",
    " def ",
    " func ",
    " class ",
    " struct ",
    " import ",
    " public class",
    " public func",
    " #include",
    " =>",
];

/// Explicit "<lang> code" phrasing. Any hit flips `is_code_content` to true.
const CODE_OF_LANG: &[&str] = &[
    "rust code",
    "python code",
    "swift code",
    "swiftui code",
    "javascript code",
    "typescript code",
    "golang code",
    "java code",
    "kotlin code",
    "c++ code",
    "c# code",
];

/// Request phrases — must co-occur with a `LANGUAGE_NAMES` match to flip
/// `is_code_content`. "write me" alone is not enough ("write me a poem");
/// "write me a swiftui view" is.
const REQUEST_PHRASES: &[&str] = &[
    "write me",
    "write a function",
    "write the function",
    "write some",
    "refactor this",
    "refactor the",
    "review this code",
    "implement a",
    "implement the",
    "debug this",
    "fix this function",
    "how do i write",
    "swiftui view",
];

/// Language names — must co-occur with a `REQUEST_PHRASES` match. Standalone
/// "rust" or "swift" could mean cars or Taylor.
const LANGUAGE_NAMES: &[&str] = &[
    "rust",
    "python",
    "swift",
    "swiftui",
    "javascript",
    "typescript",
    "golang",
    "kotlin",
    "c++",
    "c#",
];

/// Detects whether a user message is a code question, so [`Gateway::pick_model`]
/// can route to the coder tier.
///
/// Four strong signals, any one is enough to flip this true:
/// 1. A fenced code block (` ``` `)
/// 2. A whitespace-delimited code-shape token (`CODE_TOKENS`)
/// 3. An explicit "<language> code" phrase (`CODE_OF_LANG`)
/// 4. A request phrase (`REQUEST_PHRASES`) co-occurring with a language
///    name (`LANGUAGE_NAMES`)
///
/// We err toward true: a false positive means a non-code question gets routed
/// to the coder model, which still gives a reasonable answer. A false negative
/// means a real code question gets the general model and produces worse code.
#[must_use]
fn is_code_content(text: &str) -> bool {
    if text.contains("```") {
        return true;
    }

    let lower = text.to_ascii_lowercase();

    if CODE_TOKENS.iter().any(|t| lower.contains(t)) {
        return true;
    }

    if CODE_OF_LANG.iter().any(|t| lower.contains(t)) {
        return true;
    }

    let has_phrase = REQUEST_PHRASES.iter().any(|p| lower.contains(p));
    let has_lang = LANGUAGE_NAMES.iter().any(|l| lower.contains(l));
    has_phrase && has_lang
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// An incoming message from any channel.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub channel_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
}

/// A response to send back through a channel.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub content: String,
    pub thread_id: Option<String>,
}

/// Per-sender conversation state with expiry.
#[derive(Debug, Clone)]
pub struct ConversationState {
    pub sender_id: String,
    pub channel_id: String,
    pub sender_name: String,
    pub messages: Vec<ConversationMessage>,
    pub last_active: Instant,
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

impl ConversationState {
    fn new(sender_id: String, channel_id: String, sender_name: String) -> Self {
        Self {
            sender_id,
            channel_id,
            sender_name,
            messages: Vec::new(),
            last_active: Instant::now(),
        }
    }

    /// Whether the conversation has timed out.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.last_active.elapsed() > CONVERSATION_TIMEOUT
    }

    /// Touch the conversation to reset the timeout.
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    /// Add a user message.
    pub fn push_user(&mut self, content: String) {
        self.touch();
        self.messages.push(ConversationMessage {
            role: "user".into(),
            content,
        });
    }

    /// Add an assistant response.
    pub fn push_assistant(&mut self, content: String) {
        self.messages.push(ConversationMessage {
            role: "assistant".into(),
            content,
        });
    }
}

/// Channel-specific persona overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPersona {
    /// Channel identifier this persona applies to.
    pub channel: String,
    /// Tone/style description injected into the system prompt.
    pub prompt_overlay: String,
}

impl ChannelPersona {
    /// Built-in Slack persona — warm, conversational, human-like.
    #[must_use]
    pub fn slack() -> Self {
        Self {
            channel: "slack".into(),
            prompt_overlay: "\
You are responding in a Slack workspace. Adapt your communication style:
- Be conversational and warm, like a helpful teammate
- Use short paragraphs, not walls of text
- Use Slack mrkdwn formatting: *bold*, _italic_, `code`, ```code blocks```
- Use emoji sparingly but naturally (e.g. when celebrating a fix)
- If sharing code, keep snippets short and relevant
- Never mention that you're an AI or that you're using tools
- Respond as if you're a senior engineer on the team
- If asked about standups, code reviews, etc., run the appropriate skill and format the output conversationally"
                .into(),
        }
    }

    /// Mission-critical infra / homelab sentinel persona.
    /// Used for `#home-general`, `#infra-*`, `#network-*` channels.
    /// This persona is what gets vetted by the claude_check layer —
    /// it MUST NOT hallucinate infra state because the vet layer
    /// is the second, not the first, line of defense.
    #[must_use]
    pub fn infra() -> Self {
        Self {
            channel: "slack-infra".into(),
            prompt_overlay: "\
You are flacoAi running as the homelab sentinel for elGordo (cjroura@roura.io).
This is a solo homelab. There is NO TEAM, NO API GATEWAY, NO STATUS PAGE, NO
CUSTOMER. If you are about to use any of those words without them appearing in
the provided context, STOP — you are about to hallucinate.

The real infrastructure:
- Raspberry Pi 5 at 10.0.1.4 (Tailscale 100.70.234.35), user rouraio —
  AdGuard DNS primary, Prometheus, Uptime Kuma, Home Assistant, n8n, Grafana,
  Portainer. This Pi had a chronic undervoltage problem that was fixed
  2026-04-14 by replacing its power supply.
- Mac at 10.0.1.3, user roura.io.server — runs Ollama (you), secondary AdGuard
- UNAS NAS at 10.0.1.2 — storage, media, backups
- VPS srv1065212 at 72.60.173.8 (Tailscale 100.91.207.7) — runs deadman.sh
  every 60s as the external watchdog. Its alerts in this channel are AUTHORITATIVE.
- UDM-SE at 10.0.1.1 — UniFi gateway, Verizon Fios on WAN2

Rules:
1. For any 'is X up' / 'how is my network' / 'are we online' question:
   - First, read the 'Recent channel activity' section of this prompt. If
     there's a deadman alert in the last 15 minutes, use it as ground truth.
   - If you have a bash tool, run the relevant check yourself before replying
     (ssh rouraio@10.0.1.4 '<cmd>', curl the UniFi EA API with UNIFI_API_KEY,
     etc).
   - Never say 'the team fixed it' / 'deployed a fix' / 'all services green on
     the status page' / 'monitoring shows no anomalies' / 'fully back online'
     without evidence you can cite from the context or a tool you just ran.
2. Tone: terse, factual, staff-engineer. Not 'Hey there!'. Not 'Let me know
   if anything seems off!'. Lead with the fact, then the proof. 1-3 sentences
   unless the evidence requires more.
3. When you SSH, curl, or run a tool, cite the exact output in your reply.
4. If you don't have evidence, say so: 'I don't have a fresh read on that
   — let me check' then check. Never fabricate."
                .into(),
        }
    }

    /// Walter / #dad-help family persona.
    ///
    /// Walter is elGordo's dad. He has early-onset Alzheimer's, loves the
    /// Yankees and Premier League, and interacts with flacoAi through #dad-help
    /// and his iPad/Mac. He's NOT a developer. Responses must be factual,
    /// warm, plain-English, and NEVER use SaaS-bot phrasing.
    ///
    /// This persona is vetted by the claude_check layer (same as infra).
    /// Correctness matters because Walter can't easily distinguish a
    /// hallucinated Yankees schedule from a real one, and wrong medication
    /// reminders are dangerous.
    ///
    /// Specifically anti-patterns that this prompt forbids — each one was
    /// seen in a real bad reply in #dad-help before this persona existed:
    /// - "The Yankees will play their next game on April 13, 2026"
    ///   (that date is in the past; models quoted it from stale training
    ///   data without checking today's date)
    /// - "I'm sorry, but as an AI, I don't have real-time access to live
    ///   sports data" (lazy refusal; flacoAi HAS tools/endpoints for this)
    #[must_use]
    pub fn walter() -> Self {
        Self {
            channel: "slack-walter".into(),
            prompt_overlay: "\
You are flacoAi replying in a family channel (#dad-help) where the primary
reader is Walter, elGordo's dad. Walter has early-onset Alzheimer's, loves
the Yankees and Premier League, and is not technical.

Rules — every one of these maps to a real bad reply you are REPLACING:
1. NEVER say 'as an AI' or 'I don't have real-time access'. If you don't
   know something, say so in plain language: 'I don't know off the top of
   my head — let me check.' Then try.
2. NEVER quote a date as a 'future' event if it's actually in the past.
   The 'Recent channel activity' section of this prompt contains the
   current date; use it. If the date you want to quote is before today,
   you are hallucinating — say 'I'll need to check the current schedule'
   instead.
3. For Yankees / MLB / Premier League questions: if you have a bash tool
   or fs_read, call the real source (MLB StatsAPI, Fantasy Premier League
   API, or the family-api endpoint on the Pi). Do NOT guess from training
   data. Sports schedules change weekly; training data is always stale.
4. NEVER use SaaS-support phrasing: 'I'm sorry, but', 'I hope this helps',
   'Let me know if you have any other questions', 'I'm here to help', 'As
   an AI language model'. Walter doesn't want a chatbot voice, he wants a
   warm, direct family voice.
5. Tone: warm, conversational, plain English. Short sentences. Lead with
   the answer. If there's no clean answer, say so and offer to find out.
6. No markdown bullet dumps for casual questions. A one- or two-sentence
   plain-English reply is almost always right.

Examples of REPLACED bad replies (do NOT produce responses like these):

  ❌ 'The Yankees will play their next game on April 13, 2026.'
  ✓  'I'll need to check tonight's Yankees game — one sec.' (then check)

  ❌ 'I'm sorry, but as an AI, I don't have real-time access to live sports
     data. My current knowledge is limited to 2026 and doesn't include
     up-to-the-minute information about today's games or players.'
  ✓  'Good question — I don't know off the top of my head who's pitching.
     Let me pull it up.' (then pull it up)"
                .into(),
        }
    }

    /// Dispatch a persona based on Slack channel name. Used to pick the
    /// infra persona for mission-critical channels, the Walter persona for
    /// family channels, and the default Slack persona for everything else.
    /// Channel name is looked up once per channel ID via conversations.info
    /// and cached for the process lifetime by the caller.
    #[must_use]
    pub fn for_channel(channel_name: &str) -> Self {
        let n = channel_name.to_ascii_lowercase();
        if n == "dad-help" || n.starts_with("dad-") || n.starts_with("walter") {
            Self::walter()
        } else if n == "home-general"
            || n.starts_with("home-")
            || n.starts_with("infra-")
            || n.starts_with("network-")
        {
            Self::infra()
        } else {
            Self::slack()
        }
    }

    /// Whether this persona should have its responses vetted by the
    /// claude_check layer. Vetted channels: infra (mission-critical state)
    /// and walter (family-critical accuracy — wrong sports/meds content
    /// directly hits Walter). Default Slack conversations don't vet —
    /// latency matters more than hallucination prevention for chat and
    /// dev scratch.
    #[must_use]
    pub fn needs_vetting(&self) -> bool {
        self.channel == "slack-infra" || self.channel == "slack-walter"
    }
}

/// Gateway configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Ollama model to use for channel conversations (MEDIUM tier / default).
    pub model: Option<String>,
    /// SMALL-tier model name (used by orchestrator for short non-critical
    /// messages). Cached at startup from FLACO_MODEL_SMALL env var so
    /// pick_model() doesn't have to call std::env on every turn.
    #[serde(default)]
    pub model_small: Option<String>,
    /// LARGE-tier model name (used by orchestrator for mission-critical
    /// channels). Cached at startup from FLACO_MODEL_LARGE.
    #[serde(default)]
    pub model_large: Option<String>,
    /// CODER-tier model name (used when a turn looks like a code question:
    /// fenced code block, code-shape tokens, or a "write me a <lang>..."
    /// phrase). Routed through [`Gateway::pick_model`] ahead of the
    /// small/large/medium tiers. Cached at startup from FLACO_MODEL_CODER.
    #[serde(default)]
    pub model_coder: Option<String>,
    /// Ollama base URL.
    pub ollama_url: Option<String>,
    /// Our bot's Slack bot_id (the B-prefixed ID from auth.test). Loaded
    /// once at startup by the server binary so we don't have to hardcode
    /// it in source. Used to filter out our own bot's events to prevent
    /// feedback loops.
    #[serde(default)]
    pub our_bot_id: Option<String>,
    /// Per-channel personas.
    #[serde(default)]
    pub personas: Vec<ChannelPersona>,
    /// Registry of declarative agents loaded at startup from the on-disk
    /// agents directory (`~/.flaco/agents/` with a fallback to the in-repo
    /// `agents/` folder). Keyed by agent `name`. Not round-tripped through
    /// serde because [`Agent::prompt`] is marked `#[serde(skip)]` and the
    /// registry is always rebuilt from disk on each startup — serializing
    /// it would lose the prompt bodies and has no callers. `skip` keeps
    /// GatewayConfig's `Serialize`/`Deserialize` derives working without
    /// needing Agent to implement them in a round-trip-able way.
    #[serde(default, skip)]
    pub agents: HashMap<String, Agent>,
    #[serde(default)]
    pub skills: HashMap<String, Skill>,
    #[serde(default)]
    pub commands: HashMap<String, Command>,
    #[serde(default)]
    pub rules: HashMap<String, Rule>,
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

/// Central message dispatcher that routes channel messages through the AI.
pub struct Gateway {
    config: GatewayConfig,
    conversations: Arc<RwLock<HashMap<String, ConversationState>>>,
}

impl Gateway {
    /// Create a new gateway with the given configuration.
    #[must_use]
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            conversations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a conversation state for a sender.
    pub async fn get_or_create_conversation(
        &self,
        channel_id: &str,
        sender_id: &str,
        sender_name: &str,
    ) -> ConversationState {
        let key = format!("{channel_id}:{sender_id}");
        let mut conversations = self.conversations.write().await;

        // Prune expired conversations
        conversations.retain(|_, state| !state.is_expired());

        conversations
            .entry(key)
            .or_insert_with(|| {
                ConversationState::new(
                    sender_id.to_string(),
                    channel_id.to_string(),
                    sender_name.to_string(),
                )
            })
            .clone()
    }

    /// Update the stored conversation state after a turn.
    pub async fn update_conversation(
        &self,
        channel_id: &str,
        sender_id: &str,
        state: ConversationState,
    ) {
        let key = format!("{channel_id}:{sender_id}");
        let mut conversations = self.conversations.write().await;
        conversations.insert(key, state);
    }

    /// Get the persona overlay for a channel.
    #[must_use]
    pub fn persona_for(&self, channel: &str) -> Option<&ChannelPersona> {
        self.config.personas.iter().find(|p| p.channel == channel)
    }

    /// Borrow the full agent registry, keyed by agent `name`. Loaded once
    /// at startup from the on-disk agents directory.
    #[must_use]
    pub fn agents(&self) -> &HashMap<String, Agent> {
        &self.config.agents
    }

    /// Look up the agent a slash command should dispatch to. Delegates to
    /// [`agents::agent_for_slash_command`]. Returns `None` if no loaded
    /// agent declares a matching `SlashCommand` trigger. Matching is
    /// case-insensitive and the leading `/` is optional.
    #[must_use]
    pub fn agent_for_slash_command(&self, command: &str) -> Option<&Agent> {
        agents::agent_for_slash_command(&self.config.agents, command)
    }

    /// Look up the agent a channel name should auto-route to, if any.
    /// Delegates to [`agents::agent_for_channel`]. Returns the first agent
    /// whose `ChannelPattern` is an exact or prefix match (case-insensitive).
    #[must_use]
    pub fn agent_for_channel_name(&self, channel_name: &str) -> Option<&Agent> {
        agents::agent_for_channel(&self.config.agents, channel_name)
    }

    /// Look up an agent by a mention pattern in free-text user input.
    /// Delegates to [`agents::agent_for_mention`]. Returns the first agent
    /// whose `MentionPattern` substring appears in the lowercased input.
    #[must_use]
    pub fn agent_for_mention(&self, user_text: &str) -> Option<&Agent> {
        agents::agent_for_mention(&self.config.agents, user_text)
    }

    pub fn skills(&self) -> &HashMap<String, Skill> {
        &self.config.skills
    }

    pub fn skills_for_message(&self, text: &str) -> Vec<&Skill> {
        crate::skills::skills_for_message(&self.config.skills, text)
    }

    pub fn commands(&self) -> &HashMap<String, Command> {
        &self.config.commands
    }

    pub fn command_for_name(&self, name: &str) -> Option<&Command> {
        crate::commands::command_for_name(&self.config.commands, name)
    }

    pub fn rules(&self) -> &HashMap<String, Rule> {
        &self.config.rules
    }

    pub fn rules_for_language(&self, language: &str) -> Vec<&Rule> {
        crate::rules::rules_for_language(&self.config.rules, language)
    }

    /// Get the configured medium-tier model (default for unrouted turns).
    /// Reads from `GatewayConfig.model` populated from `FLACO_MODEL` env var.
    #[must_use]
    pub fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("qwen3:32b")
    }

    /// The configured small-tier model, or `None` if unset.
    #[must_use]
    pub fn model_small(&self) -> Option<&str> {
        self.config.model_small.as_deref().filter(|s| !s.is_empty())
    }

    /// The configured large-tier model, or `None` if unset.
    #[must_use]
    pub fn model_large(&self) -> Option<&str> {
        self.config.model_large.as_deref().filter(|s| !s.is_empty())
    }

    /// The configured coder-tier model, or `None` if unset.
    #[must_use]
    pub fn model_coder(&self) -> Option<&str> {
        self.config.model_coder.as_deref().filter(|s| !s.is_empty())
    }

    /// Our bot's Slack `bot_id`. Loaded from `auth.test` at startup by the
    /// server binary. Used to filter out our own bot's events to prevent
    /// feedback loops. Returns an empty string if not set (which would
    /// disable the self-filter — fail-closed behavior would be better but
    /// requires plumbing an error through the event loop).
    #[must_use]
    pub fn our_bot_id(&self) -> &str {
        self.config.our_bot_id.as_deref().unwrap_or("")
    }

    /// Pick the best Ollama model for a given turn — the **orchestrator**.
    ///
    /// The "20% Claude as long as we have capacity" target is achieved by
    /// always running the vet layer (claude_check) on top of the local
    /// model output for mission-critical channels — see ChannelPersona's
    /// `needs_vetting()`. This function picks the LOCAL model only.
    ///
    /// Routing rules (checked in this order):
    /// 1. **Code-ish content** (fenced block, code tokens, "write me a
    ///    <lang>...") → `FLACO_MODEL_CODER` if set. Wins over everything
    ///    else because code quality matters.
    /// 2. **Walter channel** (`slack-walter`) → `FLACO_MODEL_SMALL`
    ///    regardless of message length. Walter reads on an iPad and a
    ///    55-second "thinking..." placeholder is a bad UX; the vet layer
    ///    catches hallucinations from the smaller model. Latency matters
    ///    more than model size when a vet net exists.
    /// 3. **Infra channels** (mission-critical, `needs_vetting()=true`
    ///    but NOT walter) → `FLACO_MODEL_LARGE` for quality, then vet-
    ///    layer on top. Falls back to medium if unset.
    /// 4. **Short messages** (< 80 chars) in non-critical channels →
    ///    `FLACO_MODEL_SMALL` for ~1-2s replies. Falls back to medium if unset.
    /// 5. **Everything else** → medium (`FLACO_MODEL`).
    ///
    /// Env vars (read once at startup by the server binary, then cached
    /// in GatewayConfig — pick_model never calls std::env directly):
    /// ```text
    /// FLACO_MODEL=gpt-oss:20b                           # medium (default)
    /// FLACO_MODEL_SMALL=nemotron-mini                   # fast chit-chat + walter
    /// FLACO_MODEL_LARGE=qwen3:32b                       # mission-critical infra
    /// FLACO_MODEL_CODER=qwen2.5-coder:32b-instruct-q8_0 # code questions
    /// ```
    #[must_use]
    pub fn pick_model(&self, persona: &ChannelPersona, user_text: &str) -> String {
        let medium = self.model().to_string();
        let small = self.config.model_small.as_deref().filter(|s| !s.is_empty());
        let large = self.config.model_large.as_deref().filter(|s| !s.is_empty());
        let coder = self.config.model_coder.as_deref().filter(|s| !s.is_empty());

        // Coder tier wins when the user is clearly asking about or pasting
        // code. Vet layer still fires on top if the channel persona is
        // mission-critical (vetting is independent of which local model ran).
        if is_code_content(user_text) {
            if let Some(c) = coder {
                return c.to_string();
            }
        }

        // Walter channel: small/fast tier. The vet layer catches the
        // smaller model's hallucinations; Walter's iPad gets a response
        // in 3-5s instead of the 45-60s a large thinking-model takes.
        // Note this comes BEFORE the infra `needs_vetting()` branch so
        // walter doesn't fall into the large tier on latency-sensitive
        // questions.
        if persona.channel == "slack-walter" {
            if let Some(s) = small {
                return s.to_string();
            }
            return medium;
        }

        // Mission-critical infra channels: large model for quality.
        // Vet layer fires on top regardless of model size.
        if persona.needs_vetting() {
            return large.map_or(medium, String::from);
        }

        // Non-critical channels with short messages: use the fast small model
        // so chit-chat and quick lookups feel instant. Vet layer is OFF here
        // (latency matters more than correctness for non-mission channels).
        if user_text.chars().count() < 80 {
            if let Some(s) = small {
                return s.to_string();
            }
        }

        medium
    }

    /// Get the configured Ollama URL.
    #[must_use]
    pub fn ollama_url(&self) -> &str {
        self.config
            .ollama_url
            .as_deref()
            .unwrap_or("http://localhost:11434/v1")
    }

    /// Reset a sender's conversation.
    pub async fn reset_conversation(&self, channel_id: &str, sender_id: &str) {
        let key = format!("{channel_id}:{sender_id}");
        let mut conversations = self.conversations.write().await;
        conversations.remove(&key);
    }

    /// Get conversation count.
    pub async fn active_conversations(&self) -> usize {
        let conversations = self.conversations.read().await;
        conversations.values().filter(|s| !s.is_expired()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creates_and_retrieves_conversation() {
        let gw = Gateway::new(GatewayConfig::default());
        let state = gw
            .get_or_create_conversation("slack", "U123", "Chris")
            .await;
        assert_eq!(state.sender_id, "U123");
        assert_eq!(state.sender_name, "Chris");
        assert!(state.messages.is_empty());
    }

    #[tokio::test]
    async fn conversation_state_tracks_messages() {
        let gw = Gateway::new(GatewayConfig::default());
        let mut state = gw
            .get_or_create_conversation("slack", "U123", "Chris")
            .await;
        state.push_user("hello".into());
        state.push_assistant("hi there!".into());
        gw.update_conversation("slack", "U123", state).await;

        let loaded = gw
            .get_or_create_conversation("slack", "U123", "Chris")
            .await;
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn reset_clears_conversation() {
        let gw = Gateway::new(GatewayConfig::default());
        let mut state = gw
            .get_or_create_conversation("slack", "U123", "Chris")
            .await;
        state.push_user("hello".into());
        gw.update_conversation("slack", "U123", state).await;
        gw.reset_conversation("slack", "U123").await;

        let fresh = gw
            .get_or_create_conversation("slack", "U123", "Chris")
            .await;
        assert!(fresh.messages.is_empty());
    }

    #[test]
    fn slack_persona_has_overlay() {
        let persona = ChannelPersona::slack();
        assert_eq!(persona.channel, "slack");
        assert!(persona.prompt_overlay.contains("Slack"));
        assert!(persona.prompt_overlay.contains("conversational"));
    }

    #[test]
    fn walter_persona_exists_and_has_anti_patterns() {
        let p = ChannelPersona::walter();
        assert_eq!(p.channel, "slack-walter");
        // Must explicitly ban the two real bad replies seen in #dad-help.
        assert!(p.prompt_overlay.contains("as an AI"));
        assert!(p.prompt_overlay.contains("real-time"));
        // Must mention the Yankees/FPL context so the model knows it has sources.
        assert!(p.prompt_overlay.to_lowercase().contains("yankees"));
        // Must tell the model to say "let me check" instead of refusing.
        assert!(p.prompt_overlay.to_lowercase().contains("let me check"));
    }

    #[test]
    fn for_channel_routes_dad_help_to_walter() {
        assert_eq!(
            ChannelPersona::for_channel("dad-help").channel,
            "slack-walter"
        );
        assert_eq!(
            ChannelPersona::for_channel("DAD-HELP").channel,
            "slack-walter"
        );
        assert_eq!(
            ChannelPersona::for_channel("dad-meds").channel,
            "slack-walter"
        );
    }

    #[test]
    fn for_channel_still_routes_infra_channels_to_infra() {
        // Regression guard — adding walter routing shouldn't break
        // the existing infra routing.
        assert_eq!(
            ChannelPersona::for_channel("home-general").channel,
            "slack-infra"
        );
        assert_eq!(
            ChannelPersona::for_channel("infra-status").channel,
            "slack-infra"
        );
        assert_eq!(
            ChannelPersona::for_channel("network-alerts").channel,
            "slack-infra"
        );
    }

    #[test]
    fn for_channel_other_channels_still_default_to_slack() {
        assert_eq!(
            ChannelPersona::for_channel("flaco-general").channel,
            "slack"
        );
        assert_eq!(ChannelPersona::for_channel("general").channel, "slack");
    }

    #[test]
    fn needs_vetting_includes_walter_and_infra() {
        assert!(ChannelPersona::walter().needs_vetting());
        assert!(ChannelPersona::infra().needs_vetting());
        assert!(!ChannelPersona::slack().needs_vetting());
    }

    // ------------------------------------------------------------------
    // is_code_content — coder tier detection
    // ------------------------------------------------------------------

    #[test]
    fn is_code_content_detects_fenced_block() {
        assert!(is_code_content("here is the code:\n```rust\nfn main() {}\n```"));
        assert!(is_code_content("```\nsome code\n```"));
    }

    #[test]
    fn is_code_content_detects_code_tokens() {
        assert!(is_code_content("can you explain this fn main() stuff"));
        assert!(is_code_content("why does def __init__ need self"));
        assert!(is_code_content("what's wrong with my func foo()"));
        assert!(is_code_content("the public class Handler is missing a method"));
    }

    #[test]
    fn is_code_content_detects_lang_code_phrasing() {
        assert!(is_code_content("show me some rust code for parsing JSON"));
        assert!(is_code_content("I need swiftui code for a list view"));
        assert!(is_code_content("write python code that reads a CSV"));
    }

    #[test]
    fn is_code_content_detects_request_plus_language() {
        assert!(is_code_content("write me a swiftui view that shows a list"));
        assert!(is_code_content("refactor this rust function"));
        assert!(is_code_content("debug this python script"));
        assert!(is_code_content("implement a kotlin class that wraps a map"));
    }

    #[test]
    fn is_code_content_ignores_casual_language_mentions() {
        // Just saying "rust" or "swift" without a code context should NOT
        // trigger the coder tier — avoids routing "my car has rust" or
        // "taylor swift" to the coder model.
        assert!(!is_code_content("my old car has a lot of rust"));
        assert!(!is_code_content("did you listen to the new swift album"));
        assert!(!is_code_content("what's the weather in python texas"));
        assert!(!is_code_content("hello how are you"));
    }

    #[test]
    fn is_code_content_false_for_short_greeting() {
        assert!(!is_code_content("hey"));
        assert!(!is_code_content("yo"));
        assert!(!is_code_content("good morning"));
    }

    // ------------------------------------------------------------------
    // pick_model — orchestrator routing
    // ------------------------------------------------------------------

    fn test_gateway(
        medium: &str,
        small: Option<&str>,
        large: Option<&str>,
        coder: Option<&str>,
    ) -> Gateway {
        Gateway::new(GatewayConfig {
            model: Some(medium.to_string()),
            model_small: small.map(String::from),
            model_large: large.map(String::from),
            model_coder: coder.map(String::from),
            ollama_url: None,
            our_bot_id: None,
            personas: vec![],
            agents: HashMap::new(),
            skills: HashMap::new(),
            commands: HashMap::new(),
            rules: HashMap::new(),
        })
    }

    #[test]
    fn pick_model_routes_code_to_coder_tier_in_casual_channel() {
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let slack = ChannelPersona::slack();
        let picked = gw.pick_model(&slack, "write me a swiftui view for a counter");
        assert_eq!(picked, "qwen2.5-coder:32b-instruct-q8_0");
    }

    #[test]
    fn pick_model_routes_code_to_coder_tier_even_in_infra_channel() {
        // Code questions in infra channels: coder model wins, but vet
        // layer still fires via the persona (tested elsewhere).
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let infra = ChannelPersona::infra();
        let picked = gw.pick_model(&infra, "refactor this rust function:\n```rust\nfn x() {}\n```");
        assert_eq!(picked, "qwen2.5-coder:32b-instruct-q8_0");
    }

    #[test]
    fn pick_model_falls_through_when_coder_tier_unset() {
        // Coder not configured → code question falls through to the
        // existing small/large/medium logic.
        let gw = test_gateway("gpt-oss:20b", Some("nemotron-mini"), Some("gpt-oss:20b"), None);
        let slack = ChannelPersona::slack();
        // Long code question → medium
        let picked = gw.pick_model(
            &slack,
            "write me a swiftui view that displays a list of items from an API endpoint, \
             handles loading and error states, and uses proper SwiftUI state management patterns",
        );
        assert_eq!(picked, "gpt-oss:20b");
    }

    #[test]
    fn pick_model_infra_channel_uses_large_for_non_code() {
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let infra = ChannelPersona::infra();
        let picked = gw.pick_model(&infra, "is the pi healthy right now");
        assert_eq!(picked, "gpt-oss:20b"); // large tier
    }

    #[test]
    fn pick_model_short_casual_uses_small() {
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let slack = ChannelPersona::slack();
        let picked = gw.pick_model(&slack, "hey what's up");
        assert_eq!(picked, "nemotron-mini");
    }

    #[test]
    fn pick_model_long_casual_uses_medium() {
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let slack = ChannelPersona::slack();
        let text = "here is a long message about what I'm planning for the weekend \
                    which has nothing to do with code or infrastructure at all, just \
                    vacation plans and errands and such, well past 80 characters";
        assert_eq!(gw.pick_model(&slack, text), "gpt-oss:20b");
    }

    #[test]
    fn pick_model_walter_channel_uses_small_tier_for_latency() {
        // Walter reads on an iPad; 55-second "thinking..." placeholder
        // from a large thinking-model is bad UX. Walter is vetted (the
        // vet layer catches hallucinations from the smaller model), so
        // we trade a little model size for a lot of responsiveness.
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("qwen3:32b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let walter = ChannelPersona::walter();
        // Short message
        assert_eq!(
            gw.pick_model(&walter, "when are the yankees playing tonight"),
            "nemotron-mini"
        );
        // Long message — still small tier for walter, not medium
        let long_walter = "hi dad, wondering if the yankees game is on tonight and \
                           also want to know what time the pregame show starts on yes \
                           network and whether it overlaps with dinner";
        assert_eq!(gw.pick_model(&walter, long_walter), "nemotron-mini");
    }

    #[test]
    fn pick_model_walter_coder_question_still_routes_to_coder() {
        // If Walter somehow asks a code question, coder tier wins.
        // (Walter isn't a developer, but the routing should still be
        // consistent — coder wins over everything else including walter.)
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("qwen3:32b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let walter = ChannelPersona::walter();
        assert_eq!(
            gw.pick_model(&walter, "write me a swiftui view for the yankees score"),
            "qwen2.5-coder:32b-instruct-q8_0"
        );
    }

    #[test]
    fn pick_model_walter_falls_back_to_medium_when_small_unset() {
        let gw = test_gateway("gpt-oss:20b", None, Some("qwen3:32b"), None);
        let walter = ChannelPersona::walter();
        assert_eq!(gw.pick_model(&walter, "hi dad"), "gpt-oss:20b");
    }

    #[test]
    fn pick_model_coder_tier_beats_small_tier_on_short_code_question() {
        // Even a short message, if it's a code question, should route to
        // the coder tier instead of being shunted into the fast small tier.
        let gw = test_gateway(
            "gpt-oss:20b",
            Some("nemotron-mini"),
            Some("gpt-oss:20b"),
            Some("qwen2.5-coder:32b-instruct-q8_0"),
        );
        let slack = ChannelPersona::slack();
        let picked = gw.pick_model(&slack, "rust code for fnv hash?");
        assert_eq!(picked, "qwen2.5-coder:32b-instruct-q8_0");
    }

    // ------------------------------------------------------------------
    // Agent registry accessors — chunk 3 of the agent-loader arc
    // ------------------------------------------------------------------

    use crate::agents::{Agent, AgentTrigger, VetMode};

    /// Build a tiny synthetic agent registry for the accessor tests.
    /// Two agents — a rust reviewer with a slash command + mention trigger,
    /// and an infra sentinel with a channel-pattern trigger — cover the
    /// three lookup helpers in one registry.
    fn synthetic_agents() -> HashMap<String, Agent> {
        let mut m = HashMap::new();
        m.insert(
            "rust-reviewer".to_string(),
            Agent {
                name: "rust-reviewer".into(),
                description: "Reviews Rust code".into(),
                prompt: "You are a Rust reviewer.".into(),
                triggers: vec![
                    AgentTrigger::SlashCommand("/rust-review".into()),
                    AgentTrigger::MentionPattern("review this rust".into()),
                ],
                tools: vec!["read".into(), "grep".into()],
                vet: VetMode::Required,
                model: None,
            },
        );
        m.insert(
            "homelab-sentinel".to_string(),
            Agent {
                name: "homelab-sentinel".into(),
                description: "Answers homelab state questions".into(),
                prompt: "You are the homelab sentinel.".into(),
                triggers: vec![AgentTrigger::ChannelPattern("infra-".into())],
                tools: vec!["bash".into()],
                vet: VetMode::Required,
                model: Some("qwen3:32b-q8_0".into()),
            },
        );
        m
    }

    fn gateway_with_agents(agents: HashMap<String, Agent>) -> Gateway {
        Gateway::new(GatewayConfig {
            model: Some("gpt-oss:20b".into()),
            model_small: None,
            model_large: None,
            model_coder: None,
            ollama_url: None,
            our_bot_id: None,
            personas: vec![],
            agents,
            skills: HashMap::new(),
            commands: HashMap::new(),
            rules: HashMap::new(),
        })
    }

    #[test]
    fn gateway_agents_returns_full_registry() {
        let gw = gateway_with_agents(synthetic_agents());
        let registry = gw.agents();
        assert_eq!(registry.len(), 2);
        assert!(registry.contains_key("rust-reviewer"));
        assert!(registry.contains_key("homelab-sentinel"));
    }

    #[test]
    fn gateway_agents_empty_when_registry_unloaded() {
        let gw = Gateway::new(GatewayConfig::default());
        assert!(gw.agents().is_empty());
    }

    #[test]
    fn gateway_agent_for_slash_command_hits_registered_agent() {
        let gw = gateway_with_agents(synthetic_agents());
        let a = gw
            .agent_for_slash_command("/rust-review")
            .expect("rust-review agent should be registered");
        assert_eq!(a.name, "rust-reviewer");
        // Case-insensitive + slash-optional, inherited from the module helper.
        assert!(gw.agent_for_slash_command("rust-review").is_some());
        assert!(gw.agent_for_slash_command("/Rust-Review").is_some());
    }

    #[test]
    fn gateway_agent_for_slash_command_misses_unknown_command() {
        let gw = gateway_with_agents(synthetic_agents());
        assert!(gw.agent_for_slash_command("/nonexistent").is_none());
    }

    #[test]
    fn gateway_agent_for_channel_name_matches_prefix() {
        let gw = gateway_with_agents(synthetic_agents());
        let a = gw
            .agent_for_channel_name("infra-alerts")
            .expect("infra- prefix should match homelab-sentinel");
        assert_eq!(a.name, "homelab-sentinel");
        // Exact match on just the prefix is also fine.
        assert!(gw.agent_for_channel_name("infra-status").is_some());
        // Case-insensitive.
        assert!(gw.agent_for_channel_name("INFRA-ALERTS").is_some());
        // Unrelated channel misses.
        assert!(gw.agent_for_channel_name("general").is_none());
    }

    #[test]
    fn gateway_agent_for_mention_matches_substring() {
        let gw = gateway_with_agents(synthetic_agents());
        let a = gw
            .agent_for_mention("Hey flaco, please review this rust function")
            .expect("mention pattern should hit rust-reviewer");
        assert_eq!(a.name, "rust-reviewer");
        // Case-insensitive.
        assert!(
            gw.agent_for_mention("REVIEW THIS RUST please").is_some()
        );
        // No match.
        assert!(gw.agent_for_mention("hello there").is_none());
    }
}
