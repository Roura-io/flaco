//! Web research with numbered citations — flacoAi's answer engine.
//!
//! Flow:
//!   1. Web search (DDG) for the topic → top N hits
//!   2. Fetch the top M pages in parallel → plain-text bodies
//!   3. Ask Ollama to write a short answer, instructing it to cite sources
//!      with [N] markers matching hit order
//!   4. Return Markdown with numbered references at the bottom
//!
//! Output is deterministic enough for the web UI to render with clickable
//! citations.

use async_trait::async_trait;
use futures_util::future::join_all;
use serde_json::Value;

use crate::error::Result;
use crate::ollama::{ChatMessage, OllamaClient};
use super::web::{SearchHit, WebFetch, WebSearch};
use super::{Tool, ToolResult, ToolSchema};

pub struct Research {
    pub search: WebSearch,
    pub fetch: WebFetch,
    pub ollama: OllamaClient,
}

impl Research {
    pub fn new(ollama: OllamaClient) -> Self {
        Self { search: WebSearch::new(), fetch: WebFetch::new(), ollama }
    }

    pub async fn run(&self, topic: &str, depth: usize) -> Result<ResearchResult> {
        let hits = self.search.search(topic, depth.max(3).min(8)).await?;
        if hits.is_empty() {
            return Ok(ResearchResult {
                answer: format!("No search results for '{topic}'."),
                citations: vec![],
            });
        }
        let to_fetch: Vec<SearchHit> = hits.iter().take(4).cloned().collect();
        let bodies: Vec<(SearchHit, String)> = join_all(to_fetch.into_iter().map(|h| {
            let fetch = &self.fetch;
            async move {
                let body = fetch.fetch_readable(&h.url).await.unwrap_or_default();
                (h, body)
            }
        }))
        .await;

        let mut context = String::new();
        for (i, (hit, body)) in bodies.iter().enumerate() {
            let n = i + 1;
            context.push_str(&format!("SOURCE [{n}] {}\nURL: {}\n---\n", hit.title, hit.url));
            let trimmed = body.chars().take(3500).collect::<String>();
            context.push_str(&trimmed);
            context.push_str("\n\n");
        }

        let system = ChatMessage::system(
            "You are a research assistant. Answer the user's question using ONLY the sources provided.
Every factual claim must end with a citation in the form [1], [2], etc., matching the SOURCE numbers.
Keep the answer under 250 words. Markdown allowed. Do not invent sources.",
        );
        let user_msg = ChatMessage::user(format!(
            "Question: {topic}\n\nSources:\n{context}\n\nWrite the answer now."
        ));

        let resp = self
            .ollama
            .chat(vec![system, user_msg], vec![])
            .await?;
        let answer = resp.message.content;

        let citations = bodies
            .iter()
            .enumerate()
            .map(|(i, (h, _))| Citation {
                index: i + 1,
                title: h.title.clone(),
                url: h.url.clone(),
                snippet: h.snippet.clone(),
            })
            .collect();

        Ok(ResearchResult { answer, citations })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResearchResult {
    pub answer: String,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Citation {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl ResearchResult {
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.answer);
        s.push_str("\n\n---\n**Sources**\n");
        for c in &self.citations {
            s.push_str(&format!("{}. [{}]({})\n", c.index, c.title, c.url));
        }
        s
    }
}

#[async_trait]
impl Tool for Research {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "research".into(),
            description: "Web research: web search → fetch top results → summarize with numbered citations.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "topic":{"type":"string"},
                    "depth":{"type":"integer","description":"how many search results to consider, default 5"}
                },
                "required":["topic"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let topic = args.get("topic").and_then(Value::as_str).unwrap_or("").trim();
        if topic.is_empty() { return Ok(ToolResult::err("topic required")); }
        let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(5) as usize;
        let res = self.run(topic, depth).await?;
        let md = res.to_markdown();
        Ok(ToolResult::ok_text(md).with_structured(serde_json::to_value(&res)?))
    }
}
