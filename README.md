# flacoAi

<p align="center">
  <strong>Your local AI coding agent, powered by Ollama</strong>
</p>

<p align="center">
  Built by <a href="https://roura.io">Roura.io</a> &mdash; Christopher J. Roura
</p>

---

flacoAi is a self-hosted alternative to cloud-based AI coding assistants. Everything runs locally on your machine using [Ollama](https://ollama.com) — your code never leaves your computer.

## v3.0.0-rc1 — latest

flacoAi v3 is a unified runtime that serves Slack, TUI, Web, and CLI from **one brain, one memory, and one typed tool registry**. It adds:

- **Jarvis-mode** — natural-language intent router. Type `clear`, `reset`, `clear this chat`, `how are you?`, `what's on my plate` — it all works instantly, no slash-command memorisation, no LLM round-trip for meta-commands.
- **Production hardening** — automated `sqlite3 VACUUM INTO` backups, launchd supervisor with `KeepAlive`, a TOML config file at `/opt/homebrew/etc/flaco/config.toml`, a tool-loop duplication fix that was silently writing duplicate memories on v2, and a `flaco-v2 doctor` health probe that validates everything in one command.
- **CI** — GitHub Actions runs `cargo test` + `cargo clippy -- -D warnings` on every push. Live-Ollama integration test ships too, opt-in via `--ignored`.
- **v1 compat preserved** — the `flacoai` CLI and `flacoai-server` Slack bot are untouched. Safe to upgrade incrementally.

**Read more:** [`RELEASE_NOTES_v3.0.0.md`](./RELEASE_NOTES_v3.0.0.md)

**Run the smoke:** `bash deploy/smoke-v3.sh` → 13 probes, exit 0 if green.

**Everything below is still accurate for the v1 `flacoai` CLI.**

## Features

- **Interactive REPL** — chat with your AI coding agent in the terminal
- **Tool execution** — file read/write/edit, bash commands, grep, glob, web fetch
- **Streaming responses** — real-time token-by-token output
- **Conversation history** — context-aware multi-turn sessions
- **Session persistence** — resume previous conversations
- **Multiple models** — use any Ollama-compatible model
- **Ollama-native** — connects via OpenAI-compatible API at `localhost:11434`

## Install

### Homebrew (recommended)

```bash
brew tap Roura-io/tap
brew install flacoai
```

Then install [Ollama](https://ollama.com) and pull a model:

```bash
brew install --cask ollama
ollama pull qwen3:30b-a3b
```

### Interactive installer

Download the latest release from [Releases](https://github.com/Roura-io/flaco/releases), extract it, and:

- **macOS:** Double-click `install.command`
- **Terminal:** Run `./setup.sh`

The interactive installer handles Rust, Ollama, building the CLI, pulling a model, and configuring your shell.

### From source

```bash
git clone https://github.com/Roura-io/flaco.git
cd flaco
./setup.sh
```

## Quick Start

```bash
flacoai                              # interactive REPL
flacoai "explain this function"      # one-shot prompt
flacoai --model qwen3:8b             # use a specific model
```

Set a default model:

```bash
export FLACO_MODEL="qwen3:30b-a3b"
```

## Developer Build

If you're working on flacoAi itself, install the dev binary alongside the stable one:

```bash
./setup.sh --dev
```

This installs `flacoai-dev` to `~/.cargo/bin/` so both can coexist:

```bash
flacoai          # stable release
flacoai-dev      # your local dev build
```

## Project Structure

```
.
├── rust/                    # Rust CLI — primary implementation
│   ├── crates/flaco-cli/   # Interactive REPL binary
│   ├── crates/api/          # API client + Ollama provider + streaming
│   ├── crates/runtime/      # Session, tools, conversation loop, config
│   ├── crates/tools/        # 19 built-in tool specs + execution
│   ├── crates/commands/     # Slash commands
│   ├── crates/plugins/      # Plugin system
│   ├── crates/lsp/          # LSP client integration
│   └── crates/server/       # HTTP/SSE server
├── src/                     # Python — Ollama client, query engine, tools
├── tests/                   # Validation
├── setup.sh                 # Interactive installer
└── install.command          # macOS double-click installer
```

## Built-in Tools

| Tool | Description |
|------|-------------|
| `bash` | Shell execution with timeout & background support |
| `read_file` | Read files with offset/limit pagination |
| `write_file` | Create or update files |
| `edit_file` | String replacement editing |
| `glob_search` | Find files by pattern |
| `grep_search` | Regex search with context |
| `web_fetch` | Fetch URL content |
| `web_search` | Web search with domain filtering |
| `todo_write` | Structured task management |
| `notebook_edit` | Jupyter notebook editing |
| `agent` | Launch specialized sub-agents |

## Requirements

- macOS or Linux
- Internet connection (first-time setup only)
- ~8GB+ RAM recommended for default model

## License

MIT

## Author

**Christopher J. Roura** — [cjroura@roura.io](mailto:cjroura@roura.io) — [Roura.io](https://roura.io)
