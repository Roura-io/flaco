//! A Session is a single active conversation — a thin wrapper around
//! Memory that caches the current conversation id and persona.

use crate::error::Result;
use crate::memory::{Conversation, Memory, Role};
use crate::persona::{Persona, PersonaRegistry};

#[derive(Clone, Debug)]
pub struct Session {
    pub memory: Memory,
    pub conversation: Conversation,
    pub persona_name: String,
    pub surface: String,
    pub user_id: String,
}

impl Session {
    pub fn start(
        memory: Memory,
        personas: &PersonaRegistry,
        surface: &str,
        user_id: &str,
        persona_hint: Option<&str>,
    ) -> Result<Self> {
        let persona: &Persona = match persona_hint {
            Some(name) => personas.get(name),
            None => personas.get("default"),
        };
        let conv = memory.create_conversation(surface, user_id, &persona.name)?;
        memory.append_message(&conv.id, Role::System, &persona.system_prompt, None)?;
        Ok(Self {
            memory,
            conversation: conv,
            persona_name: persona.name.clone(),
            surface: surface.into(),
            user_id: user_id.into(),
        })
    }

    /// Resume the most recent conversation for this (surface, user_id),
    /// or start a new one if none exists.
    pub fn resume_or_start(
        memory: Memory,
        personas: &PersonaRegistry,
        surface: &str,
        user_id: &str,
        persona_hint: Option<&str>,
    ) -> Result<Self> {
        if let Some(conv) = memory.latest_conversation_for(surface, user_id)? {
            return Ok(Self {
                persona_name: conv.persona.clone(),
                surface: surface.into(),
                user_id: user_id.into(),
                conversation: conv,
                memory,
            });
        }
        Self::start(memory, personas, surface, user_id, persona_hint)
    }

    pub fn append_user(&self, content: &str) -> Result<()> {
        self.memory.append_message(&self.conversation.id, Role::User, content, None)?;
        Ok(())
    }

    pub fn append_assistant(&self, content: &str) -> Result<()> {
        self.memory.append_message(&self.conversation.id, Role::Assistant, content, None)?;
        Ok(())
    }

    pub fn append_tool_result(&self, tool: &str, content: &str) -> Result<()> {
        self.memory
            .append_message(&self.conversation.id, Role::Tool, content, Some(tool))?;
        Ok(())
    }
}
