//! flaco-web — Perplexity-style web UI for flacoAi v2.
//!
//! Single-file Axum app with HTMX on the front end. Every interaction is a
//! form POST that returns an HTML fragment, keeping the JS surface to zero.
//! Routes:
//!
//!   GET  /                — full page
//!   POST /chat            — run a turn through Runtime, return HTML fragment
//!   POST /research        — Perplexity-style research, returns HTML
//!   POST /shortcut        — Siri Shortcut generator
//!   POST /scaffold        — Jira + git scaffold
//!   GET  /memories        — list memories
//!   POST /memories        — save a memory
//!   GET  /conversations   — list recent conversations

use std::sync::Arc;

use axum::extract::{Form, State};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use flaco_core::features::Features;
use flaco_core::runtime::{Runtime, Surface};
use serde::Deserialize;
use tracing::error;

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<Runtime>,
    pub features: Arc<Features>,
    pub default_user: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/chat", post(chat))
        .route("/research", post(research))
        .route("/shortcut", post(shortcut))
        .route("/scaffold", post(scaffold))
        .route("/memories", get(memories_list).post(memories_save))
        .route("/conversations", get(conversations_list))
        .route("/tool-log", get(tool_log))
        .route("/new", post(new_conversation))
        .route("/health", get(health))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

#[derive(Deserialize)]
struct ChatForm { message: String }

async fn chat(State(state): State<AppState>, Form(form): Form<ChatForm>) -> impl IntoResponse {
    let user_text = form.message.trim();
    if user_text.is_empty() {
        return Html("<div class='msg err'>(empty message)</div>".to_string());
    }
    let session = match state.runtime.session(&Surface::Web, &state.default_user) {
        Ok(s) => s,
        Err(e) => {
            error!(?e, "session start failed");
            return Html(format!("<div class='msg err'>session error: {e}</div>"));
        }
    };
    let reply = match state.runtime.handle_turn(&session, user_text, None).await {
        Ok(t) => t,
        Err(e) => format!("error: {e}"),
    };
    Html(format!(
        "<div class='msg user'><b>you</b><p>{}</p></div>\
         <div class='msg flaco'><b>flaco</b><div class='md'>{}</div></div>",
        html_escape::encode_text(user_text),
        markdown_to_html(&reply),
    ))
}

#[derive(Deserialize)]
struct ResearchForm { topic: String }

async fn research(State(state): State<AppState>, Form(form): Form<ResearchForm>) -> impl IntoResponse {
    let topic = form.topic.trim();
    if topic.is_empty() {
        return Html("<div class='msg err'>(empty topic)</div>".to_string());
    }
    let result = match state.features.research(topic).await {
        Ok(r) => r,
        Err(e) => return Html(format!("<div class='msg err'>research failed: {e}</div>")),
    };
    let mut html = String::new();
    html.push_str("<div class='msg user'><b>research</b><p>");
    html.push_str(&html_escape::encode_text(topic));
    html.push_str("</p></div><div class='msg flaco'><b>flaco • research</b>");
    html.push_str(&format!("<div class='md'>{}</div>", markdown_to_html(&result.answer)));
    html.push_str("<ul class='cites'>");
    for c in &result.citations {
        html.push_str(&format!(
            "<li>[{}] <a href='{}' target='_blank' rel='noopener'>{}</a><br><span class='snip'>{}</span></li>",
            c.index,
            html_escape::encode_double_quoted_attribute(&c.url),
            html_escape::encode_text(&c.title),
            html_escape::encode_text(&c.snippet),
        ));
    }
    html.push_str("</ul></div>");
    Html(html)
}

#[derive(Deserialize)]
struct ShortcutForm { name: String, description: String }

async fn shortcut(State(state): State<AppState>, Form(form): Form<ShortcutForm>) -> impl IntoResponse {
    match state.features.create_shortcut(&form.name, &form.description).await {
        Ok(r) => Html(format!("<div class='msg flaco'><b>flaco • shortcut</b><p>{}</p></div>",
                              html_escape::encode_text(&r.output))),
        Err(e) => Html(format!("<div class='msg err'>shortcut failed: {e}</div>")),
    }
}

#[derive(Deserialize)]
struct ScaffoldForm { idea: String, project_key: String }

async fn scaffold(State(state): State<AppState>, Form(form): Form<ScaffoldForm>) -> impl IntoResponse {
    match state.features.scaffold(&form.idea, &form.project_key, None).await {
        Ok(r) => Html(format!("<div class='msg flaco'><b>flaco • scaffold</b><pre>{}</pre></div>",
                              html_escape::encode_text(&r.output))),
        Err(e) => Html(format!("<div class='msg err'>scaffold failed: {e}</div>")),
    }
}

async fn memories_list(State(state): State<AppState>) -> impl IntoResponse {
    let mems = state.runtime.memory.all_facts(&state.default_user, 200).unwrap_or_default();
    let mut html = String::new();
    html.push_str("<ul class='mems'>");
    if mems.is_empty() {
        html.push_str("<li><em>no memories yet</em></li>");
    }
    for m in mems {
        html.push_str(&format!(
            "<li><span class='kind'>{}</span> {}</li>",
            html_escape::encode_text(&m.kind),
            html_escape::encode_text(&m.content),
        ));
    }
    html.push_str("</ul>");
    Html(html)
}

#[derive(Deserialize)]
struct MemForm { content: String, kind: Option<String> }

async fn memories_save(State(state): State<AppState>, Form(form): Form<MemForm>) -> impl IntoResponse {
    let kind = form.kind.as_deref().unwrap_or("fact");
    let _ = state.features.remember(&state.default_user, &form.content, kind);
    memories_list(State(state)).await.into_response()
}

async fn new_conversation(State(state): State<AppState>) -> impl IntoResponse {
    // Force a brand new conversation by inserting a blank separator message
    // with a fresh conversation id.
    let personas = flaco_core::persona::PersonaRegistry::defaults();
    let _ = flaco_core::session::Session::start(
        state.runtime.memory.clone(),
        &personas,
        "web",
        &state.default_user,
        None,
    );
    Html("<div class='msg flaco'><b>flaco</b><p>started a fresh conversation.</p></div>".to_string())
}

async fn tool_log(State(state): State<AppState>) -> impl IntoResponse {
    let calls = state.runtime.memory.recent_tool_calls(20).unwrap_or_default();
    let mut html = String::new();
    html.push_str("<ul class='tools'>");
    if calls.is_empty() {
        html.push_str("<li><em>no tool calls yet</em></li>");
    }
    for (name, args_json, _) in calls {
        let short = if args_json.len() > 120 {
            format!("{}…", &args_json[..120])
        } else {
            args_json
        };
        html.push_str(&format!(
            "<li><code>{}</code> <span class='snip'>{}</span></li>",
            html_escape::encode_text(&name),
            html_escape::encode_text(&short),
        ));
    }
    html.push_str("</ul>");
    Html(html)
}

async fn conversations_list(State(state): State<AppState>) -> impl IntoResponse {
    let convs = state.runtime.memory.list_conversations(20).unwrap_or_default();
    let mut html = String::new();
    html.push_str("<ul class='convs'>");
    for c in convs {
        html.push_str(&format!(
            "<li><b>{}</b> · {} · {}</li>",
            html_escape::encode_text(&c.surface),
            html_escape::encode_text(&c.user_id),
            html_escape::encode_text(c.title.as_deref().unwrap_or(&c.id[..8])),
        ));
    }
    html.push_str("</ul>");
    Html(html)
}

/// Laughably small Markdown → HTML — enough to render bold, italics, code,
/// headings, paragraphs, and links. Good enough for LLM responses in the
/// hackathon demo; we can swap in pulldown-cmark later.
pub fn markdown_to_html(md: &str) -> String {
    let mut html = String::new();
    let mut in_code = false;
    for raw_line in md.lines() {
        let line = raw_line.trim_end();
        if line.starts_with("```") {
            if in_code {
                html.push_str("</code></pre>");
                in_code = false;
            } else {
                html.push_str("<pre><code>");
                in_code = true;
            }
            continue;
        }
        if in_code {
            html.push_str(&html_escape::encode_text(line));
            html.push('\n');
            continue;
        }
        if line.is_empty() { html.push_str("<br>"); continue; }
        if let Some(rest) = line.strip_prefix("### ") {
            html.push_str(&format!("<h3>{}</h3>", html_escape::encode_text(rest)));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>", html_escape::encode_text(rest)));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            html.push_str(&format!("<h1>{}</h1>", html_escape::encode_text(rest)));
            continue;
        }
        html.push_str("<p>");
        html.push_str(&inline_md(line));
        html.push_str("</p>");
    }
    if in_code { html.push_str("</code></pre>"); }
    html
}

fn inline_md(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next();
            let mut inner = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '*' { chars.next(); if chars.peek() == Some(&'*') { chars.next(); break; } inner.push('*'); }
                else { inner.push(nc); chars.next(); }
            }
            out.push_str("<strong>");
            out.push_str(&html_escape::encode_text(&inner));
            out.push_str("</strong>");
            continue;
        }
        if c == '`' {
            let mut inner = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '`' { chars.next(); break; }
                inner.push(nc); chars.next();
            }
            out.push_str("<code>");
            out.push_str(&html_escape::encode_text(&inner));
            out.push_str("</code>");
            continue;
        }
        if c == '[' {
            // [text](url)
            let mut text = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == ']' { chars.next(); break; }
                text.push(nc); chars.next();
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                let mut url = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc == ')' { chars.next(); break; }
                    url.push(nc); chars.next();
                }
                out.push_str(&format!(
                    "<a href='{}' target='_blank' rel='noopener'>{}</a>",
                    html_escape::encode_double_quoted_attribute(&url),
                    html_escape::encode_text(&text)
                ));
                continue;
            }
            out.push('[');
            out.push_str(&html_escape::encode_text(&text));
            out.push(']');
            continue;
        }
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        out.push_str(&html_escape::encode_text(s));
    }
    out
}

const INDEX_HTML: &str = include_str!("../templates/index.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_basic_render() {
        let h = markdown_to_html("# Title\n\nHello **world** with `code` and [link](https://example.com)");
        assert!(h.contains("<h1>Title</h1>"));
        assert!(h.contains("<strong>world</strong>"));
        assert!(h.contains("<code>code</code>"));
        assert!(h.contains("href='https://example.com'"));
    }

    #[test]
    fn md_code_block() {
        let h = markdown_to_html("```\nlet x = 1;\n```");
        assert!(h.contains("<pre><code>"));
        assert!(h.contains("let x = 1;"));
    }
}
