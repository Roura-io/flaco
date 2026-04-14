# flacoAi v2 Hackathon Plan

**Branch:** `feature/hackathon-v2`
**Window:** 2026-04-13 night → 2026-04-14 09:00 ET
**Author:** Claude (autonomous, on behalf of Chris)

## Mission

Build flacoAi v2 — a unified TUI + Slack + Web runtime that shares ONE brain,
ONE memory, and ONE typed tool registry. Ship four killer features:

1. **Unified memory across surfaces** — SQLite-backed, per-user, retrievable
   via FTS5. Every surface reads and writes the same memory.
2. **Siri Shortcut generator** — `/shortcut <english>` → real `.shortcut` file
   saved to `~/Downloads/flaco-shortcuts/` ready to open on iPhone.
3. **Jira → code scaffold** — `/scaffold <idea>` → Jira epic+stories+subtasks,
   local git branch, starter folder, first commit. Idea → ready-to-code branch
   in under a minute.
4. **Web research with citations** — `/research <topic>` (or free question in
   web UI) → DuckDuckGo → fetch top results → Ollama summarize → numbered
   citations linked to sources. Works in all three surfaces.

## Guardrails

- v1 `flacoai-server` on `mac-server` stays running. v2 runs on a new port
  (3031 for web, separate binary `flaco-v2`) and does not clobber v1 state.
- No cloud AI. Ollama only (`qwen3:32b-q8_0`, `qwen3-coder:30b`, optionally
  `nomic-embed-text`).
- Walter's production workflows are untouched.
- `cargo test --workspace` must pass before meaningful commits.
- Small, atomic commits with clear messages.
- `[BLOCKED]` tag + Slack webhook note if stuck > 20 min.

## New crate layout

```
rust/crates/
  flaco-core/     # runtime loop, session, memory, tool registry, ollama client
  flaco-web/      # Axum + HTMX web server (unified UI)
  flaco-tui/      # ratatui-based shiny TUI (optional if time)
  flaco-slack/    # Socket Mode adapter over flaco-core (thin)
  flaco-v2/       # binary wiring all surfaces
```

`flaco-core` is the only crate the adapters depend on. Existing v1 crates
(`runtime`, `channels`, `commands`, etc.) are untouched so v1 keeps building
and running.

## Memory schema

```sql
CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  surface TEXT NOT NULL,      -- slack | tui | web
  user_id TEXT NOT NULL,
  persona TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id TEXT NOT NULL,
  role TEXT NOT NULL,         -- system | user | assistant | tool
  content TEXT NOT NULL,
  tool_call_id TEXT,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(conversation_id) REFERENCES conversations(id)
);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  args_json TEXT NOT NULL,
  result_json TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE memories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,         -- fact | preference | note
  content TEXT NOT NULL,
  source_conversation TEXT,
  created_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE memories_fts USING fts5(content, user_id, kind);
```

Location: `~/infra/flaco.db` on the Mac.

## Schedule

| Hour | Phase |
|------|-------|
| 0-1  | Branch, plan, scaffold flaco-core, Ollama client |
| 1-2  | Memory (SQLite+FTS5), session, persona config, unit tests |
| 2-3  | Typed tool registry: bash, fs, web_search, web_fetch, jira, github |
| 3-4  | Runtime loop with tool-calling + streaming, integration tests |
| 4-5  | Web UI (Axum + HTMX + SSE) with citations renderer |
| 5-6  | Slack v2 adapter using flaco-core (v1 keeps running) |
| 6-7  | 4 killer features: unified-memory tool, shortcut, scaffold, research |
| 7-8  | TUI adapter (ratatui) if time, integration tests |
| 8-9  | cargo test --workspace, README, deploy alongside v1, Slack summary |

## Deployment

- `cargo build --release -p flaco-v2`
- `scp target/release/flaco-v2 mac-server:~/infra/flaco-v2`
- `~/infra/flaco-v2 --web-port 3031 --slack-port 3032 --db ~/infra/flaco.db`
- v1 (`flacoai-server`) remains untouched on its original port.
