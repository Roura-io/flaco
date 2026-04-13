# flacoAi v2 — Unified Brain

> Built during a solo 9-hour hackathon on 2026-04-13 → 2026-04-14.
> Feature branch: `feature/hackathon-v2`.

## What it is

flacoAi v2 is a unified runtime that serves **Slack**, **TUI**, and **Web**
from ONE brain, ONE memory, ONE typed tool registry. Anything you say in
any surface is remembered everywhere else.

Four killer features ship in the box:

1. **Unified memory across surfaces** — SQLite-backed facts/preferences
   store, searchable via FTS5. The model can `remember` and `recall` facts
   through its tool registry.
2. **Siri Shortcut generator** — `/shortcut <name> <english>` writes a real
   `.shortcut` plist to `~/Downloads/flaco-shortcuts/` ready to AirDrop.
3. **Jira → code scaffold** — `/scaffold <idea>` creates a Jira epic plus
   stories, local git branch, starter folder, and first commit in one go.
4. **Perplexity-style research with citations** — `/research <topic>`
   does DuckDuckGo search → fetch top results → Ollama summarize → numbered
   citations. Works in every surface.

Local AI only (Ollama). v1 `flacoai-server` keeps running untouched.

## Workspace layout

```
rust/crates/
  flaco-core/      # runtime, memory, tools (14 unit + 7 integration tests)
  flaco-web/       # Axum + HTMX web server (2 unit tests)
  flaco-slack-v2/  # Socket Mode adapter, thin wrapper over core
  flaco-tui-v2/    # ratatui shiny TUI
  flaco-v2/        # clap-based binary that wires everything
```

All v1 crates (`runtime`, `channels`, `commands`, …) are untouched.
`cargo test -p flaco-core -p flaco-web` is green.

### Pre-existing test failures (NOT regressions)

Three tests fail on `main` and therefore also on this branch — they are
untouched by v2 and were broken before this work started:

- `flaco-cli::tests::init_template_mentions_detected_rust_workspace` — asserts
  `# FLACOAI.md` string that no longer exists in the template.
- `flaco-cli::tests::shared_help_uses_resume_annotation_copy` — asserts an
  old help line that was reworded.
- `tools::tests::skill_loads_local_skill_prompt` — pre-existing panic in
  crates/tools.

They fail identically on `main` at `c6d08b7`. Fixing them is out of scope
for this hackathon but called out so morning-you isn't surprised.

## Running

```
cd rust
cargo build --release -p flaco-v2

# serve web + slack together (default)
./target/release/flaco-v2 --db ~/infra/flaco.db

# individual modes
./target/release/flaco-v2 web
./target/release/flaco-v2 slack
./target/release/flaco-v2 tui

# one-shot commands
./target/release/flaco-v2 ask "what's on my plate today?"
./target/release/flaco-v2 research "best local-first databases 2026"
./target/release/flaco-v2 shortcut "Morning Brief" "Read the top Yankees headline"
./target/release/flaco-v2 scaffold "CLI tool to deduplicate dotfiles"
```

## Deployment (current state, 2026-04-13 night)

- **Binary:** `mac-server:~/infra/flaco-v2` (arm64, release build)
- **Env file:** `mac-server:~/infra/flaco-v2.env` (mode 600)
- **Startup script:** `mac-server:~/infra/start-flaco-v2-web.sh`
- **Web UI:** running on `http://mac.home:3033` (port 3033 so it doesn't
  collide with v1 on 3031)
- **Log:** `mac-server:~/infra/flaco-v2-web.log`
- **PID:** `mac-server:~/infra/flaco-v2-web.pid`
- **Memory db:** `mac-server:~/infra/flaco.db`

v1 `flacoai-server` is untouched on its original port.

## Tools registered at startup

```
bash, fs_read, fs_write, web_search, web_fetch,
jira_create_issue, github_create_pr, scaffold_idea,
create_shortcut, research,
remember, recall, list_memories
```

## Memory schema

```sql
conversations(id, surface, user_id, persona, title, created_at, updated_at)
messages(id, conversation_id, role, content, tool_name, created_at)
tool_calls(id, conversation_id, tool_name, args_json, result_json, created_at)
memories(id, user_id, kind, content, source_conversation, created_at)
memories_fts(content, user_id, kind)  -- FTS5 virtual table
```

## Why this design

- **One core, many surfaces.** Slack / TUI / Web are all thin adapters.
  That means a feature added to the runtime shows up everywhere for free.
- **Typed tools, not one giant bash.** The model sees a real JSON schema
  per tool, which means better tool selection, safer args, and a clean
  place to hang new capabilities.
- **SQLite memory.** Durable, queryable, cheap, and trivially backed up.
  FTS5 is enough for the hackathon; embeddings can drop in later.
- **Ollama only.** Local-first is non-negotiable.

## What's intentionally not done

- **Streaming in the web UI is batch-mode**, not token-by-token. HTMX swap
  after the full reply arrives. SSE can be added without touching the core.
- **Personas are not yet hot-reloadable** — they're baked into `persona.rs`.
  Externalizing to TOML is a 30-minute follow-up.
- **Walter persona routing** in Slack v2 happens via channel name
  (`dad-help`); per-email routing needs `users:read.email` which still has
  to be granted in Slack app settings.
- **Vector recall** — FTS5 is shipping; embeddings via `nomic-embed-text`
  are a drop-in upgrade to `memory::search_facts`.
