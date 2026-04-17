//! Slack Socket Mode — connects to Slack via WebSocket instead of requiring
//! a public webhook URL. This is the preferred mode for local/development use.

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::agents::VetMode;
use crate::gateway::{ChannelPersona, Gateway};
use crate::inference::{call_ollama, claude_check, needs_web_search, web_search, CheckResult};

/// Call Slack's auth.test once to discover this app's `bot_id`. Used by the
/// server binary on startup so it can populate `GatewayConfig.our_bot_id`
/// without any hardcoded constants. Returns `None` on any error (network,
/// auth, parse) so the caller can fail loudly with its own error message.
///
/// This lives in the channels crate (which already owns reqwest) so the
/// server binary doesn't have to grow a reqwest dependency just for one call.
pub async fn fetch_bot_id(bot_token: &str) -> Option<String> {
    let http = reqwest::Client::new();
    let resp: Value = http
        .post("https://slack.com/api/auth.test")
        .bearer_auth(bot_token)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    if resp["ok"].as_bool() != Some(true) {
        return None;
    }
    resp["bot_id"].as_str().map(String::from)
}

/// Tracks pending (buffered) messages per user and whether a response is
/// already in flight, so we can batch rapid-fire messages into one reply.
struct MessageBuffer {
    /// Queued messages per "channel:user" key.
    pending: HashMap<String, Vec<String>>,
    /// Whether a response task is currently running for this key.
    in_flight: HashMap<String, bool>,
    /// Recently seen event timestamps — used to deduplicate events that Slack
    /// sends as both `message` and `app_mention`.
    seen_events: Vec<String>,
}

impl MessageBuffer {
    fn new() -> Self {
        Self {
            pending: HashMap::new(),
            in_flight: HashMap::new(),
            seen_events: Vec::new(),
        }
    }

    /// Returns true if this event timestamp was already seen (duplicate).
    fn is_duplicate(&mut self, ts: &str) -> bool {
        if self.seen_events.contains(&ts.to_string()) {
            return true;
        }
        self.seen_events.push(ts.to_string());
        // Keep only last 100 to avoid unbounded growth
        if self.seen_events.len() > 100 {
            self.seen_events.drain(..50);
        }
        false
    }
}

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
    let buffer = Arc::new(Mutex::new(MessageBuffer::new()));

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
                let _ = write.send(Message::Text(ack.to_string().into())).await;
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

                    // DIAG: log every incoming event so we can see what Slack
                    // is actually delivering (independent of any filter logic).
                    tracing::info!(
                        target: "socket_mode",
                        event_type = %event_type,
                        bot_id = %event["bot_id"].as_str().unwrap_or("-"),
                        subtype = %event["subtype"].as_str().unwrap_or("-"),
                        user = %event["user"].as_str().unwrap_or("-"),
                        text_preview = %event["text"].as_str().unwrap_or("").chars().take(60).collect::<String>(),
                        "INCOMING SLACK EVENT"
                    );

                    // ENV-GATED TEST ESCAPE HATCH: when FLACO_TEST_MODE=1 is
                    // set in the process environment, messages containing the
                    // literal marker `[flaco-test]` are allowed through even
                    // from our own bot. This lets Claude drive end-to-end
                    // Slack tests via the bot token without being able to
                    // impersonate a human user. Without FLACO_TEST_MODE set,
                    // the marker does nothing and bot filters apply strictly.
                    //
                    // The check has to come BEFORE the bot_id == our_bot_id
                    // filter — otherwise our own [flaco-test] posts get
                    // dropped at the first filter and the bypass never runs.
                    let test_text = event["text"].as_str().unwrap_or("");
                    let is_test_post = test_text.contains("[flaco-test]")
                        && std::env::var("FLACO_TEST_MODE").ok().filter(|v| !v.is_empty()).is_some();

                    // Skip our own bot replies (prevents feedback loops) and
                    // message subtypes like "message_changed" / "message_deleted".
                    // Other bots' messages (deadman, netguardian) are real signal —
                    // don't drop them before we see them. They'll be re-fetched
                    // later via conversations.history for vet-layer context.
                    //
                    // our_bot_id is loaded once at startup via auth.test and
                    // stored in Gateway — no more hardcoded constants.
                    let our_bot_id = gateway.our_bot_id();
                    if !is_test_post && !our_bot_id.is_empty() && event["bot_id"].as_str() == Some(our_bot_id) {
                        continue;
                    }
                    // Still drop subtypes that aren't new content (edits, deletes,
                    // thread_broadcast metadata, etc.). "bot_message" is the one
                    // subtype we keep so non-self bots are still ingested.
                    if let Some(subtype) = event["subtype"].as_str() {
                        if subtype != "bot_message" {
                            continue;
                        }
                    }
                    // Never respond to messages authored by ANY bot unless
                    // they contain the [flaco-test] marker AND FLACO_TEST_MODE is set.
                    if !is_test_post && event["bot_id"].is_string() {
                        continue;
                    }

                    if event_type == "message" || event_type == "app_mention" {
                        let user = event["user"].as_str().unwrap_or("").to_string();
                        let channel = event["channel"].as_str().unwrap_or("").to_string();
                        let text = event["text"].as_str().unwrap_or("").to_string();
                        let event_ts = event["ts"].as_str().unwrap_or("").to_string();

                        if user.is_empty() || text.is_empty() {
                            continue;
                        }

                        // Deduplicate: Slack sends the same message as both
                        // "message" and "app_mention" events.
                        {
                            let mut b = buffer.lock().await;
                            if b.is_duplicate(&event_ts) {
                                tracing::debug!("Skipping duplicate event {event_ts}");
                                continue;
                            }
                        }

                        let key = format!("{channel}:{user}");
                        let buf = Arc::clone(&buffer);
                        let gateway = Arc::clone(&gateway);
                        let bot_token = bot_token.to_string();
                        let http = http.clone();

                        // Add message to the buffer
                        {
                            let mut b = buf.lock().await;
                            b.pending.entry(key.clone()).or_default().push(text);
                        }

                        // If a response is already in flight for this user,
                        // the message will be picked up when it finishes.
                        // Otherwise, spawn a handler with a short debounce.
                        let already_running = {
                            let b = buf.lock().await;
                            *b.in_flight.get(&key).unwrap_or(&false)
                        };

                        if !already_running {
                            let key2 = key.clone();
                            let buf2 = Arc::clone(&buf);
                            tokio::spawn(async move {
                                // Mark in-flight
                                {
                                    buf2.lock().await.in_flight.insert(key2.clone(), true);
                                }

                                // Short debounce — wait for rapid follow-up messages
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                                // Drain all pending messages for this user
                                loop {
                                    let messages = {
                                        let mut b = buf2.lock().await;
                                        b.pending.remove(&key2).unwrap_or_default()
                                    };
                                    if messages.is_empty() {
                                        break;
                                    }

                                    let combined = messages.join("\n\n");
                                    handle_slack_message(
                                        &http, &bot_token, &gateway, &user, &channel, &combined,
                                        None,
                                    )
                                    .await;

                                    // Check if more messages arrived while we were responding
                                    let has_more = {
                                        let b = buf2.lock().await;
                                        b.pending.get(&key2).is_some_and(|v| !v.is_empty())
                                    };
                                    if !has_more {
                                        break;
                                    }
                                }

                                // Clear in-flight
                                {
                                    buf2.lock().await.in_flight.insert(key2, false);
                                }
                            });
                        }
                    }
                }
                "slash_commands" => {
                    let payload = envelope["payload"].clone();
                    let cmd = payload["command"].as_str().unwrap_or("").to_string();
                    let text = payload["text"].as_str().unwrap_or("").trim().to_string();
                    let channel = payload["channel_id"].as_str().unwrap_or("").to_string();
                    let user = payload["user_id"].as_str().unwrap_or("").to_string();
                    if channel.is_empty() || user.is_empty() {
                        continue;
                    }
                    let http = http.clone();
                    let bot_token = bot_token.to_string();
                    let gateway = Arc::clone(&gateway);
                    tokio::spawn(async move {
                        handle_slash_command(&http, &bot_token, &gateway, &user, &channel, &cmd, &text).await;
                    });
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

/// Handle a Slack slash command invocation coming through Socket Mode.
/// Supports the small set of commands we register in the Slack app manifest.
async fn handle_slash_command(
    http: &reqwest::Client,
    bot_token: &str,
    gateway: &Gateway,
    user: &str,
    channel: &str,
    command: &str,
    text: &str,
) {
    match command {
        // === MEMORY ONLY: reset flacoAi's per-user conversation state ===
        // Does NOT delete any Slack messages.
        "/reset" | "/clear" | "/new" | "/forget" => {
            gateway.reset_conversation("slack", user).await;
            send_channel_message(
                http, bot_token, channel,
                "Memory reset. Starting fresh — what's up?\n\
                _(Channel messages stay. Use `/wipe` to also delete my old replies.)_"
            ).await;
        }
        // === BOT MESSAGES ONLY: delete flacoAi's own messages from this channel ===
        // Does NOT reset memory. Does NOT touch user messages (Slack disallows).
        // Confirmation is EPHEMERAL (only the invoker sees it) so the act of
        // cleaning isn't immediately re-cluttered by a "I did the thing" reply.
        "/purge" => {
            let (deleted, errors) = purge_own_messages(http, bot_token, channel, 200, gateway.our_bot_id()).await;
            let msg = format!(
                "✓ Purged {deleted} of my messages. Errors: {errors}\n\
                _(Memory unchanged. Use `/wipe` to also reset memory.)_"
            );
            send_ephemeral_message(http, bot_token, channel, user, &msg).await;
        }
        // === EVERYTHING WE CAN: reset memory + purge bot messages ===
        // Closest to "remove everything" without admin scopes. Confirmation
        // is EPHEMERAL for the same cleanliness reason as /purge.
        "/wipe" => {
            gateway.reset_conversation("slack", user).await;
            let (deleted, errors) = purge_own_messages(http, bot_token, channel, 200, gateway.our_bot_id()).await;
            let msg = format!(
                "✓ Wiped {deleted} of my messages + reset conversation memory. Errors: {errors}\n\n\
                _Note: I can't delete your own messages — Slack only lets bots delete their own output. \
                To clear yours, click each message and choose Delete._"
            );
            send_ephemeral_message(http, bot_token, channel, user, &msg).await;
        }
        // === BOT STATUS: /health (was /status, but Slack reserves /status) ===
        "/health" | "/status" => {
            let active = gateway.active_conversations().await;
            let msg = format!(
                "flacoAi online · model `{}` · {active} active conversations",
                gateway.model()
            );
            send_channel_message(http, bot_token, channel, &msg).await;
        }
        "/help" => {
            let help = "*flacoAi — powered by Roura.io*\n\n\
                `/clear` (or `/reset`) — reset my memory of this conversation. *Does not delete any Slack messages.*\n\
                `/purge` — delete all my past replies in this channel. *Does not reset memory.*\n\
                `/wipe` — both: reset memory AND delete my past replies. *Closest to a fresh slate. Cannot delete your own messages — Slack disallows.*\n\
                `/health` — show my model and active conversation count\n\
                `/help` — this message\n\n\
                Or just @-mention me or DM me to chat.";
            send_channel_message(http, bot_token, channel, help).await;
        }
        _ => {
            // Unknown slash command: treat the argument text as a normal message
            // so the user still gets a useful response.
            let fallback = if text.is_empty() {
                format!("I don't know the `{command}` command yet. Try `/help`.")
            } else {
                handle_slack_message_sync(http, bot_token, gateway, user, channel, text, None).await;
                return;
            };
            send_channel_message(http, bot_token, channel, &fallback).await;
        }
    }
}

/// Synchronous helper wrapping `handle_slack_message` for the slash command path.
async fn handle_slack_message_sync(
    http: &reqwest::Client,
    bot_token: &str,
    gateway: &Gateway,
    user: &str,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
) {
    handle_slack_message(http, bot_token, gateway, user, channel, text, thread_ts).await;
}

/// Handle a single Slack message: get/create conversation, call Ollama, respond.
/// First-contact welcome banner shown exactly once per Slack user.
///
/// Durability is a local file (`~/.flaco-v1-welcome-seen.txt`, one user id
/// per line). This is deliberately decoupled from v2's SQLite
/// `user_state` table so v1 `channels` doesn't grow a `flaco-core`
/// dependency — the "v1 untouched" safety rail stays in place.
const V1_WELCOME: &str = "\
:wave: *Hi, I'm flaco — your local AI assistant.*\n\
I run entirely on your Mac via Ollama. No cloud, \
no API keys leaving the house.\n\
\n\
*Things I'm good at — try any of these:*\n\
• Just *talk to me* — ask a question, I'll research with real citations\n\
• \"*who do the yankees play today?*\" — real MLB data, I know today's date\n\
• \"*what's on my plate today?*\" — I'll pull from memory + open Jira\n\
• \"*clear*\", \"*reset*\", \"*new chat*\" — instant new conversation, any phrasing\n\
• `/help` — full command list\n\
\n\
*Why it's useful:* I can look up live sports/news without \
hallucinating (I know what year it is), draft emails, fact-check \
rumors before you act on them, and remember what you tell me across \
Slack, terminal, and the web UI.\n\
\n\
You'll only see this message once. Go ahead and try something.";

fn welcome_file_path() -> std::path::PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        std::path::PathBuf::from(h).join(".flaco-v1-welcome-seen.txt")
    } else {
        std::path::PathBuf::from("/tmp/.flaco-v1-welcome-seen.txt")
    }
}

/// Atomic check-and-set: returns true if the user has NOT seen the
/// welcome banner yet (and records that they have now). Returns false
/// on repeat visits or on any I/O error (never block the hot path).
fn claim_welcome_for(user: &str) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let path = welcome_file_path();
    // Ensure parent exists (HOME should always exist but be defensive).
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Read existing ids
    if let Ok(f) = std::fs::File::open(&path) {
        let seen: std::collections::HashSet<String> = BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if seen.contains(user) {
            return false;
        }
    }
    // Append the new user id
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        // Writing the id is what claims the welcome. Failure = don't
        // show the banner this time, because we couldn't persist the
        // fact that we showed it (so it'd show again next time).
        if writeln!(f, "{}", user).is_ok() {
            return true;
        }
    }
    false
}

async fn handle_slack_message(
    http: &reqwest::Client,
    bot_token: &str,
    gateway: &Gateway,
    user: &str,
    channel: &str,
    text: &str,
    _thread_ts: Option<&str>,
) {
    // First-contact welcome banner. Runs before any command handling or
    // LLM routing so a brand-new user gets a one-screen primer on what
    // flaco is and what to try. Fires exactly once per Slack user id,
    // persisted via ~/.flaco-v1-welcome-seen.txt. Failing silently is
    // fine — welcome is a nice-to-have, not part of the critical path.
    if claim_welcome_for(user) {
        send_channel_message(http, bot_token, channel, V1_WELCOME).await;
    }

    // Strip bot mentions
    let mut clean_text = strip_mentions(text);

    // Strip the [flaco-test] marker if present — it's a test-only escape
    // hatch for posting via the bot token. Without stripping, downstream
    // normalization sees "[flaco-test] purge" instead of "purge" and the
    // PURGE_PHRASINGS check fails. The marker has done its job (got the
    // event past the bot filter) by the time we reach here.
    if let Some(stripped) = clean_text.strip_prefix("[flaco-test]") {
        clean_text = stripped.trim_start().to_string();
    } else if clean_text.contains("[flaco-test]") {
        clean_text = clean_text.replace("[flaco-test]", "").trim().to_string();
    }

    // Handle special commands — respond directly in channel.
    //
    // Natural-language reset phrasings (kept in sync with
    // flaco-core::intent::detect — v1 channels doesn't depend on
    // flaco-core to avoid a v1 → v2 coupling, so this list is
    // deliberately duplicated. If you add a phrasing here, also add
    // it to crates/flaco-core/src/intent.rs RESET_EXACT.)
    let trimmed = clean_text.trim().trim_end_matches(['?', '!', '.', ',']);
    let lowered = trimmed.to_ascii_lowercase();
    let normalized = lowered
        .trim_start_matches('/')
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    const RESET_PHRASINGS: &[&str] = &[
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

    if RESET_PHRASINGS.contains(&normalized.as_str()) {
        gateway.reset_conversation("slack", user).await;
        send_channel_message(
            http,
            bot_token,
            channel,
            "Conversation reset. Starting fresh — what's up?",
        )
        .await;
        return;
    }

    // Natural-language commands — same semantics as the slash variants.
    // PURGE = delete bot messages only. WIPE = reset memory + delete bot messages.
    const PURGE_PHRASINGS: &[&str] = &[
        "purge",
        "purge channel",
        "purge this channel",
        "purge the channel",
    ];
    const WIPE_PHRASINGS: &[&str] = &[
        "wipe",
        "wipe channel",
        "wipe this channel",
        "wipe the channel",
        "clean channel",
        "clean up channel",
        "empty channel",
        "clear all",
        "clear all messages",
        "fresh slate",
        "factory reset",
    ];

    tracing::info!(
        target: "socket_mode",
        normalized = %normalized,
        text_len = normalized.len(),
        "checking purge/wipe phrasings"
    );

    let is_purge = PURGE_PHRASINGS.contains(&normalized.as_str())
        || normalized == "purge"
        || normalized.starts_with("purge ");
    let is_wipe = WIPE_PHRASINGS.contains(&normalized.as_str())
        || normalized == "wipe"
        || normalized.starts_with("wipe ");

    if is_purge {
        // Natural-language purge: completely SILENT. The visible cleaning of
        // the channel IS the confirmation. Posting "I did the thing" would
        // immediately re-clutter the channel the user just cleaned.
        tracing::info!(target: "socket_mode", "PURGE FIRED (silent)");
        let (deleted, errors) = purge_own_messages(http, bot_token, channel, 200, gateway.our_bot_id()).await;
        tracing::info!(target: "socket_mode", deleted, errors, "purge complete");
        return;
    }

    if is_wipe {
        // Same reasoning: silent. The user typed `wipe`, the channel gets
        // cleaner, that's the proof. No re-clutter.
        tracing::info!(target: "socket_mode", "WIPE FIRED (silent)");
        gateway.reset_conversation("slack", user).await;
        let (deleted, errors) = purge_own_messages(http, bot_token, channel, 200, gateway.our_bot_id()).await;
        tracing::info!(target: "socket_mode", deleted, errors, "wipe complete");
        return;
    }

    if trimmed.eq_ignore_ascii_case("/help") {
        let help = "*flacoAi commands*\n\
            `/reset` or `/clear` — wipe this conversation and start fresh\n\
            `/status` — model + active conversation count\n\
            `/help` — this message\n\
            Anything else — just talk to me normally.";
        send_channel_message(http, bot_token, channel, help).await;
        return;
    }

    if clean_text.trim().eq_ignore_ascii_case("/status") {
        let active = gateway.active_conversations().await;
        let msg = format!(
            "flacoAi is online. Model: `{}`. Active conversations: {active}.",
            gateway.model()
        );
        send_channel_message(http, bot_token, channel, &msg).await;
        return;
    }

    // ---------------------------------------------------------------
    // Agent dispatch — try to match the user message to a registered
    // agent BEFORE domain classification. Slash commands take priority
    // over mention patterns. If an agent matches, it overrides the
    // persona overlay, model, and vet policy for this turn.
    // ---------------------------------------------------------------
    let active_agent = if clean_text.starts_with('/') {
        gateway.agent_for_slash_command(&clean_text)
    } else {
        None
    }
    .or_else(|| gateway.agent_for_mention(&clean_text));

    if let Some(agent) = &active_agent {
        tracing::info!(
            target: "socket_mode",
            agent = %agent.name,
            "agent dispatched for turn"
        );
    }

    // Domain context routing (v1 backport of flaco-core::domain).
    //
    // Classify the user message into one of 7 domains, check that the
    // required env vars are set (preflight), and build a transient
    // system-prompt stanza containing the domain's API patterns, auth
    // hints, and any ground-truth files that should be auto-read. This
    // is how v1 knows to use the UniFi cloud API with $UNIFI_API_KEY
    // instead of falling back to "ask the user for admin creds" when
    // someone types `check my unifi`.
    //
    // The UnasSave stanza is special-cased as an ADDITIVE context that
    // stacks on top of whatever primary domain fired. "save this to my
    // unas" is classified as UnasSave directly, but "research X and
    // save it to my unas" classifies as General/Homelab/etc. with the
    // save recipe stacked on top so flaco knows how to do both.
    let domain = crate::domain::classify_message(&clean_text);
    tracing::info!(target: "socket_mode", domain = %domain, "classified turn domain");
    if let Err(preflight_msg) = crate::domain::preflight(domain) {
        tracing::warn!(target: "socket_mode", domain = %domain, "preflight failed: {preflight_msg}");
        send_channel_message(http, bot_token, channel, &preflight_msg).await;
        return;
    }
    let mut domain_context = crate::domain::build_context(domain);
    if domain != crate::domain::Domain::UnasSave
        && crate::domain::also_wants_save(&clean_text)
    {
        tracing::info!(target: "socket_mode", "stacking UnasSave stanza on top of primary domain");
        domain_context.push_str(&crate::domain::build_context(
            crate::domain::Domain::UnasSave,
        ));
    }

    // Resolve the channel id to a human-readable name so we can pick the
    // right persona (infra vs default) and drive the "flacoAi Pro"
    // vet-layer branding. One API call the first time we see a channel;
    // cached thereafter.
    let channel_name = fetch_channel_name(http, bot_token, channel)
        .await
        .unwrap_or_else(|| channel.to_string());

    // DMs (channel IDs starting with "D") are always 1:1 personal
    // conversations — route them through the infra/vetted persona by
    // default so the user gets the highest-quality + claude-vetted
    // experience in their personal channel with flacoAi. Other channels
    // route by name as usual.
    let channel_persona = if channel.starts_with('D') {
        ChannelPersona::infra()
    } else {
        ChannelPersona::for_channel(&channel_name)
    };

    // When an agent is active, its vet policy overrides the channel persona:
    //   Required → force vetting ON
    //   Off      → force vetting OFF
    //   Optional → inherit from the channel persona as usual
    let vet_enabled = match active_agent.as_ref().map(|a| a.vet) {
        Some(VetMode::Required) => true,
        Some(VetMode::Off) => false,
        _ => channel_persona.needs_vetting(),
    };

    // Orchestrator: pick the right local model for THIS turn.
    // - agent with model override → use that model unconditionally
    // - infra channels → large (e.g. qwen3:32b-q8_0)
    // - short messages in non-critical channels → small (e.g. nemotron-mini)
    // - everything else → medium (default FLACO_MODEL)
    let chosen_model = if let Some(model) = active_agent.as_ref().and_then(|a| a.model.as_deref()) {
        tracing::info!(
            target: "socket_mode",
            agent = %active_agent.as_ref().unwrap().name,
            model = %model,
            "agent overriding model selection"
        );
        model.to_string()
    } else {
        gateway.pick_model(&channel_persona, &clean_text)
    };

    // Branding:
    //   `flacoAi thinking... <model>`              — local only (dev, general)
    //   `flacoAi Pro thinking... <model> × claude` — vet layer on (infra)
    let thinking_text = if vet_enabled {
        format!("_flacoAi Pro is thinking..._  `{chosen_model} × claude`")
    } else {
        format!("_flacoAi is thinking..._  `{chosen_model}`")
    };
    let thinking_ts = post_thinking_message(http, bot_token, channel, &thinking_text).await;

    // Get or create per-user conversation state (used for back-and-forth
    // continuity — persists across turns within this user's chat with flaco)
    let mut conversation = gateway
        .get_or_create_conversation("slack", user, user)
        .await;
    conversation.push_user(clean_text.clone());

    // Fetch recent channel activity (last ~20 messages from ANYONE including
    // other bots like deadman) as GROUND TRUTH context. This is the fix for
    // the 2026-04-14 bug where flacoAi invented "the team fixed the API
    // gateway" while a deadman CRITICAL alert sat 2 messages above in the
    // same channel. With this fetch, those alerts are now visible to the
    // model AND to the vet layer.
    let channel_context = fetch_channel_context(http, bot_token, channel, 20, gateway.our_bot_id()).await;

    // Build the SYSTEM prompt: persona overlay + domain stanza + channel
    // activity + per-user history. Clean role separation — the LLM gets a
    // real system prompt instead of the v1 everything-as-one-user-message
    // pattern that weakened instruction-following.
    // Inject today's date at the top of every system prompt so models stop
    // quoting stale training-data dates as "future" events. Seen in a real
    // bad Yankees reply in #dad-help where a small model quoted
    // "April 13, 2026" as the next game — 2 days after it had already
    // happened — because its training data cutoff predated today.
    let today = chrono::Local::now().format("%A, %B %-d, %Y").to_string();
    let date_header = format!(
        "Current date: {today}. If you're about to quote a 'future' event \
         with a date before today, STOP — you are hallucinating from stale \
         training data. Say 'I need to check the current schedule' instead."
    );
    // When an agent is active, its prompt replaces the persona overlay.
    // The date header and all supporting context (domain, channel activity,
    // conversation history) are still injected — agents ADD to the system
    // prompt, they don't replace the supporting context.
    let persona_or_agent_prompt = if let Some(agent) = &active_agent {
        agent.prompt.clone()
    } else {
        channel_persona.prompt_overlay.clone()
    };
    let mut system_parts: Vec<String> = vec![
        date_header,
        persona_or_agent_prompt,
    ];
    if !domain_context.is_empty() {
        system_parts.push(domain_context);
    }
    if !channel_context.is_empty() {
        system_parts.push(format!(
            "## Recent channel activity in #{channel_name} (chronological, most recent LAST)\n\
These messages are GROUND TRUTH about current state. If a deadman alert \
is present in the last 15 minutes, treat it as authoritative — do NOT \
contradict it without fresh evidence from a tool you just ran.\n\n{channel_context}"
        ));
    }
    // Per-user conversation history (previous turns with this specific user)
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
            system_parts.push(format!(
                "## Previous turns with this user (oldest first)\n{}",
                history.join("\n")
            ));
        }
    }
    let mut system_prompt = system_parts.join("\n\n");

    // Web search grounding: if the message is about current events, sports,
    // news, or time-sensitive topics, fetch live results from DuckDuckGo and
    // inject them into the system prompt so the model can ground its answer
    // in fresh data instead of stale training-data dates.
    if let Some(query) = needs_web_search(&clean_text) {
        match web_search(http, &query).await {
            Ok(results) => {
                system_prompt.push_str(&format!(
                    "\n\nCurrent information from web search for '{query}':\n{results}\n\nUse the search results above to answer the question directly. Extract specific facts (dates, times, scores, names) from the results and present them confidently. Do NOT say you do not have information, do NOT tell the user to check a website, do NOT say you recommend visiting anything, do NOT say your training data is outdated, do NOT defer to external links when the answer is in the search results. If the search results contain the answer, STATE IT. If they genuinely do not contain the answer, say what you DID find."
                ));
                tracing::info!(target: "socket_mode", %query, "web search results injected into system prompt");
            }
            Err(e) => {
                tracing::warn!(target: "socket_mode", %query, error = %e, "web search failed, proceeding without");
            }
        }
    }

    // Call Ollama with the orchestrator-picked model
    let ollama_url = gateway.ollama_url().trim_end_matches("/v1");
    tracing::info!(target: "socket_mode", chosen_model = %chosen_model, persona = %channel_persona.channel, "orchestrator picked model");
    let first_try = call_ollama(
        http,
        ollama_url,
        &chosen_model,
        &system_prompt,
        &clean_text,
    )
    .await;

    // Auto-retry on empty content from thinking-models. gpt-oss:20b in
    // particular can spiral on questions where it's uncertain about tool
    // availability, burning num_predict on internal reasoning without
    // committing to a final answer. When that happens, fall back to the
    // small tier (nemotron-mini), which is non-thinking and always
    // commits to something — the vet layer will catch any hallucinations
    // on vet-enabled channels.
    let response = match first_try {
        Err(ref e) if e.starts_with("empty content from") => {
            let fallback = gateway
                .model_small()
                .unwrap_or("nemotron-mini")
                .to_string();
            tracing::warn!(
                target: "socket_mode",
                primary = %chosen_model,
                fallback = %fallback,
                reason = %e,
                "retrying with non-thinking fallback model after empty content"
            );
            call_ollama(http, ollama_url, &fallback, &system_prompt, &clean_text).await
        }
        _ => first_try,
    };

    // Determine the final text to post. For mission-critical channels, run
    // the response through the claude vet layer. For everything else, post
    // the local response directly.
    let final_text: String = match response {
        Ok(local_reply) => {
            if vet_enabled {
                match claude_check(http, &clean_text, &channel_context, &local_reply, &chosen_model, &channel_persona).await {
                    CheckResult::Approved => {
                        tracing::info!(target: "vet", "APPROVED local reply for #{channel_name}");
                        local_reply
                    }
                    CheckResult::Corrected(corrected) => {
                        tracing::warn!(
                            target: "vet",
                            "REJECTED+CORRECTED local reply for #{channel_name}"
                        );
                        format!(
                            "{corrected}\n\n_✓ vetted — flacoAi's original reply was corrected by claude_"
                        )
                    }
                    CheckResult::Unavailable(reason) => {
                        tracing::error!(target: "vet", "vet unavailable for #{channel_name}: {reason}");
                        format!("{local_reply}\n\n_⚠ unvetted — claude was unreachable ({reason})_")
                    }
                }
            } else {
                local_reply
            }
        }
        Err(e) => {
            // Detailed error goes to the log for operators. User-facing
            // message is family-friendly — never leak stack traces, model
            // names, or token counts into a Slack channel where Walter
            // might read them.
            tracing::error!(
                target: "socket_mode",
                channel = %channel_name,
                persona = %channel_persona.channel,
                "Ollama error after retry: {e}"
            );
            if channel_persona.channel == "slack-walter" {
                "Hmm, I hit a snag on that one, Dad — give me a minute and try again.".to_string()
            } else {
                "I hit a snag on that — mind trying again in a sec?".to_string()
            }
        }
    };

    // Persist the conversation turn with the final (possibly vetted) reply
    conversation.push_assistant(final_text.clone());
    gateway
        .update_conversation("slack", user, conversation)
        .await;

    // Post the final text, updating the thinking placeholder in place
    // for the first chunk and sending overflow as new messages.
    let parts = split_message(&final_text, 3900);
    if let Some(first) = parts.first() {
        if let Some(ts) = &thinking_ts {
            update_message(http, bot_token, channel, ts, first).await;
        } else {
            send_channel_message(http, bot_token, channel, first).await;
        }
    }
    for part in parts.iter().skip(1) {
        send_channel_message(http, bot_token, channel, part).await;
    }
}

/// Post a "thinking..." placeholder and return its timestamp for later update.
/// The caller supplies the exact text so we can brand it as "flacoAi" (local
/// only) vs "flacoAi Pro" (local + claude vet layer) per channel.
async fn post_thinking_message(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    thinking_text: &str,
) -> Option<String> {
    let body = json!({
        "channel": channel,
        "text": thinking_text,
    });
    let resp: Value = http
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    if resp["ok"].as_bool() == Some(true) {
        resp["ts"].as_str().map(String::from)
    } else {
        None
    }
}

/// Update an existing message in place.
async fn update_message(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    ts: &str,
    text: &str,
) {
    let body = json!({
        "channel": channel,
        "ts": ts,
        "text": text,
    });
    let _ = http
        .post("https://slack.com/api/chat.update")
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await;
}

/// Bulk-delete our own (flacoAi bot) messages from a channel. Fetches
/// the last `limit` messages via conversations.history, filters to messages
/// where `bot_id` matches the caller-supplied `our_bot_id`, and calls
/// chat.delete on each.
///
/// Returns (deleted_count, error_count). Slack rate-limits chat.delete at
/// around 50/min for Tier 3; we sleep 250ms between deletes to stay polite
/// and avoid 429s on runs of 200+ messages.
///
/// Cannot delete human messages — bots only have `chat:write` for their
/// own output. For a full channel wipe, the user must delete their messages
/// manually or use admin scopes (not wired here).
async fn purge_own_messages(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    limit: usize,
    our_bot_id: &str,
) -> (usize, usize) {
    let mut deleted = 0usize;
    let mut errors = 0usize;

    if our_bot_id.is_empty() {
        tracing::warn!(target: "socket_mode", "purge_own_messages called with empty our_bot_id — refusing to run");
        return (0, 1);
    }

    let history: Value = match http
        .get("https://slack.com/api/conversations.history")
        .bearer_auth(bot_token)
        .query(&[("channel", channel), ("limit", &limit.to_string())])
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(_) => return (0, 1),
        },
        Err(_) => return (0, 1),
    };

    if history["ok"].as_bool() != Some(true) {
        return (0, 1);
    }

    let messages = match history["messages"].as_array() {
        Some(m) => m.clone(),
        None => return (0, 0),
    };

    for m in messages {
        // Only delete OUR own bot messages
        if m["bot_id"].as_str() != Some(our_bot_id) {
            continue;
        }
        let Some(ts) = m["ts"].as_str() else {
            continue;
        };

        let del_body = json!({"channel": channel, "ts": ts});
        let resp = http
            .post("https://slack.com/api/chat.delete")
            .bearer_auth(bot_token)
            .json(&del_body)
            .send()
            .await;
        match resp {
            Ok(r) => {
                if let Ok(j) = r.json::<Value>().await {
                    if j["ok"].as_bool() == Some(true) {
                        deleted += 1;
                    } else {
                        errors += 1;
                    }
                } else {
                    errors += 1;
                }
            }
            Err(_) => errors += 1,
        }
        // Rate-limit politeness: 250ms between deletes (~4/sec, 240/min)
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    (deleted, errors)
}

/// Send an ephemeral message visible only to the specified user.
/// Doesn't appear in conversations.history, doesn't clutter the channel,
/// disappears when the user navigates away. Use for "I did the thing"
/// confirmations after destructive commands like /purge and /wipe so the
/// user sees the receipt without re-cluttering the channel they just cleaned.
async fn send_ephemeral_message(
    http: &reqwest::Client,
    bot_token: &str,
    channel: &str,
    user: &str,
    text: &str,
) {
    let body = json!({"channel": channel, "user": user, "text": text});
    let _ = http
        .post("https://slack.com/api/chat.postEphemeral")
        .bearer_auth(bot_token)
        .json(&body)
        .send()
        .await;
}

/// Send a new top-level message to the channel (not a thread reply).
async fn send_channel_message(http: &reqwest::Client, bot_token: &str, channel: &str, text: &str) {
    let body = json!({"channel": channel, "text": text});
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

// =====================================================================
// Channel name cache + lookup
// =====================================================================

fn channel_name_cache() -> &'static tokio::sync::RwLock<HashMap<String, String>> {
    static CACHE: std::sync::OnceLock<tokio::sync::RwLock<HashMap<String, String>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::RwLock::new(HashMap::new()))
}

/// Resolve a Slack channel ID to its human name ("home-general", "infra-alerts",
/// etc.) by calling conversations.info. Cached for the process lifetime —
/// channel renames are rare and a restart refreshes the cache.
async fn fetch_channel_name(
    http: &reqwest::Client,
    bot_token: &str,
    channel_id: &str,
) -> Option<String> {
    {
        let cache = channel_name_cache().read().await;
        if let Some(name) = cache.get(channel_id) {
            return Some(name.clone());
        }
    }

    let resp: Value = http
        .get("https://slack.com/api/conversations.info")
        .bearer_auth(bot_token)
        .query(&[("channel", channel_id)])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    if resp["ok"].as_bool() != Some(true) {
        tracing::warn!(
            "conversations.info failed for {channel_id}: {}",
            resp["error"].as_str().unwrap_or("unknown")
        );
        return None;
    }

    let name = resp["channel"]["name"].as_str()?.to_string();
    channel_name_cache()
        .write()
        .await
        .insert(channel_id.to_string(), name.clone());
    Some(name)
}

// =====================================================================
// Channel context fetch (for vet layer + model grounding)
// =====================================================================

/// Fetch the last `limit` messages from a channel via conversations.history.
/// Includes bot messages (deadman alerts, netguardian alerts, etc.) because
/// that's the entire point — these are GROUND TRUTH for the model and the vet
/// layer. Excludes our OWN bot id so we don't loop on our own replies.
///
/// Returns a formatted string ready to inject into a system prompt, e.g.:
///
///     [13:51 UTC] deadman: 🚨 CRITICAL — Pi unreachable on Tailscale
///     [13:52 UTC] elGordo: are we back online?
///     [13:53 UTC] flaco (prev reply): checking...
///
/// Empty string if the channel has no recent activity or the fetch fails —
/// the caller just skips injecting channel context in that case.
async fn fetch_channel_context(
    http: &reqwest::Client,
    bot_token: &str,
    channel_id: &str,
    limit: usize,
    our_bot_id: &str,
) -> String {
    let resp: Result<Value, _> = async {
        http.get("https://slack.com/api/conversations.history")
            .bearer_auth(bot_token)
            .query(&[
                ("channel", channel_id),
                ("limit", &limit.to_string()),
                ("inclusive", "true"),
            ])
            .send()
            .await?
            .json::<Value>()
            .await
    }
    .await;

    let Ok(resp) = resp else {
        tracing::warn!("conversations.history network error for {channel_id}");
        return String::new();
    };

    if resp["ok"].as_bool() != Some(true) {
        tracing::warn!(
            "conversations.history failed for {channel_id}: {}",
            resp["error"].as_str().unwrap_or("unknown")
        );
        return String::new();
    }

    let messages = match resp["messages"].as_array() {
        Some(m) => m,
        None => return String::new(),
    };

    let mut lines: Vec<String> = Vec::new();
    // messages are newest-first; reverse so they read chronologically
    for m in messages.iter().rev() {
        // Skip our own bot's past replies (they're already in conversation state)
        if !our_bot_id.is_empty() && m["bot_id"].as_str() == Some(our_bot_id) {
            continue;
        }

        let text = m["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }

        // Unix epoch timestamp → HH:MM UTC label
        let ts_label = m["ts"]
            .as_str()
            .and_then(|t| t.split('.').next())
            .and_then(|s| s.parse::<i64>().ok())
            .map(|epoch| {
                let hh = (epoch % 86400) / 3600;
                let mm = (epoch % 3600) / 60;
                format!("[{:02}:{:02} UTC]", hh, mm)
            })
            .unwrap_or_else(|| "[--:-- UTC]".to_string());

        let author = if let Some(name) = m["username"].as_str() {
            name.to_string()
        } else if m["bot_id"].is_string() {
            // Other bots (deadman, netguardian, walter). Use bot_profile.name
            // if present, otherwise a generic label.
            m["bot_profile"]["name"]
                .as_str()
                .unwrap_or("bot")
                .to_string()
        } else if let Some(user_id) = m["user"].as_str() {
            // A human we don't have a display name for. The user id is
            // unique and fine for the LLM's purposes.
            format!("user:{user_id}")
        } else {
            "unknown".to_string()
        };

        // Truncate long messages to keep context tight
        let clipped = if text.len() > 500 {
            format!("{}... (truncated)", &text[..500])
        } else {
            text.to_string()
        };

        lines.push(format!("{ts_label} {author}: {clipped}"));
    }

    lines.join("\n")
}
