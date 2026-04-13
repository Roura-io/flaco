# flaco-core

The brain of flacoAi v2 — a single runtime crate that Slack, TUI, and Web
surfaces all share.

```
flaco-core
├── error        Error + Result
├── ollama       Minimal Ollama HTTP client (chat + streaming chat)
├── memory       SQLite-backed unified memory (+ FTS5 search)
├── persona      System prompts: default / walter / dev
├── session      Conversation wrapper that resumes by (surface, user)
├── tools        Typed tool registry
│   ├── bash     Guarded shell command tool
│   ├── fs_rw    Read/write files
│   ├── web      DuckDuckGo search + readable fetch
│   ├── jira     Create issues (Jira REST v3)
│   ├── github   Create PRs (GitHub REST)
│   ├── memory_tool  remember / recall / list_memories
│   ├── shortcut Siri Shortcut generator (XML plist)
│   ├── scaffold Jira epic + git branch + starter scaffold
│   └── research Perplexity-style search → fetch → cite
├── runtime      handle_turn tool-calling loop with event bus
└── features     Direct entry points for slash commands
```

## Usage

```rust
use std::sync::Arc;
use flaco_core::{
    memory::Memory, ollama::OllamaClient, persona::PersonaRegistry,
    runtime::{Runtime, Surface}, tools::ToolRegistry,
};

let memory = Memory::open("~/infra/flaco.db")?;
let ollama = OllamaClient::from_env();
let reg = ToolRegistry::new(); // register tools here
let runtime = Runtime::new(memory, ollama, reg, PersonaRegistry::defaults());

let session = runtime.session(&Surface::Web, "chris")?;
let reply = runtime.handle_turn(&session, "hi flaco", None).await?;
```

## Tests

```
cargo test -p flaco-core
cargo test -p flaco-core --test integration
```
