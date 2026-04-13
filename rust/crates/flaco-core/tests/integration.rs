//! Integration tests that exercise flaco-core end to end without touching
//! the network. We spin up an in-memory SQLite, build a runtime with an
//! echo-only tool, and verify session persistence, memory recall, and
//! shortcut generation.

use std::sync::Arc;

use async_trait::async_trait;
use flaco_core::memory::{Memory, Role};
use flaco_core::persona::PersonaRegistry;
use flaco_core::runtime::{Runtime, Surface};
use flaco_core::session::Session;
use flaco_core::tools::research::ResearchResult;
use flaco_core::tools::{Tool, ToolRegistry, ToolResult, ToolSchema};
use serde_json::Value;

struct EchoTool;
#[async_trait]
impl Tool for EchoTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".into(),
            description: "echo".into(),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
        }
    }
    async fn call(&self, args: Value) -> flaco_core::Result<ToolResult> {
        Ok(ToolResult::ok_text(
            args.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        ))
    }
}

#[test]
fn unified_memory_is_shared_across_surfaces() {
    let memory = Memory::open_in_memory().unwrap();
    // Slack session writes a fact…
    memory
        .remember_fact("chris", "preference", "loves Rust", None)
        .unwrap();
    memory
        .remember_fact("chris", "fact", "yankees fan", None)
        .unwrap();
    // …and both the web surface and the TUI surface see it.
    let hits = memory.search_facts("chris", "yankees", 10).unwrap();
    assert_eq!(hits.len(), 1);

    let hits = memory.search_facts("chris", "Rust", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].content.contains("Rust"));
}

#[test]
fn session_resume_finds_latest() {
    let memory = Memory::open_in_memory().unwrap();
    let personas = PersonaRegistry::defaults();
    let s1 = Session::start(memory.clone(), &personas, "web", "chris", None).unwrap();
    memory
        .append_message(&s1.conversation.id, Role::User, "hi", None)
        .unwrap();

    let resumed =
        Session::resume_or_start(memory.clone(), &personas, "web", "chris", None).unwrap();
    assert_eq!(resumed.conversation.id, s1.conversation.id);
}

#[test]
fn tool_registry_lists_and_routes() {
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(EchoTool));
    let names = reg.names();
    assert!(names.contains(&"echo".to_string()));
    let schemas = reg.schemas();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0]["function"]["name"], "echo");
}

#[tokio::test]
async fn research_result_renders_markdown_with_citations() {
    let res = ResearchResult {
        answer: "The answer is **42** [1] because reasons [2].".into(),
        citations: vec![
            flaco_core::tools::research::Citation {
                index: 1,
                title: "The Hitchhiker's Guide".into(),
                url: "https://example.com/1".into(),
                snippet: "snippet one".into(),
            },
            flaco_core::tools::research::Citation {
                index: 2,
                title: "Reasons".into(),
                url: "https://example.com/2".into(),
                snippet: "snippet two".into(),
            },
        ],
    };
    let md = res.to_markdown();
    assert!(md.contains("[1]"));
    assert!(md.contains("1. [The Hitchhiker's Guide](https://example.com/1)"));
    assert!(md.contains("2. [Reasons](https://example.com/2)"));
}

#[test]
fn persona_routing_by_channel() {
    let reg = PersonaRegistry::defaults();
    assert_eq!(reg.route_for_channel("dad-help").name, "walter");
    assert_eq!(reg.route_for_channel("dev-reviews").name, "dev");
    assert_eq!(reg.route_for_channel("flaco-general").name, "default");
}

#[tokio::test]
async fn shortcut_tool_writes_plist_to_disk() {
    use flaco_core::tools::shortcut::CreateShortcut;
    let dir = tempfile::tempdir().unwrap();
    let tool = CreateShortcut::new(dir.path());
    let result = tool
        .call(serde_json::json!({
            "name": "Hello Chris",
            "description": "Speak good morning Chris and open https://www.mlb.com/yankees",
        }))
        .await
        .unwrap();
    assert!(result.ok, "{}", result.output);
    let path = dir.path().join("Hello_Chris.shortcut");
    assert!(path.exists());
    let body = std::fs::read_to_string(path).unwrap();
    // Sanity: the plist should contain the identifiers we expect.
    assert!(body.contains("WFWorkflowActions"));
    assert!(body.contains("openurl") || body.contains("speaktext"));
}

#[test]
fn runtime_smoke_wires_components() {
    // Build a runtime with a no-op Ollama client — we're not going to call
    // the network, just verify construction and session creation work.
    let memory = Memory::open_in_memory().unwrap();
    let personas = PersonaRegistry::defaults();
    let mut reg = ToolRegistry::new();
    reg.register(Arc::new(EchoTool));
    let ollama = flaco_core::ollama::OllamaClient::new("http://127.0.0.1:1", "qwen3:32b-q8_0");
    let runtime = Runtime::new(memory, ollama, reg, personas);

    let session = runtime.session(&Surface::Web, "chris").unwrap();
    assert_eq!(session.surface, "web");
    assert_eq!(session.user_id, "chris");
}
