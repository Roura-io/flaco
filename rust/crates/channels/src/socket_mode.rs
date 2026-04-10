//! Slack Socket Mode — connects to Slack via WebSocket instead of requiring
//! a public webhook URL. This is the preferred mode for local/development use.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::gateway::{ChannelPersona, Gateway};

/// Run the Slack Socket Mode connection loop.
///
/// This connects to Slack's WebSocket endpoint, receives events, processes
/// them through the Gateway, and sends responses back via the Web API.
pub async fn run_socket_mode(
    app_token: &str,
    bot_token: &str,
    gateway: Arc<Gateway>,
) -> Result<(), String> {
    let http = reqwest::Client::new();

    loop {
        tracing::info!("Requesting Socket Mode WebSocket URL...");

        // Step 1: Get a WebSocket URL from Slack (retry on failure)
        let ws_url = match get_websocket_url(&http, app_token).await {
            Ok(url) => url,
            Err(e) => {
                tracing::error!("Failed to get WebSocket URL: {e}. Retrying in 10 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
        };
        tracing::info!("Connecting to Slack WebSocket...");

        // Step 2: Connect via WebSocket (retry on failure)
        let (ws_stream, _) = match tokio_tungstenite::connect_async(&ws_url).await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::error!("WebSocket connect failed: {e}. Retrying in 10 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
        };

        let (mut write, mut read) = ws_stream.split();
        tracing::info!("Connected to Slack Socket Mode");

        // Step 3: Read events from the WebSocket
        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(Message::Text(text)) => text,
                Ok(Message::Ping(data)) => {
                    let _ = write.send(Message::Pong(data)).await;
                    continue;
                }
                Ok(Message::Close(_)) => {
                    tracing::warn!("Slack closed the WebSocket, reconnecting...");
                    break;
                }
                Ok(_) => continue,
                Err(e) => {
                    tracing::error!("WebSocket error: {e}");
                    break;
                }
            };

            let envelope: Value = match serde_json::from_str(msg.as_str()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Failed to parse Socket Mode envelope: {e}");
                    continue;
                }
            };

            // Acknowledge the envelope immediately (Slack requires this within 3s)
            let envelope_id = envelope["envelope_id"].as_str().unwrap_or("");
            if !envelope_id.is_empty() {
                let ack = json!({"envelope_id": envelope_id});
                let _ = write
                    .send(Message::Text(ack.to_string().into()))
                    .await;
            }

            let event_type = envelope["type"].as_str().unwrap_or("");

            match event_type {
                "hello" => {
                    tracing::info!("Slack Socket Mode handshake complete");
                }
                "disconnect" => {
                    tracing::info!("Slack requested disconnect, reconnecting...");
                    break;
                }
                "events_api" => {
                    let payload = &envelope["payload"];
                    let event = &payload["event"];
                    let event_type = event["type"].as_str().unwrap_or("");

                    // Skip bot messages and subtypes
                    if event["bot_id"].is_string() || event["subtype"].is_string() {
                        continue;
                    }

                    if event_type == "message" || event_type == "app_mention" {
                        let user = event["user"].as_str().unwrap_or("").to_string();
                        let channel = event["channel"].as_str().unwrap_or("").to_string();
                        let text = event["text"].as_str().unwrap_or("").to_string();
                        let thread_ts = event["thread_ts"]
                            .as_str()
                            .or_else(|| event["ts"].as_str())
                            .map(String::from);

                        if user.is_empty() || text.is_empty() {
                            continue;
                        }

                        let gateway = Arc::clone(&gateway);
                        let bot_token = bot_token.to_string();
                        let http = http.clone();

                        tokio::spawn(async move {
                            handle_slack_message(
                                &http,
                                &bot_token,
                                &gateway,
                                &user,
                                &channel,
                                &text,
                                thread_ts.as_deref(),
                            )
                            .await;
                        });
                    }
                }
                _ => {
                    tracing::debug!("Ignoring Socket Mode event type: {event_type}");
                }
            }
        }

        // Brief pause before reconnecting
        tracing::info!("Reconnecting in 2 seconds...");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Get a WebSocket URL from Slack's apps.connections.open endpoint.
async fn get_websocket_url(http: &reqwest::Client, app_token: &str) -> Result<String, String> {
    let resp: Value = http
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(app_token)
        .send()
        .await
        .map_err(|e| format!("connections.open failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("connections.open parse error: {e}"))?;

    if resp["ok"].as_bool() != Some(true) {
        return Err(format!(
            "connections.open error: {}",
            resp["error"].as_str().unwrap_or("unknown")
        ));
    }

    resp["url"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "no url in connections.open response".into())
}

/// Handle a single Slack message: get/create conversation, call Ollama, respond.
async fn handle_slack_message(
    http: &reqwest::Client,
    bot_token: &str,
    gateway: &Gateway,
    user: &str,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
) {
    // Add thinking reaction
    if let Some(ts) = thread_ts {
        let _ = http
            .post("https://slack.com/api/reactions.add")
            .bearer_auth(bot_token)
            .json(&json!({"channel": channel, "timestamp": ts, "name": "thinking_face"}))
            .send()
            .await;
    }

    // Strip bot mentions
    let clean_text = strip_mentions(text);

    // Handle special commands
    if clean_text.trim().eq_ignore_ascii_case("/reset") {
        gateway.reset_conversation("slack", user).await;
        send_slack_message(http, bot_token, channel, "Conversation reset! Starting fresh.", thread_ts).await;
        return;
    }

    if clean_text.trim().eq_ignore_ascii_case("/status") {
        let active = gateway.active_conversations().await;
        let msg = format!(
            "flacoAi is online. Model: `{}`. Active conversations: {active}.",
            gateway.model()
        );
        send_slack_message(http, bot_token, channel, &msg, thread_ts).await;
        return;
    }

    // Get or create conversation state
    let mut conversation = gateway
        .get_or_create_conversation("slack", user, user)
        .await;
    conversation.push_user(clean_text.clone());

    // Build prompt with persona + history
    let persona = ChannelPersona::slack();
    let mut prompt_parts = vec![persona.prompt_overlay.clone()];

    if conversation.messages.len() > 1 {
        let history: Vec<String> = conversation
            .messages
            .iter()
            .rev()
            .skip(1)
            .take(10)
            .rev()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect();
        if !history.is_empty() {
            prompt_parts.push(format!("Conversation history:\n{}", history.join("\n")));
        }
    }
    prompt_parts.push(format!("User message: {clean_text}"));

    // Call Ollama
    let ollama_url = gateway.ollama_url().trim_end_matches("/v1");
    let response = call_ollama(http, ollama_url, gateway.model(), &prompt_parts.join("\n\n")).await;

    match response {
        Ok(reply) => {
            conversation.push_assistant(reply.clone());
            gateway
                .update_conversation("slack", user, conversation)
                .await;

            // Split long messages for Slack's 4096 limit
            for part in split_message(&reply, 3900) {
                send_slack_message(http, bot_token, channel, &part, thread_ts).await;
            }
        }
        Err(e) => {
            tracing::error!("Ollama error: {e}");
            send_slack_message(
                http,
                bot_token,
                channel,
                &format!("Sorry, I hit an error: {e}"),
                thread_ts,
            )
            .await;
        }
    }
}

async fn call_ollama(
    http: &reqwest::Client,
    ollama_url: &str,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": false
    });

    let resp: Value = http
        .post(format!("{ollama_url}/api/chat"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Ollama error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Ollama parse error: {e}"))?;

    Ok(resp["message"]["content"]
        .as_str()
        .unwrap_or("I couldn't generate a response.")
        .to_string())
}

async fn send_slack_message(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
) {
    let mut body = json!({"channel": channel, "text": text});
    if let Some(ts) = thread_ts {
        body["thread_ts"] = Value::String(ts.to_string());
    }
    let _ = http
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await;
}

fn strip_mentions(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_mention = false;
    for ch in text.chars() {
        match ch {
            '<' => in_mention = true,
            '>' if in_mention => {
                in_mention = false;
            }
            _ if !in_mention => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}

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
        let split_at = remaining[..max_len].rfind('\n').unwrap_or(max_len);
        parts.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }
    parts
}
