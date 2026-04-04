# flacoAi

<p align="center">
  <strong>Your local AI coding agent, powered by Ollama</strong>
</p>

<p align="center">
  Built by <a href="https://roura.io">Roura.io</a> &mdash; Christopher J. Roura
</p>

---

flacoAi is a self-hosted alternative to cloud-based AI coding assistants. Everything runs locally on your machine using [Ollama](https://ollama.com) — your code never leaves your computer.

## Features

- **Interactive REPL** — chat with your AI coding agent in the terminal
- **Tool execution** — file read/write/edit, bash commands, grep, glob, web fetch
- **Streaming responses** — real-time token-by-token output
- **Conversation history** — context-aware multi-turn sessions
- **Session persistence** — resume previous conversations
- **Multiple models** — use any Ollama-compatible model
- **Ollama-native** — connects via OpenAI-compatible API at `localhost:11434`

## Install

Download the latest release from [Releases](https://github.com/Roura-io/flaco/releases), extract it, and:

- **macOS:** Double-click `install.command`
- **Terminal:** Run `./setup.sh`

Or clone and build:

```bash
git clone https://github.com/Roura-io/flaco.git
cd flaco
./setup.sh
```

The installer handles Rust, Ollama, building the CLI, pulling a model, and configuring your shell.

## Quick Start

```bash
flaco                              # interactive REPL
flaco "explain this function"      # one-shot prompt
flaco --model qwen3:8b             # use a specific model
```

Set a default model:

```bash
export FLACO_MODEL="qwen3:30b-a3b"
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
