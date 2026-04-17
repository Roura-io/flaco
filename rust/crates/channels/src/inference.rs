//! Shared inference pipeline — Ollama call + Claude vet layer.
//!
//! Extracted from `socket_mode.rs` so both Slack and terminal (REPL) code
//! paths can reuse the same Ollama call logic, Claude vetting, and
//! vet-decision logging without duplication.

use serde_json::{json, Value};

use crate::gateway::ChannelPersona;

// =====================================================================
// Ollama inference
// =====================================================================

/// Call an Ollama model via its `/api/chat` endpoint with system + user
/// messages. Returns the trimmed assistant content, or an error string
/// describing what went wrong (network, empty-content thinking-model
/// spiral, parse failure, etc.).
///
/// `ollama_url` must be the **base** URL *without* the `/v1` suffix
/// (e.g. `http://localhost:11434`). The function appends `/api/chat`.
pub async fn call_ollama(
    http: &reqwest::Client,
    ollama_url: &str,
    model: &str,
    system_prompt: &str,
    user_message: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_message},
        ],
        "stream": false,
        "options": {
            "num_ctx": 32768,
            "temperature": 0.2,
            "top_p": 0.9,
            "num_predict": 4096
        }
    });

    let resp: Value = http
        .post(format!("{ollama_url}/api/chat"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("Ollama error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Ollama parse error: {e}"))?;

    let content = resp["message"]["content"].as_str().unwrap_or("").trim();
    if !content.is_empty() {
        return Ok(content.to_string());
    }

    // Empty content. Thinking-model style — check if reasoning happened
    // at all so we can distinguish "model never ran" from "model thought
    // itself into a corner without committing to an answer".
    let thinking_len = resp["message"]["thinking"]
        .as_str()
        .map_or(0, |s| s.trim().len());
    let done_reason = resp["done_reason"].as_str().unwrap_or("");
    Err(format!(
        "empty content from {model} (thinking_len={thinking_len}, done_reason={done_reason}). \
         Likely a thinking-model that exhausted num_predict={} before committing to a final \
         answer. Try a higher num_predict or a non-thinking model.",
        4096
    ))
}

// =====================================================================
// Claude vet layer (flacoAi Pro)
// =====================================================================

/// Result of the claude_check vet call.
#[derive(Debug)]
pub enum CheckResult {
    /// Claude approved the local response as-is.
    Approved,
    /// Claude rejected and provided a corrected version.
    Corrected(String),
    /// Claude couldn't be reached. The string carries a typed reason so
    /// the caller can surface it in logs and the unvetted-reply tag:
    ///   - "auth: HTTP 401 ..."   -> ANTHROPIC_API_KEY is wrong or revoked
    ///   - "quota: HTTP 429 ..."  -> rate-limited or over spend cap
    ///   - "network: ..."         -> DNS / TCP / TLS / timeout
    ///   - "parse: ..."           -> Anthropic returned non-JSON or unexpected shape
    ///   - "rejected-no-correction: ..." -> model said REJECTED but didn't give a CORRECTED block
    ///   - "unparseable: ..."     -> model reply didn't start with APPROVED or REJECTED
    ///   - "not-configured"       -> ANTHROPIC_API_KEY env var is unset
    Unavailable(String),
}

/// Vet a local flacoAi response against the real infra context using the
/// Anthropic API (not the CLI — faster, no process spawn, no Pro quota
/// consumption). Returns Approved / Corrected / Unavailable.
///
/// The API key comes from `ANTHROPIC_API_KEY` env var. If unset, returns
/// Unavailable immediately without attempting a call.
///
/// Model choice: claude-haiku-4-5 is the cheapest and fastest — this is a
/// yes/no plus short correction task, not something that needs Opus. At
/// ~$0.0006 per check, 100 checks/day = $1.80/month. If Haiku turns out to
/// be too permissive in practice, flip to claude-sonnet-4-5 with one const
/// change.
pub async fn claude_check(
    http: &reqwest::Client,
    user_question: &str,
    channel_context: &str,
    local_response: &str,
    local_model_name: &str,
    persona: &ChannelPersona,
) -> CheckResult {
    let start = std::time::Instant::now();

    // Defensive guard: if the local model returned empty or whitespace-only
    // output, there's nothing for Haiku to vet.
    if local_response.trim().is_empty() {
        let result = CheckResult::Unavailable("empty-local-response".into());
        log_vet_decision(
            user_question,
            channel_context,
            local_response,
            local_model_name,
            &result,
            start.elapsed(),
        );
        return result;
    }

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            let result = CheckResult::Unavailable("not-configured".into());
            log_vet_decision(user_question, channel_context, local_response, local_model_name, &result, start.elapsed());
            return result;
        }
    };

    const VET_MODEL: &str = "claude-haiku-4-5";
    const INFRA_FACTS: &str = "\
Real infrastructure elGordo runs:
- Pi 5 at 10.0.1.4 (Tailscale 100.70.234.35): Prometheus, Uptime Kuma, Home Assistant, n8n, Grafana
- Mac at 10.0.1.3: Ollama (local LLM)
- UNAS NAS 10.0.1.2: storage
- VPS srv1065212 (Tailscale 100.91.207.7): deadman.sh external watchdog, posts alerts to #home-general
- UDM-SE 10.0.1.1: Verizon Fios gateway
- LAN DNS: Cloudflare Families 1.1.1.3 + Quad9 9.9.9.9 via UDM DHCP (as of 2026-04-15)
This is a SOLO homelab. There is NO team, NO API gateway, NO status page, NO customer.";

    let today = chrono::Local::now().format("%A, %B %-d, %Y").to_string();

    let (context_facts, rules_block) = if persona.channel == "slack-walter" {
        (
            "\
Audience: Walter, elGordo's dad. He has early-onset Alzheimer's. Reads in
plain English, not a developer. Lives in Slack channel #dad-help. Interests:
Yankees, Premier League, meds routine, daily brief.

flacoAi has access to real-time data sources for Walter's interests:
- MLB StatsAPI (live Yankees schedule, scores, pitchers) via the Pi's
  walter-daily-brief n8n workflow
- Fantasy Premier League API
- BBC RSS headlines
- wttr.in for New York weather
If the reply says 'as an AI I don't have access', that is WRONG — flacoAi
DOES have access via those endpoints.",
            format!("\
Vetting rules for Walter channels:
1. If the reply says 'as an AI', 'I don't have real-time access', 'my knowledge is limited to', or similar SaaS-chatbot refusals — REJECT. flacoAi has tools; a lazy refusal is a bug.
2. If the reply quotes a 'next' / 'upcoming' / 'future' event with a date that is BEFORE today ({today}), REJECT — that's a stale training-data date. Correct it to 'I need to check the current schedule'.
3. If the reply invents a Yankees score, pitcher, FPL gameweek number, or news headline without citing the channel context or a tool output, REJECT.
4. If the reply uses support-bot phrasing ('I'm sorry, but', 'I hope this helps', 'Let me know if you have any other questions', 'I'm here to help'), REJECT.
5. If the reply is warm, plain-English, and either grounded in context OR says 'let me check' without fabricating — APPROVE."),
        )
    } else if persona.channel == "terminal" {
        (
            "",
            "\
Vetting rules for terminal:
1. If the reply fabricates facts, URLs, package names, or API details not in the conversation — REJECT.
2. If the reply says 'as an AI model', 'I don\'t have access', 'I recommend checking', 'visit the official website', or similar deflective phrasing — REJECT. The user asked a question, answer it.
3. If web search results were provided in the system prompt but the reply ignores them and says it does not have the information — REJECT. Correct by extracting the relevant facts from the search results.
4. If the reply is grounded, direct, and appropriately answers the question — APPROVE.".to_string(),
        )
    } else {
        (
            INFRA_FACTS,
            "\
Vetting rules for infra channels:
1. If the reply contradicts anything in 'Recent channel activity' (especially a recent deadman alert), REJECT.
2. If the reply invents teams, API gateways, status pages, fixes, timelines, or customers not in context, REJECT.
3. If the reply makes a definitive 'currently X' claim without evidence in context, REJECT.
4. If the reply uses SaaS-support phrasing ('the team', 'deployed a fix', 'status page green', 'no anomalies', 'let me know if anything seems off'), REJECT.
5. If the reply is grounded, factual, and appropriately terse — APPROVE.".to_string(),
        )
    };

    let vet_prompt = format!(
        "You are vetting a response from a local AI (flacoAi/{local_model_name}) before it's \
posted to elGordo (or his dad Walter) in Slack. Your job is to catch hallucinations and \
SaaS-chatbot phrasing that doesn't match reality.

Today's date: {today}.

{context_facts}

Recent channel activity (chronological, most recent LAST):
{channel_context}

The user asked:
{user_question}

flacoAi wants to reply with:
{local_response}

{rules_block}

Respond with EXACTLY one of these formats:

APPROVED

or:

REJECTED: <one line explaining the issue>
CORRECTED: <a better reply grounded in the context above>

Do not write anything else. No preamble, no explanation, no markdown outside CORRECTED."
    );

    let body = json!({
        "model": VET_MODEL,
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": vet_prompt}]
    });

    let resp = match http
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let result = CheckResult::Unavailable(format!("network: {e}"));
            log_vet_decision(user_question, channel_context, local_response, local_model_name, &result, start.elapsed());
            return result;
        }
    };

    if !resp.status().is_success() {
        let code = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        let reason = if code.as_u16() == 401 || code.as_u16() == 403 {
            format!("auth: HTTP {code}: {body_text}")
        } else if code.as_u16() == 429 {
            format!("quota: HTTP {code}: {body_text}")
        } else if code.is_server_error() {
            format!("network: HTTP {code}: {body_text}")
        } else {
            format!("unexpected HTTP {code}: {body_text}")
        };
        let result = CheckResult::Unavailable(reason);
        log_vet_decision(user_question, channel_context, local_response, local_model_name, &result, start.elapsed());
        return result;
    }

    let parsed: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            let result = CheckResult::Unavailable(format!("parse: {e}"));
            log_vet_decision(user_question, channel_context, local_response, local_model_name, &result, start.elapsed());
            return result;
        }
    };

    let text = parsed["content"][0]["text"].as_str().unwrap_or("").trim();

    let result = if text.starts_with("APPROVED") {
        CheckResult::Approved
    } else if text.starts_with("REJECTED") {
        let corrected = text
            .split("CORRECTED:")
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();
        if corrected.is_empty() {
            CheckResult::Unavailable(format!("rejected-no-correction: {text}"))
        } else {
            CheckResult::Corrected(corrected)
        }
    } else {
        CheckResult::Unavailable(format!("unparseable: {text}"))
    };

    log_vet_decision(user_question, channel_context, local_response, local_model_name, &result, start.elapsed());
    result
}

// =====================================================================
// Vet decision logging
// =====================================================================

/// Append a single vet decision to `~/.flaco/vet-decisions.jsonl` for the
/// data-driven Haiku -> Sonnet upgrade decision. Every claude_check call
/// logs a single JSONL line with the question, context (truncated to 2KB
/// to keep the file tractable), local response, verdict classification,
/// corrected reply (if any), error reason (if any), latency, timestamp.
///
/// Absorbs all I/O errors silently — logging failure must NEVER break the
/// Slack reply path. The corpus is opportunistic, not load-bearing.
pub fn log_vet_decision(
    user_question: &str,
    channel_context: &str,
    local_response: &str,
    local_model_name: &str,
    result: &CheckResult,
    latency: std::time::Duration,
) {
    let (verdict, corrected, unavailable_reason) = match result {
        CheckResult::Approved => ("APPROVED", None, None),
        CheckResult::Corrected(c) => ("REJECTED_CORRECTED", Some(c.as_str()), None),
        CheckResult::Unavailable(r) => ("UNAVAILABLE", None, Some(r.as_str())),
    };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let clip = |s: &str, n: usize| -> String {
        if s.len() <= n { s.to_string() } else { format!("{}...", &s[..n]) }
    };

    let entry = json!({
        "ts_epoch": ts,
        "verdict": verdict,
        "latency_ms": latency.as_millis(),
        "local_model": local_model_name,
        "user_question": clip(user_question, 2000),
        "channel_context": clip(channel_context, 2000),
        "local_response": clip(local_response, 2000),
        "corrected_response": corrected,
        "unavailable_reason": unavailable_reason,
    });

    // Best-effort append. ~/.flaco/vet-decisions.jsonl
    let path = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h).join(".flaco/vet-decisions.jsonl"),
        Err(_) => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{entry}");
    }
}

// =====================================================================
// Web search grounding
// =====================================================================

/// Keywords that suggest the user is asking about current events, sports,
/// news, or other time-sensitive topics that the local LLM's stale
/// training data cannot answer accurately.
const SPORTS_KEYWORDS: &[&str] = &[
    "yankees", "mets", "knicks", "rangers", "nets", "giants", "jets",
    "epl", "premier league", "nfl", "nba", "mlb", "nhl",
    "score", "game today", "who won", "who lost", "standings",
    "lineup", "schedule", "pitcher", "roster", "trade",
    "champions league", "world cup", "playoffs", "series",
];

const NEWS_KEYWORDS: &[&str] = &[
    "news", "latest", "what happened", "today", "this week",
    "current", "update on", "breaking", "announced", "election",
    "congress", "president", "senate",
];

const TIME_SENSITIVE_KEYWORDS: &[&str] = &[
    "weather", "when is", "what time", "is it open",
    "stock price", "market", "forecast", "traffic",
];

const EXPLICIT_SEARCH_KEYWORDS: &[&str] = &[
    "search for", "google", "look up", "find out", "search",
];

/// Filler words to strip when building a search query from the user's
/// message, so DuckDuckGo gets a cleaner factual query.
const FILLER_WORDS: &[&str] = &[
    "hey", "hi", "hello", "flaco", "flacoai", "please", "can you",
    "could you", "tell me", "do you know", "i want to know",
    "i need to know", "just", "like", "really", "actually", "basically",
];

/// Detect if a message needs web search grounding.
/// Returns `Some(search_query)` if search is needed, `None` otherwise.
pub fn needs_web_search(text: &str) -> Option<String> {
    let lower = text.to_lowercase();

    let matched = SPORTS_KEYWORDS.iter().any(|kw| lower.contains(kw))
        || NEWS_KEYWORDS.iter().any(|kw| lower.contains(kw))
        || TIME_SENSITIVE_KEYWORDS.iter().any(|kw| lower.contains(kw))
        || EXPLICIT_SEARCH_KEYWORDS.iter().any(|kw| lower.contains(kw));

    if !matched {
        return None;
    }

    // Build a cleaned search query: strip filler words, keep the factual
    // question. Work on the original text to preserve casing of proper
    // nouns, but filter using lowercased comparisons.
    let words: Vec<&str> = text.split_whitespace().collect();
    let cleaned: Vec<&str> = words
        .into_iter()
        .filter(|w| {
            let wl = w.to_lowercase();
            let wl = wl.trim_matches(|c: char| !c.is_alphanumeric());
            !FILLER_WORDS.iter().any(|f| *f == wl)
        })
        .collect();

    let query = if cleaned.is_empty() {
        text.trim().to_string()
    } else {
        cleaned.join(" ")
    };

    Some(query)
}

/// Search DuckDuckGo lite for current information.
///
/// Fetches the HTML-lite results page and parses the top results (title +
/// snippet) using simple string operations. Returns a formatted string
/// with numbered results, or an error message if the search fails.
///
/// Uses a 5-second timeout so a slow or unreachable DDG never blocks the
/// inference pipeline for long.
pub async fn web_search(http: &reqwest::Client, query: &str) -> Result<String, String> {
    let encoded = query
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else if c == ' ' {
                "+".to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect::<String>();

    let resp = http
        .post("https://lite.duckduckgo.com/lite/")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={encoded}"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("web search network error: {e}"))?;

    // DDG lite returns 202 for successful results
    if !resp.status().is_success() && resp.status().as_u16() != 202 {
        return Err(format!("web search HTTP {}", resp.status()));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| format!("web search body error: {e}"))?;

    let results = parse_ddg_lite(&html);

    if results.is_empty() {
        return Err("web search returned no results".into());
    }

    tracing::info!(
        target: "inference",
        query = %query,
        results = results.len(),
        "web search completed"
    );

    let formatted = results
        .iter()
        .enumerate()
        .map(|(i, (title, snippet))| format!("{}. {} -- {}", i + 1, title, snippet))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(formatted)
}

/// Parse DuckDuckGo lite HTML to extract result titles and snippets.
///
/// DDG lite uses a table-based layout. Result links are in
/// `<a class="result-link">` tags, and snippets are in
/// `<td class="result-snippet">` tags. We do simple string scanning
/// rather than pulling in an HTML parser crate.
fn parse_ddg_lite(html: &str) -> Vec<(String, String)> {
    let mut results: Vec<(String, String)> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let mut snippets: Vec<String> = Vec::new();

    // Extract titles from result links: <a class="result-link" ...>TITLE</a>
    let mut search_from = 0;
    while let Some(start) = html[search_from..].find("class=\"result-link\"").or_else(|| html[search_from..].find("class='result-link'")) {
        let abs_start = search_from + start;
        // Find the closing > of the <a> tag
        if let Some(tag_end) = html[abs_start..].find('>') {
            let content_start = abs_start + tag_end + 1;
            // Find the closing </a>
            if let Some(end) = html[content_start..].find("</a>") {
                let title = strip_html_tags(&html[content_start..content_start + end]);
                let title = decode_html_entities(&title).trim().to_string();
                if !title.is_empty() {
                    titles.push(title);
                }
                search_from = content_start + end;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Extract snippets from: <td class="result-snippet">SNIPPET</td>
    search_from = 0;
    while let Some(start) = html[search_from..].find("class=\"result-snippet\"").or_else(|| html[search_from..].find("class='result-snippet'")) {
        let abs_start = search_from + start;
        if let Some(tag_end) = html[abs_start..].find('>') {
            let content_start = abs_start + tag_end + 1;
            if let Some(end) = html[content_start..].find("</td>") {
                let snippet = strip_html_tags(&html[content_start..content_start + end]);
                let snippet = decode_html_entities(&snippet).trim().to_string();
                if !snippet.is_empty() {
                    snippets.push(snippet);
                }
                search_from = content_start + end;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Pair titles with snippets (they come in order)
    let count = titles.len().min(snippets.len()).min(5);
    for i in 0..count {
        results.push((titles[i].clone(), snippets[i].clone()));
    }

    results
}

/// Strip HTML tags from a string, leaving only text content.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

/// Decode common HTML entities.
fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}
