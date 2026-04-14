//! Runtime: the glue that binds Session + Ollama + ToolRegistry into a single
//! `handle` method each surface can call.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::Result;
use crate::memory::{Memory, Role};
use crate::ollama::{ChatMessage, OllamaClient};
use crate::persona::PersonaRegistry;
use crate::session::Session;
use crate::tools::ToolRegistry;

/// Normalize a tool-call invocation into a deduplication key. JSON keys are
/// sorted so `{"a":1,"b":2}` and `{"b":2,"a":1}` produce the same key.
pub fn dedup_key(tool_name: &str, args: &serde_json::Value) -> String {
    fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut sorted: std::collections::BTreeMap<String, serde_json::Value> =
                    std::collections::BTreeMap::new();
                for (k, val) in map {
                    sorted.insert(k.clone(), canonicalize(val));
                }
                serde_json::to_value(sorted).unwrap_or(serde_json::Value::Null)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(canonicalize).collect())
            }
            other => other.clone(),
        }
    }
    let canon = canonicalize(args);
    format!("{tool_name}::{}", serde_json::to_string(&canon).unwrap_or_default())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Surface {
    Slack,
    Tui,
    Web,
    Cli,
}

impl Surface {
    pub fn as_str(&self) -> &'static str {
        match self {
            Surface::Slack => "slack",
            Surface::Tui => "tui",
            Surface::Web => "web",
            Surface::Cli => "cli",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub enum Event {
    TextChunk(String),
    ToolCall { name: String, args: serde_json::Value },
    ToolResult { name: String, output: String, ok: bool },
    Done { full_text: String },
    Error(String),
}

#[derive(Clone)]
pub struct RuntimeConfig {
    pub max_tool_rounds: usize,
    pub persona_default: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self { max_tool_rounds: 4, persona_default: "default".into() }
    }
}

#[derive(Clone)]
pub struct Runtime {
    pub memory: Memory,
    pub ollama: OllamaClient,
    pub tools: Arc<ToolRegistry>,
    pub personas: PersonaRegistry,
    pub config: RuntimeConfig,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish()
    }
}

impl Runtime {
    pub fn new(
        memory: Memory,
        ollama: OllamaClient,
        tools: ToolRegistry,
        personas: PersonaRegistry,
    ) -> Self {
        Self {
            memory,
            ollama,
            tools: Arc::new(tools),
            personas,
            config: RuntimeConfig::default(),
        }
    }

    /// Start or resume a session for the given surface/user.
    pub fn session(&self, surface: &Surface, user_id: &str) -> Result<Session> {
        Session::resume_or_start(
            self.memory.clone(),
            &self.personas,
            surface.as_str(),
            user_id,
            Some(&self.config.persona_default),
        )
    }

    /// Generate a short title from the first user message and store it on
    /// the conversation. Non-fatal on failure — titling is cosmetic.
    pub async fn auto_title(&self, session: &Session, first_message: &str) {
        if session.conversation.title.is_some() { return; }
        let prompt = format!(
            "Write a punchy 3-6 word title (no quotes, no period) for this conversation opener: \"{first_message}\""
        );
        let messages = vec![
            ChatMessage::system("You write very short, vivid conversation titles. Reply with just the title, nothing else."),
            ChatMessage::user(prompt),
        ];
        if let Ok(resp) = self.ollama.chat(messages, vec![]).await {
            let title = resp
                .message
                .content
                .trim()
                .trim_matches('"')
                .trim()
                .to_string();
            if !title.is_empty() && title.len() < 120 {
                let _ = self.memory.set_title(&session.conversation.id, &title);
            }
        }
    }

    /// Run a single turn: append the user message, let the model think (with
    /// tool-calling loop up to `max_tool_rounds`), and return the final
    /// assistant text. Events are pushed into `tx` for surfaces that want
    /// live updates.
    pub async fn handle_turn(
        &self,
        session: &Session,
        user_text: &str,
        tx: Option<mpsc::UnboundedSender<Event>>,
    ) -> Result<String> {
        let first_message = self
            .memory
            .recent_messages(&session.conversation.id, 50)
            .map(|h| h.iter().all(|m| m.role != Role::User))
            .unwrap_or(true);

        // Domain context routing: classify the user message ONCE per turn
        // to pick the right system-prompt stanza, required env, and
        // source-of-truth files. If the domain needs env that isn't set,
        // refuse gracefully before burning an LLM call.
        let domain = crate::domain::classify_message(user_text);
        tracing::info!(domain = %domain, "classified turn domain");
        if let Err(preflight_msg) = crate::domain::preflight(domain) {
            tracing::warn!(domain = %domain, "preflight failed: {preflight_msg}");
            session.append_user(user_text)?;
            session.append_assistant(&preflight_msg)?;
            return Ok(preflight_msg);
        }
        // Build the transient system message once — we'll prepend it to
        // every LLM round inside the tool loop so the context survives
        // across tool calls without bloating the persisted history.
        let domain_context = crate::domain::build_context(domain);

        session.append_user(user_text)?;

        // Dedup state for this turn. Every (tool_name, canonicalized_args)
        // pair that has already been executed goes in here; repeats short-
        // circuit with a "already done" tool result so the model stops
        // calling them. This fixes the v1/v2 bug where qwen3 would fire
        // `remember` 6 times for the same fact.
        let mut seen_calls: HashSet<String> = HashSet::new();
        let mut all_duplicates_bailout = false;

        let mut round = 0usize;
        loop {
            round += 1;
            let history = self.memory.recent_messages(&session.conversation.id, 30)?;
            let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 1);
            // Transient system message: the domain-specific stanza +
            // auto-read source-of-truth files. Injected fresh every
            // round of the tool loop so the model doesn't "forget"
            // the domain context mid-turn, but NEVER persisted to
            // SQLite — which keeps the history clean and lets a
            // future turn in the same conversation pick a different
            // domain without the old stanza leaking in.
            if !domain_context.is_empty() {
                messages.push(ChatMessage::system(&domain_context));
            }
            messages.extend(history.iter().map(|m| match m.role {
                Role::System => ChatMessage::system(&m.content),
                Role::User => ChatMessage::user(&m.content),
                Role::Assistant => ChatMessage::assistant(&m.content),
                Role::Tool => ChatMessage {
                    role: "tool".into(),
                    content: m.content.clone(),
                    tool_calls: vec![],
                    tool_name: m.tool_name.clone(),
                },
            }));

            let schemas = self.tools.schemas();
            let resp = match self.ollama.chat(messages, schemas).await {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("(model error: {e})");
                    if let Some(tx) = &tx { let _ = tx.send(Event::Error(msg.clone())); }
                    session.append_assistant(&msg)?;
                    return Ok(msg);
                }
            };

            let msg = &resp.message;

            // If the model asked for tools, run them and continue.
            if !msg.tool_calls.is_empty() && round <= self.config.max_tool_rounds {
                let mut executed_any = 0usize;
                let mut duplicate_count = 0usize;
                for call in &msg.tool_calls {
                    let name = call.function.name.clone();
                    let args = call.function.arguments.clone();
                    let key = dedup_key(&name, &args);

                    // Short-circuit identical repeat calls with a tool
                    // result that nudges the model to finish. We still
                    // record the repeat to the conversation so the model
                    // sees it, but we never execute the underlying tool
                    // twice.
                    if seen_calls.contains(&key) {
                        duplicate_count += 1;
                        let warn = format!(
                            "[flaco] already executed {name} with these exact args in this turn — not running it again. Write your final reply now."
                        );
                        tracing::warn!(tool = %name, "skipping duplicate tool call");
                        session.append_tool_result(&name, &warn)?;
                        if let Some(tx) = &tx {
                            let _ = tx.send(Event::ToolResult {
                                name: name.clone(),
                                output: warn,
                                ok: false,
                            });
                        }
                        continue;
                    }
                    seen_calls.insert(key);

                    if let Some(tx) = &tx {
                        let _ = tx.send(Event::ToolCall { name: name.clone(), args: args.clone() });
                    }
                    let result = match self.tools.call(&name, args.clone()).await {
                        Ok(r) => r,
                        Err(e) => crate::tools::ToolResult::err(format!("tool {name} error: {e}")),
                    };
                    let args_str = args.to_string();
                    let result_json = serde_json::to_string(&result).unwrap_or_default();
                    let _ = self
                        .memory
                        .record_tool_call(&session.conversation.id, &name, &args_str, &result_json);
                    session.append_tool_result(&name, &result.output)?;
                    executed_any += 1;
                    if let Some(tx) = &tx {
                        let _ = tx.send(Event::ToolResult {
                            name: name.clone(),
                            output: result.output.clone(),
                            ok: result.ok,
                        });
                    }
                }

                // If this round was nothing but duplicates, the model is
                // stuck in a loop. Force it to produce a text response by
                // bailing out of the tool loop on the next iteration.
                if executed_any == 0 && duplicate_count > 0 {
                    tracing::warn!(
                        duplicates = duplicate_count,
                        "tool loop had only duplicate calls — forcing text response"
                    );
                    all_duplicates_bailout = true;
                    // Loop one more time; the tool results we just appended
                    // tell the model to stop, and the next response should
                    // be text. If it's still tool calls, we break below.
                }
                if all_duplicates_bailout && round > 1 {
                    // We've already warned the model; if it's STILL trying
                    // to call tools on the next round, synthesize a final
                    // reply from what we have and return.
                    let fallback = if msg.content.trim().is_empty() {
                        "Done.".to_string()
                    } else {
                        msg.content.clone()
                    };
                    session.append_assistant(&fallback)?;
                    if let Some(tx) = &tx {
                        let _ = tx.send(Event::Done { full_text: fallback.clone() });
                    }
                    return Ok(fallback);
                }
                continue;
            }

            // Final assistant message.
            let text = msg.content.clone();
            session.append_assistant(&text)?;
            if let Some(tx) = &tx {
                let _ = tx.send(Event::TextChunk(text.clone()));
                let _ = tx.send(Event::Done { full_text: text.clone() });
            }
            if first_message {
                // Fire-and-forget a title generation; we've already saved the
                // turn, so failure is harmless.
                let this = self.clone();
                let session = session.clone();
                let first = user_text.to_string();
                tokio::spawn(async move {
                    this.auto_title(&session, &first).await;
                });
            }
            return Ok(text);
        }
    }
}
