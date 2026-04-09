//! Slack channel — Events API webhook handler + Web API client.
//!
//! Handles incoming Slack messages, routes them through the Gateway,
//! and responds with human-like conversational messages.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::gateway::{ChannelPersona, Gateway};

// ---------------------------------------------------------------------------
// Slack API types
// ---------------------------------------------------------------------------

/// Slack Events API request body.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum SlackEvent {
    /// URL verification challenge (sent once during app setup).
    UrlVerification { challenge: String },
    /// Normal event callback.
    EventCallback { event: SlackEventPayload },
}

/// The inner event payload.
#[derive(Debug, Deserialize)]
struct SlackEventPayload {
    /// Event type: "message", "app_mention", etc.
    #[serde(rename = "type")]
    event_type: String,
    /// Message text.
    #[serde(default)]
    text: String,
    /// User ID of the sender.
    #[serde(default)]
    user: String,
    /// Channel ID.
    #[serde(default)]
    channel: String,
    /// Thread timestamp (for threaded replies).
    #[serde(default)]
    thread_ts: Option<String>,
    /// Message timestamp (used as thread parent if no thread_ts).
    #[serde(default)]
    ts: Option<String>,
    /// Bot ID (to avoid responding to ourselves).
    #[serde(default)]
    bot_id: Option<String>,
    /// Subtype (to ignore message_changed, etc.).
    #[serde(default)]
    subtype: Option<String>,
}

/// Slack Web API response.
#[derive(Debug, Deserialize)]
struct SlackApiResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Slack channel
// ---------------------------------------------------------------------------

/// Configuration for the Slack channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    /// Slack bot token (xoxb-...).
    pub bot_token: String,
    /// Slack signing secret for request verification.
    pub signing_secret: String,
    /// Optional: specific channel IDs to listen to (empty = all).
    #[serde(default)]
    pub allowed_channels: Vec<String>,
}

impl SlackConfig {
    /// Load from environment variables.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let bot_token = std::env::var("SLACK_BOT_TOKEN").ok()?;
        let signing_secret = std::env::var("SLACK_SIGNING_SECRET").ok()?;
        let allowed_channels = std::env::var("SLACK_ALLOWED_CHANNELS")
            .ok()
            .map(|s| s.split(',').map(str::trim).map(String::from).collect())
            .unwrap_or_default();
        Some(Self {
            bot_token,
            signing_secret,
            allowed_channels,
        })
    }
}

/// The Slack channel handler.
pub struct SlackChannel {
    config: SlackConfig,
    gateway: Arc<Gateway>,
    http: reqwest::Client,
}

impl SlackChannel {
    /// Create a new Slack channel with the given config and gateway.
    #[must_use]
    pub fn new(config: SlackConfig, gateway: Arc<Gateway>) -> Self {
        Self {
            config,
            gateway,
            http: reqwest::Client::new(),
        }
    }

    /// Build the Axum router for Slack webhook endpoints.
    #[must_use]
    pub fn router(self) -> Router {
        let state = Arc::new(RwLock::new(self));
        Router::new()
            .route("/slack/events", post(handle_slack_event))
            .with_state(state)
    }

    /// Send a message to a Slack channel (or reply in a thread).
    pub async fn send_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<(), String> {
        let mut body = json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = Value::String(ts.to_string());
        }

        let resp = self
            .http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.config.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Slack API error: {e}"))?;

        let api_resp: SlackApiResponse = resp.json().await.map_err(|e| e.to_string())?;
        if api_resp.ok {
            Ok(())
        } else {
            Err(format!(
                "Slack API error: {}",
                api_resp.error.unwrap_or_else(|| "unknown".into())
            ))
        }
    }

    /// Add a reaction to a message.
    pub async fn add_reaction(
        &self,
        channel: &str,
        timestamp: &str,
        emoji: &str,
    ) -> Result<(), String> {
        let body = json!({
            "channel": channel,
            "timestamp": timestamp,
            "name": emoji,
        });

        let _resp = self
            .http
            .post("https://slack.com/api/reactions.add")
            .bearer_auth(&self.config.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Slack reactions API error: {e}"))?;

        Ok(())
    }

    /// Process an incoming Slack message and generate a response.
    async fn handle_message(&self, event: &SlackEventPayload) -> Result<String, String> {
        let mut conversation = self
            .gateway
            .get_or_create_conversation("slack", &event.user, &event.user)
            .await;

        // Strip bot mentions from the message text
        let clean_text = strip_slack_mentions(&event.text);
        if clean_text.trim().is_empty() {
            return Ok("Hey! What can I help you with?".into());
        }

        // Check for slash-command-like messages
        if clean_text.trim().eq_ignore_ascii_case("/reset") {
            self.gateway.reset_conversation("slack", &event.user).await;
            return Ok("Conversation reset! Starting fresh.".into());
        }

        if clean_text.trim().eq_ignore_ascii_case("/status") {
            let active = self.gateway.active_conversations().await;
            return Ok(format!(
                "flacoAi is online. Model: `{}`. Active conversations: {active}.",
                self.gateway.model()
            ));
        }

        // Add user message to conversation
        conversation.push_user(clean_text.clone());

        // Build the prompt with conversation history + persona overlay
        let persona = ChannelPersona::slack();
        let mut prompt_parts = vec![persona.prompt_overlay.clone()];

        // Add conversation history context
        if conversation.messages.len() > 1 {
            let history: Vec<String> = conversation
                .messages
                .iter()
                .rev()
                .skip(1) // skip the current message (we'll send it as the prompt)
                .take(10) // last 10 messages for context
                .rev()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect();
            if !history.is_empty() {
                prompt_parts.push(format!("Conversation history:\n{}", history.join("\n")));
            }
        }

        prompt_parts.push(format!("User message: {clean_text}"));
        let full_prompt = prompt_parts.join("\n\n");

        // Call Ollama via HTTP (simple completion, no tool calling for Slack)
        let response = self.call_ollama(&full_prompt).await?;

        // Store assistant response
        conversation.push_assistant(response.clone());
        self.gateway
            .update_conversation("slack", &event.user, conversation)
            .await;

        Ok(response)
    }

    /// Call Ollama for a simple chat completion.
    async fn call_ollama(&self, prompt: &str) -> Result<String, String> {
        let ollama_url = self.gateway.ollama_url().trim_end_matches("/v1");
        let url = format!("{ollama_url}/api/chat");

        let body = json!({
            "model": self.gateway.model(),
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": false
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {e}"))?;

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Ollama parse error: {e}"))?;
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("I couldn't generate a response. Try again?");

        Ok(content.to_string())
    }
}

/// Verify Slack request signature using HMAC-SHA256.
fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &str,
    signature: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let sig_basestring = format!("v0:{timestamp}:{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes())
        .expect("HMAC key should be valid");
    mac.update(sig_basestring.as_bytes());
    let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    expected == signature
}

/// Strip Slack user/bot mentions like <@U12345> from text.
fn strip_slack_mentions(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_mention = false;
    for ch in text.chars() {
        match ch {
            '<' if !in_mention => in_mention = true,
            '>' if in_mention => {
                in_mention = false;
                result.push(' ');
            }
            _ if !in_mention => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}

/// Split a long message for Slack's 4096 char limit.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut parts = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            parts.push(remaining.to_string());
            break;
        }
        // Try to split at a newline
        let split_at = remaining[..max_len].rfind('\n').unwrap_or(max_len);
        parts.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }
    parts
}

// ---------------------------------------------------------------------------
// Axum handler
// ---------------------------------------------------------------------------

type SlackState = Arc<RwLock<SlackChannel>>;

async fn handle_slack_event(
    State(state): State<SlackState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Parse the event
    let event: SlackEvent = match serde_json::from_str(&body) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to parse Slack event: {e}");
            return (StatusCode::BAD_REQUEST, "invalid event".to_string());
        }
    };

    match event {
        SlackEvent::UrlVerification { challenge } => (StatusCode::OK, challenge),
        SlackEvent::EventCallback { event: payload } => {
            // Verify signature
            let channel = state.read().await;
            let timestamp = headers
                .get("x-slack-request-timestamp")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let signature = headers
                .get("x-slack-signature")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if !verify_slack_signature(&channel.config.signing_secret, timestamp, &body, signature)
            {
                tracing::warn!("Invalid Slack signature");
                return (StatusCode::UNAUTHORIZED, "invalid signature".to_string());
            }
            drop(channel);

            // Ignore bot messages and subtypes (edits, etc.)
            if payload.bot_id.is_some() || payload.subtype.is_some() {
                return (StatusCode::OK, String::new());
            }

            // Only handle "message" and "app_mention" events
            if payload.event_type != "message" && payload.event_type != "app_mention" {
                return (StatusCode::OK, String::new());
            }

            // Process in background so we respond to Slack within 3s
            let state_clone = Arc::clone(&state);
            let payload_clone = payload;
            tokio::spawn(async move {
                let channel = state_clone.read().await;

                // Add a thinking reaction
                let msg_ts = payload_clone.ts.as_deref().unwrap_or("");
                let _ = channel
                    .add_reaction(&payload_clone.channel, msg_ts, "thinking_face")
                    .await;

                // Generate response
                match channel.handle_message(&payload_clone).await {
                    Ok(response) => {
                        let thread_ts = payload_clone
                            .thread_ts
                            .as_deref()
                            .or(payload_clone.ts.as_deref());

                        // Split long messages
                        for part in split_message(&response, 3900) {
                            if let Err(e) = channel
                                .send_message(&payload_clone.channel, &part, thread_ts)
                                .await
                            {
                                tracing::error!("Failed to send Slack message: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to handle Slack message: {e}");
                        let _ = channel
                            .send_message(
                                &payload_clone.channel,
                                &format!("Sorry, I hit an error: {e}"),
                                payload_clone.thread_ts.as_deref(),
                            )
                            .await;
                    }
                }
            });

            (StatusCode::OK, String::new())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_slack_mentions() {
        assert_eq!(strip_slack_mentions("<@U12345> hello"), "hello");
        assert!(strip_slack_mentions("hey <@U123> check this").contains("check this"));
        assert_eq!(strip_slack_mentions("no mentions"), "no mentions");
    }

    #[test]
    fn splits_long_messages() {
        let short = "hello world";
        assert_eq!(split_message(short, 100), vec!["hello world"]);

        let long = "line1\nline2\nline3\nline4";
        let parts = split_message(long, 12);
        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|p| p.len() <= 12));
    }

    #[test]
    fn verifies_slack_signature() {
        // Known test values from Slack docs
        let secret = "8f742231b10e8888abcd99yez67543aa";
        let timestamp = "1531420618";
        let body = "token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow&channel_id=G8LX6JTS7&channel_name=mpdm-kev--]]";

        // This won't match because the values are synthetic, but it shouldn't panic
        let result = verify_slack_signature(secret, timestamp, body, "v0=abc123");
        assert!(!result); // Expected to fail with wrong signature
    }

    #[test]
    fn slack_config_from_env_returns_none_when_missing() {
        // Clean env
        std::env::remove_var("SLACK_BOT_TOKEN");
        std::env::remove_var("SLACK_SIGNING_SECRET");
        assert!(SlackConfig::from_env().is_none());
    }
}
