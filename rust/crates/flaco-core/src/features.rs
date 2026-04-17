//! Direct (non-model) entry points for the 4 killer features, so surfaces
//! can wire slash commands straight to the logic without going through the
//! full LLM tool-calling loop when the user has already been explicit.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use crate::error::Result;
use crate::memory::Memory;
use crate::ollama::{ChatMessage, OllamaClient};
use crate::tools::jira::{JiraClient, JiraIssueSummary};
use crate::tools::research::{Research, ResearchResult};
use crate::tools::scaffold::Scaffold;
use crate::tools::shortcut::CreateShortcut;
use crate::tools::{Tool, ToolResult};

#[derive(Clone)]
pub struct Features {
    pub memory: Memory,
    pub ollama: OllamaClient,
    pub jira: Option<JiraClient>,
    pub shortcut_out_dir: PathBuf,
}

impl Features {
    pub fn new(memory: Memory, ollama: OllamaClient) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        Self {
            memory,
            ollama,
            jira: JiraClient::from_env(),
            shortcut_out_dir: home.join("Downloads/flaco-shortcuts"),
        }
    }

    pub async fn research(&self, topic: &str) -> Result<ResearchResult> {
        let r = Research::new(self.ollama.clone());
        r.run(topic, 5).await
    }

    pub async fn create_shortcut(&self, name: &str, description: &str) -> Result<ToolResult> {
        let t = CreateShortcut::new(&self.shortcut_out_dir);
        t.call(json!({"name": name, "description": description})).await
    }

    pub async fn scaffold(
        &self,
        idea: &str,
        project_key: &str,
        target_dir: Option<&str>,
    ) -> Result<ToolResult> {
        let t = Scaffold { jira: self.jira.clone() };
        let mut args = json!({"idea": idea, "project_key": project_key});
        if let Some(dir) = target_dir {
            args.as_object_mut().unwrap().insert("target_dir".into(), json!(dir));
        }
        t.call(args).await
    }

    pub fn remember(&self, user: &str, content: &str, kind: &str) -> Result<i64> {
        self.memory.remember_fact(user, kind, content, None)
    }

    /// Morning Brief — the fifth killer feature.
    ///
    /// Pulls open Jira tickets assigned to the user (if Jira is configured),
    /// their most recent `facts` from memory, and asks the local model to
    /// synthesize a short, opinionated "here's your day" summary. Returns a
    /// structured `MorningBrief` so each surface can render it how it likes.
    pub async fn morning_brief(&self, user: &str) -> Result<MorningBrief> {
        // 1. memories — take the 10 most recently saved facts.
        let facts = self.memory.all_facts(user, 10)?;
        let memory_block = if facts.is_empty() {
            "(no memories yet)".to_string()
        } else {
            facts
                .iter()
                .map(|f| format!("- [{}] {}", f.kind, truncate(&f.content, 240)))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // 2. Jira — open issues assigned to currentUser().
        let issues: Vec<JiraIssueSummary> = match &self.jira {
            Some(j) => j.my_open_issues(15).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let jira_block = if issues.is_empty() {
            if self.jira.is_some() {
                "(no open tickets assigned right now)".to_string()
            } else {
                "(Jira not configured)".to_string()
            }
        } else {
            issues
                .iter()
                .map(|i| {
                    let pri = i.priority.as_deref().unwrap_or("—");
                    format!("- {} [{}] ({}, {}): {}", i.key, i.kind, i.status, pri, truncate(&i.summary, 160))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        // 3. synthesize via Ollama.
        let system = "You are flacoAi, a no-nonsense personal ops assistant for Chris at Roura.io. \
                      Given facts you already know about him and his open Jira tickets, write a \
                      focused Morning Brief. Be concrete, honest, and short. Do not invent tickets. \
                      Output Markdown with three sections exactly: \
                      '## Focus today' (1 short paragraph, your best call on what matters most given \
                      priorities and ticket types), \
                      '## On your plate' (bulleted list referencing ticket keys verbatim), \
                      '## Heads up' (2-4 bullets mixing memory facts and any risks you see). \
                      Keep the whole thing under 220 words.";
        let user_prompt = format!(
            "User: {user}\n\nWhat I know about {user} (from shared memory):\n{memory_block}\n\nOpen Jira tickets assigned to {user}:\n{jira_block}\n\nWrite today's Morning Brief."
        );
        let messages = vec![
            ChatMessage::system(system),
            ChatMessage::user(user_prompt),
        ];
        let resp = self.ollama.chat(messages, vec![]).await?;
        let markdown = resp.message.content.trim().to_string();

        Ok(MorningBrief {
            markdown,
            issues,
            fact_count: facts.len(),
        })
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MorningBrief {
    pub markdown: String,
    pub issues: Vec<JiraIssueSummary>,
    pub fact_count: usize,
}

#[allow(dead_code)]
fn _force_arc_import() {
    let _: Option<Arc<u8>> = None;
}
