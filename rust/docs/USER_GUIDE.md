# flacoAi User Guide

**Version 0.1.0** | Built by Roura.io | Author: Christopher J. Roura

---

## What is flacoAi?

flacoAi is a local AI coding agent powered by Ollama. It runs entirely on your hardware — no cloud API keys needed. It connects to your local (or remote) Ollama instance and gives you an interactive coding assistant with built-in tools, engineering skills, and system integrations.

---

## Installation

### Option A: Setup Script (recommended)
```bash
git clone git@github.com:Roura-io/flaco.git
cd flaco
./setup.sh
```

The setup script handles everything:
- Installs Rust (if needed)
- Installs Ollama (if needed)
- Builds the release binary
- Installs bundled engineering skills
- Configures your shell (PATH, model, Ollama URL)
- Pulls an AI model

### Option B: Homebrew
```bash
brew tap roura-io/tap
brew install flacoai
```

### Option C: Manual Build
```bash
cd rust
cargo build --release -p flaco-cli
cp target/release/flacoai ~/.local/bin/
cp -r crates/tools/skills ~/.local/bin/skills
```

---

## Quick Start

### Start the Interactive REPL
```
flacoai
```

### One-Shot Prompt
```
flacoai "explain this function"
flacoai -p "fix the bug in main.rs"
```

### Specify a Model
```
flacoai --model qwen3:32b
```

### Permission Modes
```
flacoai --permission-mode read-only          # can only read files
flacoai --permission-mode workspace-write    # can read and edit files
flacoai --permission-mode danger-full-access # full shell + file access
```

---

## Slash Commands

Type these in the REPL:

### Core
| Command | Description |
|---------|-------------|
| `/help` | Show all available commands |
| `/status` | Show session status |
| `/model [name]` | Show or switch the active model |
| `/permissions [mode]` | Show or switch permission mode |
| `/clear` | Start a fresh session |
| `/compact` | Compress session history to save context |

### Workspace
| Command | Description |
|---------|-------------|
| `/init` | Scaffold FLACOAI.md and config files |
| `/memory` | Inspect instruction files and memory |
| `/config` | Show runtime configuration |
| `/diff` | Show git diff |

### Git
| Command | Description |
|---------|-------------|
| `/commit [message]` | Stage and commit changes |
| `/branch list/create/switch` | Manage git branches |
| `/worktree add/remove/list` | Manage git worktrees |

### Skills & Agents
| Command | Description |
|---------|-------------|
| `/skills` | List all available skills |
| `/agents` | List all available agents |
| `/plugins` | Manage plugins |

### Editor
| Shortcut | Action |
|----------|--------|
| `Tab` | Complete slash commands |
| `Shift+Enter` or `Ctrl+J` | Insert a newline |
| `/vim` | Toggle modal editing |

---

## Engineering Skills

flacoAi ships with 10 bundled engineering workflow skills. Trigger them by name:

### `/code-review`
Reviews code changes for security, performance, and correctness. Runs `git diff`, reads changed files, and produces a structured report with Critical/Warning/Info findings and an APPROVE/REQUEST_CHANGES verdict.

### `/debug`
Structured debugging in 4 phases:
1. **Reproduce** — capture the exact error
2. **Isolate** — narrow to root cause
3. **Diagnose** — explain why it's broken
4. **Fix** — implement and verify the fix

### `/deploy-checklist`
Pre-deployment verification checklist: build, tests, lint, security scan, config check, migration review, rollback plan. Outputs a pass/fail table with a READY/BLOCKED verdict.

### `/standup`
Generates a daily standup report from git history: what was completed, what's in progress, blockers, and plan for today.

### `/architecture`
Creates or evaluates Architecture Decision Records (ADRs): context, decision drivers, options with pros/cons, decision rationale, consequences.

### `/documentation`
Writes and maintains technical documentation: README, API docs, setup guides, architecture docs. Reads existing code to stay accurate.

### `/incident-response`
Two modes:
- **Active incident** — triage, identify cause, suggest mitigation, draft status update
- **Post-incident** — build timeline, write RCA with root cause, impact, action items

### `/retro`
Sprint/project retrospective: gathers git data, analyzes velocity/hotspots/churn, generates what went well/didn't/action items.

### `/tech-debt`
Technical debt audit: scans for TODO/FIXME/HACK markers, large files, high-churn files, missing tests. Categorizes and prioritizes with impact/effort scores.

### `/onboarding`
Generates an onboarding guide for new contributors: prerequisites, quick start, project structure, architecture overview, development workflow, common tasks.

---

## Built-in Integrations

flacoAi comes with 19 built-in integrations that work like Perplexity — the AI automatically uses them when relevant.

### Code & Dev
| Integration | What It Does | Requires |
|-------------|-------------|----------|
| `github` | Search repos, list issues/PRs, view code | `gh` CLI |
| `git_ops` | Log, blame, diff, stash, shortlog | `git` |
| `docker` | List containers, logs, stats, inspect | `docker` CLI |
| `npm_registry` | Search packages, check versions | (always available) |
| `crates_io` | Search Rust crates, check versions | (always available) |
| `homebrew` | Search/info formulae | `brew` CLI |

### Research & Web
| Integration | What It Does | Requires |
|-------------|-------------|----------|
| `stack_overflow` | Search Q&A | (always available) |
| `docs_fetch` | Fetch & extract docs from URLs | (always available) |
| `youtube_transcript` | Extract video transcripts | `yt-dlp` |
| `pdf_read` | Extract text from PDFs | `pdftotext` |

### Productivity (macOS)
| Integration | What It Does | Requires |
|-------------|-------------|----------|
| `calendar` | Read Calendar events | macOS |
| `reminders` | Read/create Reminders | macOS |
| `notes` | Search/read Notes | macOS |
| `contacts` | Search Contacts | macOS |

### Cloud Services
| Integration | What It Does | Requires |
|-------------|-------------|----------|
| `slack` | Send messages, read channels | `SLACK_TOKEN` env var |
| `jira` | Search/view issues | `JIRA_URL`, `JIRA_EMAIL`, `JIRA_TOKEN` |
| `notion` | Search/read pages | `NOTION_TOKEN` |

### System
| Integration | What It Does | Requires |
|-------------|-------------|----------|
| `system_info` | CPU, RAM, disk, network, processes | (always available) |
| `ollama_admin` | List/pull/show models on Ollama host | Ollama running |

---

## Model Intelligence

flacoAi can intelligently recommend models based on what you're doing:

| Task Type | Preferred Models |
|-----------|-----------------|
| Coding | qwen3, deepseek-coder, codellama |
| Research | qwen3, llama3, mistral |
| Reasoning | deepseek-r1, qwen3, llama3 |
| Creative | llama3, mistral, qwen3 |
| General | qwen3 (default) |

If a preferred model isn't installed, flacoAi will suggest which model to pull on your Ollama host.

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OLLAMA_BASE_URL` | `http://localhost:11434/v1` | Ollama server URL |
| `FLACO_MODEL` | `qwen3:30b-a3b` | Default model |
| `SLACK_TOKEN` | (none) | Slack bot token |
| `JIRA_URL` | (none) | Jira instance URL |
| `JIRA_EMAIL` | (none) | Jira email |
| `JIRA_TOKEN` | (none) | Jira API token |
| `NOTION_TOKEN` | (none) | Notion integration token |

### Remote Ollama Host

If your Ollama runs on a different machine (e.g., a GPU server):

```bash
# Add to ~/.zshrc
export OLLAMA_BASE_URL="http://10.0.1.3:11434/v1"
```

### Project Configuration

Create a `FLACOAI.md` in your project root to give flacoAi project-specific instructions:

```bash
flacoai
> /init
```

This creates:
- `FLACOAI.md` — project instructions (auto-detected for your stack)
- `.flacoai/settings.json` — project-level settings
- `.gitignore` entries for local session files

### Settings Files (JSON)

Priority order (later overrides earlier):
1. `~/.flacoai/settings.json` — global
2. `.flacoai/settings.json` — project
3. `.flacoai/settings.local.json` — local (gitignored)

---

## Custom Skills

Create your own skills in `.flacoai/skills/<name>/SKILL.md`:

```markdown
---
name: my-skill
description: "What this skill does"
---

# Instructions for the AI

Your detailed instructions here...
```

Skills are discovered from:
1. Bundled skills (shipped with flacoAi)
2. Project: `.flacoai/skills/`
3. User: `~/.flacoai/skills/`

Project/user skills can shadow bundled ones with the same name.

---

## Tips

- **Tab completion** — type `/` then Tab to see all slash commands
- **Multiline input** — Shift+Enter or Ctrl+J for newlines
- **Resume sessions** — `flacoai --resume path/to/session.json`
- **Vim mode** — type `/vim` to toggle modal editing
- **Permission escalation** — if the AI needs a tool you haven't allowed, it will ask

---

## File Locations

| Path | Purpose |
|------|---------|
| `~/.local/bin/flacoai` | The binary |
| `~/.local/bin/skills/` | Bundled engineering skills |
| `~/.flaco/` | State directory (install state, preferences) |
| `~/.flacoai/` | User-level config and skills |
| `.flacoai/` | Project-level config and skills |
| `FLACOAI.md` | Project instructions |

---

## Troubleshooting

### "model not found"
The model isn't installed on your Ollama host. Run:
```bash
ollama pull qwen3:32b
```

### "failed to reach Ollama"
Check that Ollama is running:
```bash
curl http://localhost:11434/api/tags
```
Or if remote:
```bash
curl http://10.0.1.3:11434/api/tags
```

### "permission denied"
Run flacoAi with a higher permission mode:
```bash
flacoai --permission-mode workspace-write
```

### Skills not showing
Verify the skills directory exists next to the binary:
```bash
ls $(dirname $(which flacoai))/skills/
```

---

*flacoAi by Roura.io — Local AI, zero cloud dependency.*
