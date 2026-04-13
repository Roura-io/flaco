//! flaco-slack-v2 — a thin Socket Mode Slack adapter that routes every
//! non-command message through flaco-core Runtime.
//!
//! This is intentionally minimal: v1 `channels::socket_mode` on the Mac
//! still handles production workflows. v2 runs alongside v1 and takes a
//! different bot token (or the same — Slack allows multiple parallel Socket
//! Mode connections from one app), so Chris can A/B them.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use flaco_core::features::Features;
use flaco_core::runtime::{Runtime, Surface};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: String,
}

impl SlackConfig {
    pub fn from_env() -> Result<Self> {
        let bot_token = std::env::var("SLACK_BOT_TOKEN").context("SLACK_BOT_TOKEN")?;
        let app_token = std::env::var("SLACK_APP_TOKEN").context("SLACK_APP_TOKEN")?;
        Ok(Self { bot_token, app_token })
    }
}

#[derive(Clone)]
pub struct SlackAdapter {
    pub runtime: Arc<Runtime>,
    pub features: Arc<Features>,
    pub config: SlackConfig,
    pub http: reqwest::Client,
}

impl SlackAdapter {
    pub fn new(runtime: Arc<Runtime>, features: Arc<Features>, config: SlackConfig) -> Self {
        Self {
            runtime,
            features,
            config,
            http: reqwest::Client::new(),
        }
    }

    async fn open_wss(&self) -> Result<String> {
        let resp = self
            .http
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(&self.config.app_token)
            .send()
            .await?;
        let body: Value = resp.json().await?;
        if body.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(anyhow!("apps.connections.open failed: {}", body));
        }
        Ok(body
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("no url in response"))?
            .to_string())
    }

    async fn post_message(&self, channel: &str, text: &str, thread_ts: Option<&str>) -> Result<()> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        if let Some(ts) = thread_ts {
            body.as_object_mut().unwrap().insert("thread_ts".into(), Value::String(ts.into()));
        }
        let resp = self
            .http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.config.bot_token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let j: Value = resp.json().await?;
        if j.get("ok").and_then(Value::as_bool) != Some(true) {
            warn!("slack post failed: {}", j);
        }
        Ok(())
    }

    pub async fn run(self) -> Result<()> {
        loop {
            match self.run_once().await {
                Ok(()) => info!("slack-v2 wss closed cleanly, reconnecting"),
                Err(e) => error!("slack-v2 wss error: {e}, reconnecting in 3s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    async fn run_once(&self) -> Result<()> {
        let url = self.open_wss().await?;
        info!("slack-v2 connecting to {}", url);
        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
        let (mut write, mut read) = ws.split();

        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => return Err(anyhow!("ws error: {e}")),
            };
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Ping(p) => {
                    write.send(Message::Pong(p)).await.ok();
                    continue;
                }
                Message::Close(_) => return Ok(()),
                _ => continue,
            };

            let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
            let env_type = frame.get("type").and_then(Value::as_str).unwrap_or("");

            // Always ack envelopes that have one.
            if let Some(env_id) = frame.get("envelope_id").and_then(Value::as_str) {
                let ack = serde_json::json!({ "envelope_id": env_id });
                write.send(Message::Text(ack.to_string().into())).await.ok();
            }

            match env_type {
                "hello" => info!("slack-v2: hello"),
                "disconnect" => return Ok(()),
                "events_api" => {
                    if let Some(payload) = frame.get("payload") {
                        let this = self.clone();
                        let payload = payload.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_event(payload).await {
                                error!("event handler error: {e}");
                            }
                        });
                    }
                }
                "slash_commands" => {
                    if let Some(payload) = frame.get("payload") {
                        let this = self.clone();
                        let payload = payload.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_slash(payload).await {
                                error!("slash handler error: {e}");
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn handle_event(&self, payload: Value) -> Result<()> {
        let event = payload.get("event").unwrap_or(&payload);
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        if event_type != "message" && event_type != "app_mention" { return Ok(()); }
        // Skip bot messages
        if event.get("bot_id").is_some() { return Ok(()); }
        let subtype = event.get("subtype").and_then(Value::as_str);
        if subtype.is_some() { return Ok(()); } // ignore edits/deletes/joins

        let channel = event.get("channel").and_then(Value::as_str).unwrap_or("").to_string();
        let user = event.get("user").and_then(Value::as_str).unwrap_or("unknown").to_string();
        let text = event.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        if text.trim().is_empty() { return Ok(()); }

        let session = self.runtime.session(&Surface::Slack, &user)?;
        let reply = self.runtime.handle_turn(&session, &text, None).await?;
        let reply = if reply.trim().is_empty() { "(no reply)".into() } else { reply };
        self.post_message(&channel, &reply, None).await?;
        Ok(())
    }

    async fn handle_slash(&self, payload: Value) -> Result<()> {
        let command = payload.get("command").and_then(Value::as_str).unwrap_or("");
        let text = payload.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let channel = payload.get("channel_id").and_then(Value::as_str).unwrap_or("").to_string();
        let user = payload.get("user_id").and_then(Value::as_str).unwrap_or("unknown").to_string();

        let response_text = match command {
            "/reset" | "/clear" | "/new" | "/forget" => {
                // Start a brand-new session for this user on Slack. We do this
                // by calling Session::start directly, which inserts a fresh
                // conversation row into memory — the next turn will resume it.
                use flaco_core::persona::PersonaRegistry;
                use flaco_core::session::Session;
                let personas = PersonaRegistry::defaults();
                let _ = Session::start(
                    self.runtime.memory.clone(),
                    &personas,
                    "slack",
                    &user,
                    None,
                );
                "Conversation reset. Starting fresh — what's up?".to_string()
            }
            "/help" => {
                "*flacoAi — powered by Roura.io*\n\
                 `/reset` or `/clear` — wipe this conversation and start fresh\n\
                 `/brief` — your morning brief (memory + open Jira)\n\
                 `/research <topic>` — web research with citations\n\
                 `/shortcut name: description` — generate a real Siri Shortcut\n\
                 `/scaffold <idea>` — Jira epic + stories + local git branch\n\
                 `/memories` — what I remember about you\n\
                 `/status` — model + memory counts\n\
                 Anything else — just talk to me normally."
                    .to_string()
            }
            "/status" => {
                let tools = self.runtime.tools.names();
                let mem_count = self.runtime.memory.all_facts(&user, 10_000).map(|v| v.len()).unwrap_or(0);
                format!(
                    "flacoAi v2 is online.\nModel: `{}`\nMemories: {mem_count}\nTools: {}",
                    self.runtime.ollama.model(),
                    tools.len()
                )
            }
            "/brief" => {
                match self.features.morning_brief(&user).await {
                    Ok(b) => b.markdown,
                    Err(e) => format!("brief error: {e}"),
                }
            }
            "/research" => {
                match self.features.research(&text).await {
                    Ok(r) => r.to_markdown(),
                    Err(e) => format!("research error: {e}"),
                }
            }
            "/shortcut" => {
                let (name, desc) = if let Some(idx) = text.find(':') {
                    (text[..idx].trim().to_string(), text[idx+1..].trim().to_string())
                } else {
                    ("Flaco Shortcut".into(), text.clone())
                };
                match self.features.create_shortcut(&name, &desc).await {
                    Ok(r) => r.output,
                    Err(e) => format!("shortcut error: {e}"),
                }
            }
            "/scaffold" => {
                match self.features.scaffold(&text, "FLACO", None).await {
                    Ok(r) => r.output,
                    Err(e) => format!("scaffold error: {e}"),
                }
            }
            "/memories" => {
                let mems = self.runtime.memory.all_facts(&user, 50).unwrap_or_default();
                if mems.is_empty() {
                    "(no memories yet)".to_string()
                } else {
                    mems.iter()
                        .map(|m| format!("#{} [{}] {}", m.id, m.kind, m.content))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            _ => {
                // Fall through to normal chat using the slash text.
                let session = self.runtime.session(&Surface::Slack, &user)?;
                self.runtime.handle_turn(&session, &text, None).await.unwrap_or_else(|e| format!("error: {e}"))
            }
        };
        self.post_message(&channel, &response_text, None).await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct _PlaceholderForCompile;
