//! Direct (non-model) entry points for the 4 killer features, so surfaces
//! can wire slash commands straight to the logic without going through the
//! full LLM tool-calling loop when the user has already been explicit.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use crate::error::Result;
use crate::memory::Memory;
use crate::ollama::OllamaClient;
use crate::tools::research::{Research, ResearchResult};
use crate::tools::scaffold::Scaffold;
use crate::tools::shortcut::CreateShortcut;
use crate::tools::jira::JiraClient;
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
}

#[allow(dead_code)]
fn _force_arc_import() {
    let _: Option<Arc<u8>> = None;
}
