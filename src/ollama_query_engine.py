"""Ollama-powered query engine that replaces the mock QueryEnginePort.

This module bridges the flacoAi porting workspace with real Ollama
local model inference, using the same tool system from the ported tools.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Generator
from uuid import uuid4

from .ollama_client import OllamaClient, OllamaError, StreamToken, UsageStats


# ---------------------------------------------------------------------------
# Tool schema registry — maps the ported tool inventory to Ollama format
# ---------------------------------------------------------------------------

OLLAMA_TOOL_SCHEMAS: list[dict] = [
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a file at the given path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute or relative path to the file"},
                },
                "required": ["path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "write_file",
            "description": "Write content to a file, creating it if it doesn't exist.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to the file"},
                    "content": {"type": "string", "description": "Content to write"},
                },
                "required": ["path", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "edit_file",
            "description": "Replace a string in a file with a new string.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to the file"},
                    "old_string": {"type": "string", "description": "The text to find and replace"},
                    "new_string": {"type": "string", "description": "The replacement text"},
                },
                "required": ["path", "old_string", "new_string"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "list_directory",
            "description": "List files and directories at the given path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path to list"},
                },
                "required": ["path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "find_files",
            "description": "Search for files by name pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern to match (e.g. '*.py')"},
                    "path": {"type": "string", "description": "Starting directory"},
                },
                "required": ["pattern"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "search_in_files",
            "description": "Search for a pattern across files in a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex or text pattern to search for"},
                    "path": {"type": "string", "description": "Directory to search in"},
                    "include": {"type": "string", "description": "File glob to include (e.g. '*.py')"},
                },
                "required": ["pattern"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "run_command",
            "description": "Execute a shell command and return its output.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to run"},
                    "cwd": {"type": "string", "description": "Working directory (optional)"},
                },
                "required": ["command"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "create_branch",
            "description": "Create a new git branch.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Branch name to create"},
                },
                "required": ["name"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "git_commit",
            "description": "Stage all changes and create a git commit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "Commit message"},
                },
                "required": ["message"],
            },
        },
    },
]


# ---------------------------------------------------------------------------
# Basic tool executor (file ops + shell)
# ---------------------------------------------------------------------------


class BasicToolExecutor:
    """Executes tool calls from the Ollama model.

    Provides file operations, directory listing, and shell commands.
    """

    def __init__(self, cwd: str = ".") -> None:
        self.cwd = cwd

    def execute(self, name: str, args: dict) -> str:
        """Execute a tool by name and return the result as a string."""
        import os
        import subprocess
        import glob as glob_mod
        from pathlib import Path

        try:
            if name == "read_file":
                path = Path(args["path"]).expanduser()
                if not path.exists():
                    return f"Error: File not found: {path}"
                return path.read_text(errors="replace")[:50000]

            elif name == "write_file":
                path = Path(args["path"]).expanduser()
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(args["content"])
                return f"Written {len(args['content'])} chars to {path}"

            elif name == "edit_file":
                path = Path(args["path"]).expanduser()
                if not path.exists():
                    return f"Error: File not found: {path}"
                content = path.read_text()
                old = args["old_string"]
                if old not in content:
                    return f"Error: old_string not found in {path}"
                new_content = content.replace(old, args["new_string"], 1)
                path.write_text(new_content)
                return f"Edited {path}"

            elif name == "list_directory":
                path = Path(args.get("path", ".")).expanduser()
                if not path.is_dir():
                    return f"Error: Not a directory: {path}"
                entries = sorted(path.iterdir())
                lines = []
                for entry in entries[:100]:
                    kind = "dir" if entry.is_dir() else "file"
                    lines.append(f"[{kind}] {entry.name}")
                return "\n".join(lines) if lines else "(empty directory)"

            elif name == "find_files":
                pattern = args["pattern"]
                start = args.get("path", ".")
                matches = glob_mod.glob(
                    os.path.join(start, "**", pattern), recursive=True
                )
                return "\n".join(matches[:50]) if matches else "No files found."

            elif name == "search_in_files":
                import re
                pattern = args["pattern"]
                start = args.get("path", ".")
                include = args.get("include", "*")
                matches = glob_mod.glob(
                    os.path.join(start, "**", include), recursive=True
                )
                results = []
                compiled = re.compile(pattern)
                for fpath in matches[:200]:
                    if not os.path.isfile(fpath):
                        continue
                    try:
                        with open(fpath, "r", errors="replace") as f:
                            for lineno, line in enumerate(f, 1):
                                if compiled.search(line):
                                    results.append(f"{fpath}:{lineno}: {line.rstrip()}")
                                    if len(results) >= 50:
                                        break
                    except (OSError, UnicodeDecodeError):
                        continue
                    if len(results) >= 50:
                        break
                return "\n".join(results) if results else "No matches found."

            elif name == "run_command":
                command = args["command"]
                cwd = args.get("cwd", self.cwd)
                result = subprocess.run(
                    command, shell=True, capture_output=True, text=True,
                    cwd=cwd, timeout=120,
                )
                output = result.stdout
                if result.stderr:
                    output += f"\n[stderr]\n{result.stderr}"
                if result.returncode != 0:
                    output += f"\n[exit code: {result.returncode}]"
                return output[:30000] if output else "(no output)"

            elif name == "create_branch":
                branch = args["name"]
                result = subprocess.run(
                    ["git", "checkout", "-b", branch],
                    capture_output=True, text=True, cwd=self.cwd,
                )
                if result.returncode != 0:
                    return f"Error: {result.stderr}"
                return f"Created and switched to branch: {branch}"

            elif name == "git_commit":
                message = args["message"]
                subprocess.run(
                    ["git", "add", "-A"], capture_output=True, cwd=self.cwd,
                )
                result = subprocess.run(
                    ["git", "commit", "-m", message],
                    capture_output=True, text=True, cwd=self.cwd,
                )
                if result.returncode != 0:
                    return f"Error: {result.stderr}"
                return f"Committed: {message}"

            else:
                return f"Unknown tool: {name}"

        except Exception as exc:
            return f"Tool error ({name}): {exc}"


# ---------------------------------------------------------------------------
# Ollama-powered query engine
# ---------------------------------------------------------------------------


@dataclass
class OllamaQueryEngine:
    """Drop-in replacement for QueryEnginePort that uses real Ollama inference."""

    client: OllamaClient = field(default_factory=OllamaClient)
    tool_executor: BasicToolExecutor = field(default_factory=BasicToolExecutor)
    tools: list[dict] = field(default_factory=lambda: list(OLLAMA_TOOL_SCHEMAS))
    session_id: str = field(default_factory=lambda: uuid4().hex)

    @classmethod
    def create(
        cls,
        host: str | None = None,
        model: str | None = None,
        system_prompt: str | None = None,
        cwd: str = ".",
    ) -> OllamaQueryEngine:
        """Create an engine with custom configuration."""
        return cls(
            client=OllamaClient(host=host, model=model, system_prompt=system_prompt),
            tool_executor=BasicToolExecutor(cwd=cwd),
        )

    def chat(self, prompt: str) -> Generator[StreamToken, None, None]:
        """Simple chat without tools."""
        yield from self.client.chat(prompt)

    def agent_chat(self, prompt: str) -> Generator[StreamToken, None, None]:
        """Agent chat with tools — the primary interface."""
        yield from self.client.agent_chat(prompt, self.tools, self.tool_executor)

    def chat_blocking(self, prompt: str) -> str:
        """Blocking chat that returns the full response as a string."""
        tokens = []
        for token in self.agent_chat(prompt):
            if not token.is_tool_call and not token.is_thinking:
                tokens.append(token.text)
        return "".join(tokens)

    @property
    def usage(self) -> UsageStats:
        return self.client.last_usage

    def list_models(self) -> list[str]:
        return self.client.list_models()

    def reset(self) -> None:
        self.client.reset()
        self.session_id = uuid4().hex
