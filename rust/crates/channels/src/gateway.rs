//! Central message gateway — routes messages from channels through the AI
//! with per-sender conversation state and persona overlays.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Timeout for conversation state (2 hours).
const CONVERSATION_TIMEOUT: Duration = Duration::from_secs(7200);

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
}

/// Gateway configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Ollama model to use for channel conversations.
    pub model: Option<String>,
    /// Ollama base URL.
    pub ollama_url: Option<String>,
    /// Per-channel personas.
    #[serde(default)]
    pub personas: Vec<ChannelPersona>,
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

    /// Get the configured model.
    #[must_use]
    pub fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("qwen3:32b")
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
}
