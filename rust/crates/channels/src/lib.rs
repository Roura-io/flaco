//! Channel system for flacoAi — Gateway dispatcher + Slack channel.
//!
//! Enables flacoAi to communicate through external channels (Slack, HTTP API)
//! with human-like conversational responses and per-sender conversation state.

#![allow(
    clippy::unnecessary_literal_bound,
    clippy::doc_markdown,
    clippy::double_must_use
)]

pub mod agents;
pub mod commands;
pub mod domain;
pub mod frontmatter;
pub mod gateway;
pub mod rules;
pub mod skills;
pub mod slack;
pub mod inference;
pub mod socket_mode;

pub use agents::{
    agent_for_channel, agent_for_mention, agent_for_slash_command, load_agents_from_dir, Agent,
    AgentTrigger,
};
pub use commands::{command_for_name, load_commands_from_dir, Command};
pub use gateway::{ChannelPersona, ConversationState, Gateway, GatewayConfig, IncomingMessage};
pub use rules::{load_rules_from_dir, rules_for_language, rules_for_path, Rule};
pub use skills::{load_skills_from_dir, skills_for_message, Skill};
pub use inference::{call_ollama, claude_check, CheckResult, needs_web_search, web_search};
pub use slack::SlackChannel;
