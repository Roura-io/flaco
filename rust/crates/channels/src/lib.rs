//! Channel system for flacoAi — Gateway dispatcher + Slack channel.
//!
//! Enables flacoAi to communicate through external channels (Slack, HTTP API)
//! with human-like conversational responses and per-sender conversation state.

#![allow(
    clippy::unnecessary_literal_bound,
    clippy::doc_markdown,
    clippy::double_must_use
)]

pub mod gateway;
pub mod slack;

pub use gateway::{ChannelPersona, ConversationState, Gateway, GatewayConfig, IncomingMessage};
pub use slack::SlackChannel;
