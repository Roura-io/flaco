//! flaco-core — shared runtime for flacoAi v2.
//!
//! One brain, one memory, one tool registry, shared by Slack / TUI / Web.

pub mod error;
pub mod ollama;
pub mod memory;
pub mod persona;
pub mod session;
pub mod tools;
pub mod runtime;
pub mod features;
pub mod intent;
pub mod welcome;

pub use error::{Error, Result};
pub use memory::{Memory, Conversation, Message, Role, Fact};
pub use ollama::{OllamaClient, ChatRequest, ChatMessage, ChatResponse, ToolCall};
pub use persona::{Persona, PersonaRegistry};
pub use session::Session;
pub use tools::{Tool, ToolRegistry, ToolResult, ToolSchema};
pub use runtime::{Runtime, RuntimeConfig, Surface, Event};
