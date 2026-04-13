//! Unified SQLite-backed memory for flacoAi v2.
//!
//! All surfaces (Slack/TUI/Web) read and write through this single store so
//! a conversation started in Slack can be continued from the TUI, and facts
//! remembered in one place are visible everywhere.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "system" => Role::System,
            "user" => Role::User,
            "tool" => Role::Tool,
            _ => Role::Assistant,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub surface: String,
    pub user_id: String,
    pub persona: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub conversation_id: String,
    pub role: Role,
    pub content: String,
    pub tool_name: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fact {
    pub id: i64,
    pub user_id: String,
    pub kind: String,
    pub content: String,
    pub source_conversation: Option<String>,
    pub created_at: i64,
}

/// Thread-safe wrapper around a single SQLite connection.
#[derive(Clone)]
pub struct Memory {
    inner: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory").field("path", &self.path).finish()
    }
}

impl Memory {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { inner: Arc::new(Mutex::new(conn)), path })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { inner: Arc::new(Mutex::new(conn)), path: PathBuf::from(":memory:") })
    }

    pub fn path(&self) -> &Path { &self.path }

    pub fn create_conversation(
        &self,
        surface: &str,
        user_id: &str,
        persona: &str,
    ) -> Result<Conversation> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let conv = Conversation {
            id: id.clone(),
            surface: surface.into(),
            user_id: user_id.into(),
            persona: persona.into(),
            title: None,
            created_at: now,
            updated_at: now,
        };
        let c = self.inner.lock().unwrap();
        c.execute(
            "INSERT INTO conversations (id, surface, user_id, persona, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
            params![conv.id, conv.surface, conv.user_id, conv.persona, conv.created_at, conv.updated_at],
        )?;
        Ok(conv)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let c = self.inner.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, surface, user_id, persona, title, created_at, updated_at
             FROM conversations WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Conversation {
                id: row.get(0)?,
                surface: row.get(1)?,
                user_id: row.get(2)?,
                persona: row.get(3)?,
                title: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Look up the most recent conversation for a (surface,user_id) pair,
    /// optionally filtering by a room/channel id encoded into the user_id.
    pub fn latest_conversation_for(
        &self,
        surface: &str,
        user_id: &str,
    ) -> Result<Option<Conversation>> {
        let c = self.inner.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, surface, user_id, persona, title, created_at, updated_at
             FROM conversations WHERE surface = ?1 AND user_id = ?2
             ORDER BY updated_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![surface, user_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Conversation {
                id: row.get(0)?,
                surface: row.get(1)?,
                user_id: row.get(2)?,
                persona: row.get(3)?,
                title: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        role: Role,
        content: &str,
        tool_name: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let c = self.inner.lock().unwrap();
        c.execute(
            "INSERT INTO messages (conversation_id, role, content, tool_name, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![conversation_id, role.as_str(), content, tool_name, now],
        )?;
        let id = c.last_insert_rowid();
        c.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )?;
        Ok(id)
    }

    pub fn recent_messages(&self, conversation_id: &str, limit: usize) -> Result<Vec<Message>> {
        let c = self.inner.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, conversation_id, role, content, tool_name, created_at
             FROM messages
             WHERE conversation_id = ?1
             ORDER BY id DESC LIMIT ?2",
        )?;
        let iter = stmt.query_map(params![conversation_id, limit as i64], |row| {
            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: Role::parse(&row.get::<_, String>(2)?),
                content: row.get(3)?,
                tool_name: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut v: Vec<Message> = iter.collect::<std::result::Result<_, _>>()?;
        v.reverse();
        Ok(v)
    }

    pub fn record_tool_call(
        &self,
        conversation_id: &str,
        tool: &str,
        args_json: &str,
        result_json: &str,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        let c = self.inner.lock().unwrap();
        c.execute(
            "INSERT INTO tool_calls (id, conversation_id, tool_name, args_json, result_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, conversation_id, tool, args_json, result_json, now],
        )?;
        Ok(())
    }

    pub fn remember_fact(
        &self,
        user_id: &str,
        kind: &str,
        content: &str,
        source_conversation: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let c = self.inner.lock().unwrap();
        c.execute(
            "INSERT INTO memories (user_id, kind, content, source_conversation, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id, kind, content, source_conversation, now],
        )?;
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO memories_fts (rowid, content, user_id, kind) VALUES (?1, ?2, ?3, ?4)",
            params![id, content, user_id, kind],
        )?;
        Ok(id)
    }

    pub fn forget_fact(&self, id: i64) -> Result<()> {
        let c = self.inner.lock().unwrap();
        c.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        c.execute("DELETE FROM memories_fts WHERE rowid = ?1", params![id])?;
        Ok(())
    }

    pub fn search_facts(&self, user_id: &str, query: &str, limit: usize) -> Result<Vec<Fact>> {
        let c = self.inner.lock().unwrap();
        // FTS5 MATCH; fall back to LIKE if empty query.
        if query.trim().is_empty() {
            return self.all_facts(user_id, limit);
        }
        let mut stmt = c.prepare(
            "SELECT m.id, m.user_id, m.kind, m.content, m.source_conversation, m.created_at
             FROM memories_fts f JOIN memories m ON m.id = f.rowid
             WHERE f.user_id = ?1 AND f.content MATCH ?2
             ORDER BY m.created_at DESC LIMIT ?3",
        )?;
        let iter = stmt.query_map(params![user_id, query, limit as i64], |row| {
            Ok(Fact {
                id: row.get(0)?,
                user_id: row.get(1)?,
                kind: row.get(2)?,
                content: row.get(3)?,
                source_conversation: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(iter.collect::<std::result::Result<_, _>>()?)
    }

    pub fn all_facts(&self, user_id: &str, limit: usize) -> Result<Vec<Fact>> {
        let c = self.inner.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, user_id, kind, content, source_conversation, created_at
             FROM memories WHERE user_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let iter = stmt.query_map(params![user_id, limit as i64], |row| {
            Ok(Fact {
                id: row.get(0)?,
                user_id: row.get(1)?,
                kind: row.get(2)?,
                content: row.get(3)?,
                source_conversation: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(iter.collect::<std::result::Result<_, _>>()?)
    }

    pub fn list_conversations(&self, limit: usize) -> Result<Vec<Conversation>> {
        let c = self.inner.lock().unwrap();
        let mut stmt = c.prepare(
            "SELECT id, surface, user_id, persona, title, created_at, updated_at
             FROM conversations ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let iter = stmt.query_map(params![limit as i64], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                surface: row.get(1)?,
                user_id: row.get(2)?,
                persona: row.get(3)?,
                title: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        Ok(iter.collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_title(&self, conversation_id: &str, title: &str) -> Result<()> {
        let c = self.inner.lock().unwrap();
        c.execute(
            "UPDATE conversations SET title = ?1 WHERE id = ?2",
            params![title, conversation_id],
        )?;
        Ok(())
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    surface TEXT NOT NULL,
    user_id TEXT NOT NULL,
    persona TEXT NOT NULL,
    title TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conv_user ON conversations(user_id, surface);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_name TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id)
);
CREATE INDEX IF NOT EXISTS idx_msg_conv ON messages(conversation_id);

CREATE TABLE IF NOT EXISTS tool_calls (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    args_json TEXT NOT NULL,
    result_json TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES conversations(id)
);

CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    source_conversation TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mem_user ON memories(user_id);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    content, user_id, kind,
    tokenize='porter unicode61'
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trip() {
        let m = Memory::open_in_memory().unwrap();
        let conv = m.create_conversation("web", "chris", "default").unwrap();
        m.append_message(&conv.id, Role::User, "hello", None).unwrap();
        m.append_message(&conv.id, Role::Assistant, "hi chris", None).unwrap();
        let msgs = m.recent_messages(&conv.id, 10).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[1].content, "hi chris");
    }

    #[test]
    fn latest_conversation_tracks_updates() {
        let m = Memory::open_in_memory().unwrap();
        let a = m.create_conversation("slack", "U1", "default").unwrap();
        // sleep zero but change updated_at implicitly with append
        m.append_message(&a.id, Role::User, "ping", None).unwrap();
        let latest = m.latest_conversation_for("slack", "U1").unwrap().unwrap();
        assert_eq!(latest.id, a.id);
    }

    #[test]
    fn facts_fts_search() {
        let m = Memory::open_in_memory().unwrap();
        m.remember_fact("chris", "preference", "prefers terse responses", None).unwrap();
        m.remember_fact("chris", "fact", "yankees fan", None).unwrap();
        let hits = m.search_facts("chris", "yankees", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, "yankees fan");
    }
}
