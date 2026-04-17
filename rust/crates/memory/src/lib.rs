//! Project memory system for flacoAi — persistent memory across sessions.
//!
//! Stores decisions, conventions, context, and notes in `.flacoai/memory.json`
//! so the AI retains knowledge between conversations.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Categories of project memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Decisions,
    Conventions,
    Context,
    Notes,
}

impl MemoryCategory {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Decisions => "decisions",
            Self::Conventions => "conventions",
            Self::Context => "context",
            Self::Notes => "notes",
        }
    }

    /// Parse from a string, case-insensitive.
    #[must_use]
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "decisions" | "decision" => Some(Self::Decisions),
            "conventions" | "convention" => Some(Self::Conventions),
            "context" => Some(Self::Context),
            "notes" | "note" => Some(Self::Notes),
            _ => None,
        }
    }
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: u64,
    pub category: MemoryCategory,
    pub content: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// MemoryStore
// ---------------------------------------------------------------------------

/// Persistent project memory backed by a JSON file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryStore {
    #[serde(default)]
    entries: Vec<MemoryEntry>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl MemoryStore {
    /// Load from a JSON file. Returns an empty store if the file doesn't exist.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let mut store = std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str::<Self>(&content).ok())
            .unwrap_or_default();
        store.path = Some(path.to_path_buf());
        store
    }

    /// Create an empty store that will save to the given path.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            entries: Vec::new(),
            path: Some(path),
        }
    }

    /// Save the store to disk.
    pub fn save(&self) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| "no path configured for memory store".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Add a memory entry.
    pub fn add(
        &mut self,
        category: MemoryCategory,
        content: String,
        tags: Vec<String>,
    ) -> &MemoryEntry {
        let id = self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let entry = MemoryEntry {
            id,
            category,
            content,
            created_at: iso_now(),
            tags,
        };
        self.entries.push(entry);
        self.entries.last().expect("just pushed")
    }

    /// Search entries by substring match on content, category, or tags.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&MemoryEntry> {
        let lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.content.to_lowercase().contains(&lower)
                    || e.category.label().contains(&lower)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&lower))
            })
            .collect()
    }

    /// List all entries, optionally filtered by category.
    #[must_use]
    pub fn list(&self, category: Option<MemoryCategory>) -> Vec<&MemoryEntry> {
        match category {
            Some(cat) => self.entries.iter().filter(|e| e.category == cat).collect(),
            None => self.entries.iter().collect(),
        }
    }

    /// Remove an entry by ID. Returns true if found and removed.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() < before
    }

    /// Total number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Render memory entries as a system prompt section.
    #[must_use]
    pub fn render_for_prompt(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let mut lines = vec!["# Project Memory (from previous sessions)".to_string()];

        for category in [
            MemoryCategory::Decisions,
            MemoryCategory::Conventions,
            MemoryCategory::Context,
            MemoryCategory::Notes,
        ] {
            let items: Vec<&MemoryEntry> = self
                .entries
                .iter()
                .filter(|e| e.category == category)
                .collect();
            if items.is_empty() {
                continue;
            }

            lines.push(String::new());
            lines.push(format!("## {}", capitalize(category.label())));
            for entry in items {
                let tag_str = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", entry.tags.join(", "))
                };
                lines.push(format!("- {}{tag_str}", entry.content));
            }
        }

        lines.join("\n")
    }

    /// Default path for memory file in a project directory.
    #[must_use]
    pub fn default_path(project_root: &Path) -> PathBuf {
        project_root.join(".flacoai").join("memory.json")
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO-ish timestamp without pulling in chrono
    format!("{now}")
}

// ---------------------------------------------------------------------------
// Tool interface — used by the tools crate to dispatch memory operations
// ---------------------------------------------------------------------------

/// Input for the memory tool.
#[derive(Debug, Deserialize)]
pub struct MemoryToolInput {
    pub action: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Execute a memory tool action on the given store.
pub fn execute_memory_tool(
    store: &mut MemoryStore,
    input: MemoryToolInput,
) -> Result<String, String> {
    match input.action.as_str() {
        "save" | "add" => {
            let category = input
                .category
                .as_deref()
                .and_then(MemoryCategory::from_str_loose)
                .unwrap_or(MemoryCategory::Notes);
            let content = input
                .content
                .filter(|c| !c.trim().is_empty())
                .ok_or("content is required for save")?;
            let tags = input.tags.unwrap_or_default();
            let entry = store.add(category, content, tags);
            let result = serde_json::to_string_pretty(entry).unwrap_or_default();
            store.save()?;
            Ok(result)
        }
        "search" | "recall" => {
            let query = input
                .query
                .or(input.content)
                .filter(|q| !q.trim().is_empty())
                .ok_or("query is required for search")?;
            let results = store.search(&query);
            if results.is_empty() {
                Ok("No memories found matching that query.".into())
            } else {
                Ok(serde_json::to_string_pretty(&results).unwrap_or_default())
            }
        }
        "list" => {
            let category = input
                .category
                .as_deref()
                .and_then(MemoryCategory::from_str_loose);
            let results = store.list(category);
            if results.is_empty() {
                Ok("No memories stored.".into())
            } else {
                Ok(serde_json::to_string_pretty(&results).unwrap_or_default())
            }
        }
        "remove" | "delete" => {
            let id = input.id.ok_or("id is required for remove")?;
            if store.remove(id) {
                store.save()?;
                Ok(format!("Memory {id} removed."))
            } else {
                Err(format!("Memory {id} not found."))
            }
        }
        _ => Err(format!("unknown memory action: {}", input.action)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("flacoai-memory-{label}-{nanos}"))
    }

    #[test]
    fn add_and_list() {
        let dir = temp_path("add-list");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory.json");

        let mut store = MemoryStore::new(path.clone());
        store.add(
            MemoryCategory::Decisions,
            "Use axum for HTTP".into(),
            vec![],
        );
        store.add(
            MemoryCategory::Conventions,
            "snake_case everywhere".into(),
            vec!["style".into()],
        );

        assert_eq!(store.len(), 2);
        assert_eq!(store.list(Some(MemoryCategory::Decisions)).len(), 1);
        assert_eq!(store.list(None).len(), 2);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn search_by_content_and_tag() {
        let mut store = MemoryStore::default();
        store.add(
            MemoryCategory::Context,
            "Project uses Rust".into(),
            vec!["lang".into()],
        );
        store.add(
            MemoryCategory::Notes,
            "Remember to update docs".into(),
            vec![],
        );

        assert_eq!(store.search("rust").len(), 1);
        assert_eq!(store.search("lang").len(), 1);
        assert_eq!(store.search("update").len(), 1);
        assert_eq!(store.search("nonexistent").len(), 0);
    }

    #[test]
    fn remove_entry() {
        let mut store = MemoryStore::default();
        store.add(MemoryCategory::Notes, "temp note".into(), vec![]);
        assert_eq!(store.len(), 1);
        assert!(store.remove(1));
        assert_eq!(store.len(), 0);
        assert!(!store.remove(999));
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = temp_path("roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory.json");

        let mut store = MemoryStore::new(path.clone());
        store.add(
            MemoryCategory::Decisions,
            "Use Ollama".into(),
            vec!["infra".into()],
        );
        store.save().unwrap();

        let loaded = MemoryStore::load(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.list(None)[0].content, "Use Ollama");
        assert_eq!(loaded.list(None)[0].tags, vec!["infra"]);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn render_for_prompt_groups_by_category() {
        let mut store = MemoryStore::default();
        store.add(MemoryCategory::Decisions, "Use axum".into(), vec![]);
        store.add(MemoryCategory::Conventions, "snake_case".into(), vec![]);
        store.add(MemoryCategory::Decisions, "Use Ollama".into(), vec![]);

        let rendered = store.render_for_prompt();
        assert!(rendered.contains("# Project Memory"));
        assert!(rendered.contains("## Decisions"));
        assert!(rendered.contains("## Conventions"));
        assert!(rendered.contains("- Use axum"));
        assert!(rendered.contains("- Use Ollama"));
        assert!(rendered.contains("- snake_case"));
    }

    #[test]
    fn tool_interface_save_and_search() {
        let dir = temp_path("tool");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory.json");

        let mut store = MemoryStore::new(path);

        let result = execute_memory_tool(
            &mut store,
            MemoryToolInput {
                action: "save".into(),
                category: Some("decisions".into()),
                content: Some("Use Rust for the CLI".into()),
                query: None,
                id: None,
                tags: Some(vec!["architecture".into()]),
            },
        );
        assert!(result.is_ok());

        let result = execute_memory_tool(
            &mut store,
            MemoryToolInput {
                action: "search".into(),
                category: None,
                content: None,
                query: Some("rust".into()),
                id: None,
                tags: None,
            },
        );
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Use Rust"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_store_renders_nothing() {
        let store = MemoryStore::default();
        assert!(store.render_for_prompt().is_empty());
    }

    #[test]
    fn category_from_str_loose() {
        assert_eq!(
            MemoryCategory::from_str_loose("decisions"),
            Some(MemoryCategory::Decisions)
        );
        assert_eq!(
            MemoryCategory::from_str_loose("Decision"),
            Some(MemoryCategory::Decisions)
        );
        assert_eq!(
            MemoryCategory::from_str_loose("CONVENTIONS"),
            Some(MemoryCategory::Conventions)
        );
        assert_eq!(MemoryCategory::from_str_loose("bogus"), None);
    }
}
