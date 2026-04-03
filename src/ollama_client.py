"""Ollama API client with streaming and tool-calling support.

Adapted from flacoAi v1.0.0 to power the Python porting workspace
with real Ollama local model integration.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from typing import AsyncGenerator, Generator

import httpx


# ---------------------------------------------------------------------------
# Configuration defaults (overridable via env vars)
# ---------------------------------------------------------------------------

DEFAULT_OLLAMA_HOST = "http://localhost:11434"
DEFAULT_OLLAMA_MODEL = "qwen3:30b-a3b"
DEFAULT_SYSTEM_PROMPT = (
    "You are flacoAi, a local AI coding agent powered by Ollama. "
    "You help with code, file operations, system tasks, and infrastructure management. "
    "Be concise and precise. When using tools, explain what you're doing."
)


def get_ollama_host() -> str:
    return os.environ.get("FLACO_OLLAMA_HOST", os.environ.get("OLLAMA_HOST", DEFAULT_OLLAMA_HOST))


def get_ollama_model() -> str:
    return os.environ.get("FLACO_MODEL", os.environ.get("OLLAMA_MODEL", DEFAULT_OLLAMA_MODEL))


def get_system_prompt() -> str:
    return os.environ.get("FLACO_SYSTEM_PROMPT", DEFAULT_SYSTEM_PROMPT)


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------


class OllamaError(Exception):
    """Friendly error from the Ollama client."""


@dataclass
class StreamToken:
    """A single token from a streaming response."""
    text: str
    is_thinking: bool = False
    is_tool_call: bool = False
    tool_name: str = ""
    tool_args: dict | None = None
    tool_result: str = ""


@dataclass
class UsageStats:
    """Token usage from the last chat call."""
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_duration_ms: float = 0.0

    @property
    def total_tokens(self) -> int:
        return self.prompt_tokens + self.completion_tokens

    @property
    def tokens_per_sec(self) -> float:
        if self.total_duration_ms <= 0 or self.completion_tokens == 0:
            return 0.0
        eval_secs = self.total_duration_ms / 1000
        return self.completion_tokens / eval_secs if eval_secs > 0 else 0.0


# ---------------------------------------------------------------------------
# Synchronous client
# ---------------------------------------------------------------------------


class OllamaClient:
    """Synchronous Ollama client with agent loop support."""

    def __init__(
        self,
        host: str | None = None,
        model: str | None = None,
        system_prompt: str | None = None,
    ) -> None:
        self.host = (host or get_ollama_host()).rstrip("/")
        self.model = model or get_ollama_model()
        self.messages: list[dict] = [
            {"role": "system", "content": system_prompt or get_system_prompt()},
        ]
        self.last_usage = UsageStats()
        self.max_context_chars = 30000

    def reset(self, system_prompt: str | None = None) -> None:
        """Clear message history, keeping system prompt."""
        self.messages = [
            {"role": "system", "content": system_prompt or get_system_prompt()},
        ]
        self.last_usage = UsageStats()

    def _trim_context(self) -> None:
        """Trim old messages to stay within context limits."""
        total = sum(len(m.get("content", "")) for m in self.messages)
        if total <= self.max_context_chars:
            return

        keep_recent = 10
        if len(self.messages) <= keep_recent + 1:
            return

        for i in range(1, len(self.messages) - keep_recent):
            msg = self.messages[i]
            content = msg.get("content", "")
            if msg.get("role") == "tool" and len(content) > 200:
                first_line = content.splitlines()[0][:150] if content else ""
                self.messages[i] = {"role": "tool", "content": f"{first_line}… (trimmed)"}
            elif msg.get("role") == "assistant" and len(content) > 500:
                self.messages[i] = {
                    "role": "assistant",
                    "content": content[:300] + "… (trimmed)",
                }

    def _stream_chat(self, payload: dict) -> Generator[dict, None, None]:
        """Stream a single /api/chat call, yielding raw chunks."""
        with httpx.Client(timeout=600.0) as http:
            with http.stream(
                "POST",
                f"{self.host}/api/chat",
                json=payload,
            ) as resp:
                if resp.status_code == 404:
                    body = resp.read().decode()
                    try:
                        msg = json.loads(body).get("error", body)
                    except json.JSONDecodeError:
                        msg = body
                    raise OllamaError(f"Model not found: {msg}")
                if resp.status_code >= 500:
                    body = resp.read().decode()
                    try:
                        msg = json.loads(body).get("error", body)
                    except json.JSONDecodeError:
                        msg = body
                    raise OllamaError(f"Ollama server error: {msg}")
                resp.raise_for_status()

                for line in resp.iter_lines():
                    if not line:
                        continue
                    chunk = json.loads(line)
                    if "error" in chunk:
                        raise OllamaError(f"Ollama error: {chunk['error']}")
                    yield chunk

    def chat(self, user_message: str) -> Generator[StreamToken, None, None]:
        """Send a message and stream back tokens. No tool calling."""
        self.messages.append({"role": "user", "content": user_message})

        payload = {
            "model": self.model,
            "messages": self.messages,
            "stream": True,
        }

        full_response: list[str] = []

        try:
            for chunk in self._stream_chat(payload):
                msg_data = chunk.get("message", {})

                thinking = msg_data.get("thinking", "")
                if thinking:
                    yield StreamToken(text=thinking, is_thinking=True)

                token = msg_data.get("content", "")
                if token:
                    full_response.append(token)
                    yield StreamToken(text=token)

                if chunk.get("done"):
                    self.last_usage = UsageStats(
                        prompt_tokens=chunk.get("prompt_eval_count", 0),
                        completion_tokens=chunk.get("eval_count", 0),
                        total_duration_ms=chunk.get("eval_duration", 0) / 1_000_000,
                    )

        except (httpx.ConnectError, httpx.TimeoutException, OllamaError, httpx.HTTPStatusError):
            self.messages.pop()
            raise
        else:
            self.messages.append(
                {"role": "assistant", "content": "".join(full_response)}
            )

    def agent_chat(
        self,
        user_message: str,
        tools: list[dict],
        tool_executor,
    ) -> Generator[StreamToken, None, None]:
        """Agent loop: send a message, let the model call tools, repeat.

        Yields StreamTokens for thinking, content, and tool call events.
        """
        self.messages.append({"role": "user", "content": user_message})
        max_iterations = 25

        try:
            for _ in range(max_iterations):
                self._trim_context()
                payload = {
                    "model": self.model,
                    "messages": self.messages,
                    "tools": tools,
                    "stream": True,
                }

                full_content: list[str] = []
                tool_calls: list[dict] = []

                for chunk in self._stream_chat(payload):
                    msg_data = chunk.get("message", {})

                    thinking = msg_data.get("thinking", "")
                    if thinking:
                        yield StreamToken(text=thinking, is_thinking=True)

                    token = msg_data.get("content", "")
                    if token:
                        full_content.append(token)
                        yield StreamToken(text=token)

                    if "tool_calls" in msg_data:
                        tool_calls.extend(msg_data["tool_calls"])

                    if chunk.get("done"):
                        self.last_usage = UsageStats(
                            prompt_tokens=chunk.get("prompt_eval_count", 0),
                            completion_tokens=chunk.get("eval_count", 0),
                            total_duration_ms=chunk.get("eval_duration", 0) / 1_000_000,
                        )

                # Save the assistant message
                assistant_msg: dict = {"role": "assistant", "content": "".join(full_content)}
                if tool_calls:
                    assistant_msg["tool_calls"] = tool_calls
                self.messages.append(assistant_msg)

                # If no tool calls, we're done
                if not tool_calls:
                    break

                # Execute each tool call and feed results back
                for tc in tool_calls:
                    func = tc.get("function", {})
                    name = func.get("name", "unknown")
                    args = func.get("arguments", {})

                    yield StreamToken(
                        text="",
                        is_tool_call=True,
                        tool_name=name,
                        tool_args=args,
                    )

                    result = tool_executor.execute(name, args)

                    yield StreamToken(
                        text="",
                        is_tool_call=True,
                        tool_name=name,
                        tool_args=args,
                        tool_result=result,
                    )

                    self.messages.append({"role": "tool", "content": result})

        except httpx.ConnectError:
            self.messages.pop()
            raise OllamaError(
                f"Cannot connect to Ollama at {self.host}. Is it running?"
            )
        except httpx.TimeoutException:
            self.messages.pop()
            raise OllamaError(
                "Request timed out. The model may be loading or the server is overloaded."
            )
        except OllamaError:
            self.messages.pop()
            raise
        except httpx.HTTPStatusError as exc:
            self.messages.pop()
            raise OllamaError(f"HTTP {exc.response.status_code}: {exc.response.text}")

    def list_models(self) -> list[str]:
        """Return model names available on the Ollama server."""
        with httpx.Client(timeout=10.0) as http:
            resp = http.get(f"{self.host}/api/tags")
            resp.raise_for_status()
            data = resp.json()
        return [m["name"] for m in data.get("models", [])]


# ---------------------------------------------------------------------------
# Async client
# ---------------------------------------------------------------------------


class AsyncOllamaClient:
    """Async Ollama client with agent loop support.

    Mirrors OllamaClient but uses httpx.AsyncClient for non-blocking I/O.
    """

    def __init__(
        self,
        host: str | None = None,
        model: str | None = None,
        system_prompt: str | None = None,
    ) -> None:
        self.host = (host or get_ollama_host()).rstrip("/")
        self.model = model or get_ollama_model()
        self.messages: list[dict] = [
            {"role": "system", "content": system_prompt or get_system_prompt()},
        ]
        self.last_usage = UsageStats()
        self.max_context_chars = 30000

    def reset(self, system_prompt: str | None = None) -> None:
        """Clear message history, keeping system prompt."""
        self.messages = [
            {"role": "system", "content": system_prompt or get_system_prompt()},
        ]
        self.last_usage = UsageStats()

    def _trim_context(self) -> None:
        """Trim old messages to stay within context limits."""
        total = sum(len(m.get("content", "")) for m in self.messages)
        if total <= self.max_context_chars:
            return
        keep_recent = 10
        if len(self.messages) <= keep_recent + 1:
            return
        for i in range(1, len(self.messages) - keep_recent):
            msg = self.messages[i]
            content = msg.get("content", "")
            if msg.get("role") == "tool" and len(content) > 200:
                first_line = content.splitlines()[0][:150] if content else ""
                self.messages[i] = {"role": "tool", "content": f"{first_line}… (trimmed)"}
            elif msg.get("role") == "assistant" and len(content) > 500:
                self.messages[i] = {
                    "role": "assistant",
                    "content": content[:300] + "… (trimmed)",
                }

    async def agent_chat(
        self,
        user_message: str,
        tools: list[dict],
        tool_executor,
    ) -> AsyncGenerator[StreamToken, None]:
        """Async agent loop: send message, call tools, repeat."""
        import asyncio

        self.messages.append({"role": "user", "content": user_message})
        max_iterations = 25

        try:
            async with httpx.AsyncClient(timeout=600.0) as http:
                for _ in range(max_iterations):
                    self._trim_context()
                    payload = {
                        "model": self.model,
                        "messages": self.messages,
                        "tools": tools,
                        "stream": True,
                    }

                    full_content: list[str] = []
                    tool_calls: list[dict] = []

                    async with http.stream(
                        "POST",
                        f"{self.host}/api/chat",
                        json=payload,
                    ) as resp:
                        resp.raise_for_status()
                        async for line in resp.aiter_lines():
                            if not line:
                                continue
                            chunk = json.loads(line)
                            if "error" in chunk:
                                raise OllamaError(f"Ollama error: {chunk['error']}")

                            msg_data = chunk.get("message", {})

                            thinking = msg_data.get("thinking", "")
                            if thinking:
                                yield StreamToken(text=thinking, is_thinking=True)

                            token = msg_data.get("content", "")
                            if token:
                                full_content.append(token)
                                yield StreamToken(text=token)

                            if "tool_calls" in msg_data:
                                tool_calls.extend(msg_data["tool_calls"])

                            if chunk.get("done"):
                                self.last_usage = UsageStats(
                                    prompt_tokens=chunk.get("prompt_eval_count", 0),
                                    completion_tokens=chunk.get("eval_count", 0),
                                    total_duration_ms=chunk.get("eval_duration", 0) / 1_000_000,
                                )

                    # Save assistant message
                    assistant_msg: dict = {"role": "assistant", "content": "".join(full_content)}
                    if tool_calls:
                        assistant_msg["tool_calls"] = tool_calls
                    self.messages.append(assistant_msg)

                    if not tool_calls:
                        break

                    for tc in tool_calls:
                        func = tc.get("function", {})
                        name = func.get("name", "unknown")
                        args = func.get("arguments", {})

                        yield StreamToken(
                            text="", is_tool_call=True,
                            tool_name=name, tool_args=args,
                        )

                        result = await asyncio.to_thread(
                            tool_executor.execute, name, args
                        )

                        yield StreamToken(
                            text="", is_tool_call=True,
                            tool_name=name, tool_args=args,
                            tool_result=result,
                        )

                        self.messages.append({"role": "tool", "content": result})

        except httpx.ConnectError:
            self.messages.pop()
            raise OllamaError(
                f"Cannot connect to Ollama at {self.host}. Is it running?"
            )
        except httpx.TimeoutException:
            self.messages.pop()
            raise OllamaError(
                "Request timed out. The model may be loading."
            )

    async def chat_simple(self, user_message: str) -> str:
        """Simple non-streaming chat. Returns the full response text."""
        self.messages.append({"role": "user", "content": user_message})
        payload = {
            "model": self.model,
            "messages": self.messages,
            "stream": False,
        }
        async with httpx.AsyncClient(timeout=600.0) as http:
            resp = await http.post(f"{self.host}/api/chat", json=payload)
            resp.raise_for_status()
            data = resp.json()

        content = data.get("message", {}).get("content", "")
        self.messages.append({"role": "assistant", "content": content})
        return content
