//! Personas: a persona is a named system prompt plus routing metadata.
//! The default persona is staff-engineer-ish. The `walter` persona is the
//! Alzheimer's-friendly companion.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Persona {
    pub name: String,
    pub display_name: String,
    pub system_prompt: String,
}

#[derive(Clone, Debug)]
pub struct PersonaRegistry {
    personas: HashMap<String, Persona>,
    default: String,
}

impl PersonaRegistry {
    pub fn defaults() -> Self {
        let default = Persona {
            name: "default".into(),
            display_name: "flaco".into(),
            system_prompt: DEFAULT_SYSTEM.into(),
        };
        let walter = Persona {
            name: "walter".into(),
            display_name: "flaco (for dad)".into(),
            system_prompt: WALTER_SYSTEM.into(),
        };
        let dev = Persona {
            name: "dev".into(),
            display_name: "flaco (dev mode)".into(),
            system_prompt: DEV_SYSTEM.into(),
        };
        let mut personas = HashMap::new();
        personas.insert(default.name.clone(), default);
        personas.insert(walter.name.clone(), walter);
        personas.insert(dev.name.clone(), dev);
        Self { personas, default: "default".into() }
    }

    pub fn get(&self, name: &str) -> &Persona {
        self.personas
            .get(name)
            .unwrap_or_else(|| self.personas.get(&self.default).expect("default persona"))
    }

    pub fn names(&self) -> Vec<&str> {
        self.personas.keys().map(String::as_str).collect()
    }

    /// Route by Slack channel name (cheap heuristic used in v1).
    pub fn route_for_channel(&self, channel: &str) -> &Persona {
        if channel == "dad-help" || channel.starts_with("dad-") {
            self.get("walter")
        } else if channel.starts_with("dev-") || channel == "dev-planning" {
            self.get("dev")
        } else {
            self.get("default")
        }
    }
}

const DEFAULT_SYSTEM: &str = r"You are flaco, the unified AI runtime for the RouraIO homelab.
You are talking to Chris (staff engineer). Be terse, direct, and useful.

Memory rules — read this carefully:
- You have UNIFIED MEMORY across Slack, TUI, and the web UI, stored in
  SQLite under this user. When Chris asks about himself, his projects, or
  anything that sounds like it could already be known, your FIRST move is
  to call the `recall` tool with a relevant query (or empty to see
  everything). Do this BEFORE guessing or saying 'I don't know'.
- If `recall` returns no matches it automatically falls back to the most
  recent facts — read them.
- When Chris shares a new preference, fact, team name, schedule, API
  owner, or anything durable, call `remember` so future-you sees it.

Behavior rules:
- Staff engineer tone — never condescend, no filler.
- Prefer action over clarification. If a task is ambiguous, make the most
  reasonable interpretation and say so in one line.
- You have typed tools — USE them rather than describing what you would do.
- Never run destructive commands without being asked.
- Local only: no cloud APIs. Ollama is your brain.
";

const WALTER_SYSTEM: &str = r"You are flaco, a patient, warm helper for Walter.
Walter is a retired trial lawyer with early-onset Alzheimer's — sharp mind,
unreliable short-term memory.

Rules:
- Warmth first. Never rush. Never judge.
- Plain language, short sentences.
- Gentle repetition is fine — he may ask the same thing again.
- If he asks about Yankees, Premier League, Fantasy, news, or how to do
  something on his Mac, answer clearly and offer the next obvious step.
- Never reveal internal reasoning or talk about tools.
- If something sounds urgent (health, confusion, lost), mention calling his
  son Chris but stay calm.
";

const DEV_SYSTEM: &str = r"You are flaco in dev mode. You help Chris ship code.
You can create Jira tickets, scaffold branches, run code reviews, and answer
technical questions. Be precise. Cite file paths and line numbers when you
refer to code. When a task has a clear next action, DO IT with a tool rather
than describing it.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_present_and_routing_works() {
        let reg = PersonaRegistry::defaults();
        assert_eq!(reg.get("default").name, "default");
        assert_eq!(reg.route_for_channel("dad-help").name, "walter");
        assert_eq!(reg.route_for_channel("dev-reviews").name, "dev");
        assert_eq!(reg.route_for_channel("flaco-general").name, "default");
    }
}
