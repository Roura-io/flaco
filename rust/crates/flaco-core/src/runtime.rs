//! Runtime: the glue that binds Session + Ollama + ToolRegistry into a single
//! `handle` method each surface can call.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::Result;
use crate::memory::{Memory, Role};
use crate::ollama::{ChatMessage, OllamaClient};
use crate::persona::PersonaRegistry;
use crate::session::Session;
use crate::tools::ToolRegistry;

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
        session.append_user(user_text)?;

        let mut round = 0usize;
        loop {
            round += 1;
            let history = self.memory.recent_messages(&session.conversation.id, 30)?;
            let messages: Vec<ChatMessage> = history
                .iter()
                .map(|m| match m.role {
                    Role::System => ChatMessage::system(&m.content),
                    Role::User => ChatMessage::user(&m.content),
                    Role::Assistant => ChatMessage::assistant(&m.content),
                    Role::Tool => ChatMessage {
                        role: "tool".into(),
                        content: m.content.clone(),
                        tool_calls: vec![],
                        tool_name: m.tool_name.clone(),
                    },
                })
                .collect();

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
                for call in &msg.tool_calls {
                    let name = call.function.name.clone();
                    let args = call.function.arguments.clone();
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
                    if let Some(tx) = &tx {
                        let _ = tx.send(Event::ToolResult {
                            name: name.clone(),
                            output: result.output.clone(),
                            ok: result.ok,
                        });
                    }
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
