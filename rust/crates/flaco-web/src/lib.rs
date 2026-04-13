//! flaco-web — unified web UI for flacoAi v2, powered by Roura.io.
//!
//! Single-file Axum app with HTMX on the front end. Every interaction is a
//! form POST that returns an HTML fragment, keeping the JS surface to zero.
//! Routes:
//!
//!   GET  /                — full page
//!   POST /chat            — run a turn through Runtime, return HTML fragment
//!   POST /research        — web research with numbered citations, returns HTML
//!   POST /shortcut        — Siri Shortcut generator
//!   POST /scaffold        — Jira + git scaffold
//!   POST /brief           — morning brief (Jira + memory + Ollama)
//!   GET  /memories        — list memories
//!   POST /memories        — save a memory
//!   GET  /conversations   — list recent conversations

use std::sync::Arc;

use axum::extract::{Form, Path, State};
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
        .route("/brief", post(brief))
        .route("/memories", get(memories_list).post(memories_save))
        .route("/conversations", get(conversations_list))
        .route("/conversations/{id}", get(conversation_detail))
        .route("/tool-log", get(tool_log))
        .route("/new", post(new_conversation))
        .route("/health", get(health))
        .route("/status", get(status))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let tools = state.runtime.tools.names();
    let conv_count = state.runtime.memory.list_conversations(10_000).map(|v| v.len()).unwrap_or(0);
    let mem_count = state.runtime.memory.all_facts(&state.default_user, 10_000).map(|v| v.len()).unwrap_or(0);
    let body = serde_json::json!({
        "version": "flaco-v2",
        "model": state.runtime.ollama.model(),
        "tools": tools,
        "memories": mem_count,
        "conversations": conv_count,
        "default_user": state.default_user,
    });
    axum::Json(body)
}

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

async fn brief(State(state): State<AppState>) -> impl IntoResponse {
    match state.features.morning_brief(&state.default_user).await {
        Ok(b) => {
            let mut html = String::new();
            html.push_str("<div class='msg flaco'><b>flaco • morning brief</b>");
            html.push_str(&format!("<div class='md'>{}</div>", markdown_to_html(&b.markdown)));
            if !b.issues.is_empty() {
                html.push_str("<ul class='cites'>");
                for i in &b.issues {
                    let pri = i.priority.as_deref().unwrap_or("—");
                    html.push_str(&format!(
                        "<li><b>{}</b> · {} · {} · <em>{}</em> — {}</li>",
                        html_escape::encode_text(&i.key),
                        html_escape::encode_text(&i.kind),
                        html_escape::encode_text(&i.status),
                        html_escape::encode_text(pri),
                        html_escape::encode_text(&i.summary),
                    ));
                }
                html.push_str("</ul>");
            }
            html.push_str("</div>");
            Html(html)
        }
        Err(e) => Html(format!("<div class='msg err'>brief failed: {e}</div>")),
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
    // Pull the last 200 calls and aggregate by tool name: count, last-used,
    // and a short preview of the most recent args. The sidebar shows a
    // one-liner per tool, clickable to expand to recent invocations.
    let calls = state.runtime.memory.recent_tool_calls(200).unwrap_or_default();
    let mut html = String::new();
    if calls.is_empty() {
        html.push_str("<div class='empty'>no tool calls yet</div>");
        return Html(html);
    }

    use std::collections::BTreeMap;
    struct Agg { count: usize, last_at: i64, recent_args: Vec<(i64, String)> }
    let mut by_tool: BTreeMap<String, Agg> = BTreeMap::new();
    for (name, args_json, ts) in &calls {
        let e = by_tool.entry(name.clone()).or_insert(Agg { count: 0, last_at: 0, recent_args: Vec::new() });
        e.count += 1;
        if *ts > e.last_at { e.last_at = *ts; }
        if e.recent_args.len() < 5 {
            e.recent_args.push((*ts, args_json.clone()));
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut rows: Vec<(String, Agg)> = by_tool.into_iter().collect();
    rows.sort_by(|a, b| b.1.last_at.cmp(&a.1.last_at));

    html.push_str("<ul class='tools'>");
    for (name, agg) in rows {
        let desc = tool_blurb(&name);
        let ago = humanize_ago(now - agg.last_at);
        html.push_str(&format!(
            "<li><details><summary><div class='tl-row'>\
                <code>{}</code>\
                <span class='tl-count'>×{}</span>\
            </div>\
            <div class='tl-sub'>{} · {}</div></summary>",
            html_escape::encode_text(&name),
            agg.count,
            html_escape::encode_text(desc),
            html_escape::encode_text(&ago),
        ));
        html.push_str("<div class='tl-recent'>");
        for (_ts, a) in &agg.recent_args {
            let preview: String = a.chars().take(180).collect();
            html.push_str(&format!(
                "<pre class='tl-args'>{}</pre>",
                html_escape::encode_text(&preview),
            ));
        }
        html.push_str("</div></details></li>");
    }
    html.push_str("</ul>");
    Html(html)
}

/// One-line description per tool, shown under the name in the sidebar.
fn tool_blurb(name: &str) -> &'static str {
    match name {
        "bash" => "run a shell command",
        "fs_read" => "read a local file",
        "fs_write" => "write a local file",
        "web_search" => "DuckDuckGo search",
        "web_fetch" => "fetch a URL as readable text",
        "weather" => "wttr.in weather snapshot",
        "research" => "search + fetch + cite",
        "jira_create_issue" => "create a Jira issue",
        "github_create_pr" => "create a GitHub PR",
        "scaffold_idea" => "scaffold an idea into code",
        "create_shortcut" => "write a Siri Shortcut file",
        "slack_post" => "post a message to Slack",
        "remember" => "save a fact to memory",
        "recall" => "search unified memory",
        "list_memories" => "list what I remember",
        _ => "tool",
    }
}

fn humanize_ago(secs: i64) -> String {
    if secs < 5 { "just now".into() }
    else if secs < 60 { format!("{secs}s ago") }
    else if secs < 3600 { format!("{}m ago", secs / 60) }
    else if secs < 86_400 { format!("{}h ago", secs / 3600) }
    else { format!("{}d ago", secs / 86_400) }
}

async fn conversations_list(State(state): State<AppState>) -> impl IntoResponse {
    let convs = state.runtime.memory.list_conversations(20).unwrap_or_default();
    let mut html = String::new();
    if convs.is_empty() {
        html.push_str("<div class='empty'>no conversations yet</div>");
        return Html(html);
    }
    html.push_str("<ul class='convs'>");
    for c in convs {
        let title = c.title.clone().unwrap_or_else(|| format!("conversation {}", &c.id[..8]));
        html.push_str(&format!(
            "<li><a href='#chat' hx-get='/conversations/{}' hx-target='#chat-log' hx-swap='innerHTML' \
             onclick=\"document.querySelector('nav button[data-view=chat]').click()\">\
             <b>{}</b><span class='meta'>{} · {}</span></a></li>",
            html_escape::encode_text(&c.id),
            html_escape::encode_text(&title),
            html_escape::encode_text(&c.surface),
            html_escape::encode_text(&c.user_id),
        ));
    }
    html.push_str("</ul>");
    Html(html)
}

async fn conversation_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let msgs = state.runtime.memory.recent_messages(&id, 200).unwrap_or_default();
    if msgs.is_empty() {
        return Html("<div class='msg err'>conversation not found or empty</div>".to_string());
    }
    let mut html = String::new();
    for m in msgs {
        match m.role {
            flaco_core::memory::Role::User => {
                html.push_str(&format!(
                    "<div class='msg user'><b>you</b><p>{}</p></div>",
                    html_escape::encode_text(&m.content)
                ));
            }
            flaco_core::memory::Role::Assistant => {
                html.push_str(&format!(
                    "<div class='msg flaco'><b>flaco</b><div class='md'>{}</div></div>",
                    markdown_to_html(&m.content)
                ));
            }
            flaco_core::memory::Role::Tool => {
                let name = m.tool_name.unwrap_or_else(|| "tool".into());
                let preview: String = m.content.chars().take(80).collect();
                html.push_str(&format!(
                    "<div class='msg tool'><details><summary><b>⚙ {}</b> <span class='tool-preview'>{}</span></summary><pre class='tool-out'>{}</pre></details></div>",
                    html_escape::encode_text(&name),
                    html_escape::encode_text(&preview),
                    html_escape::encode_text(&m.content),
                ));
            }
            flaco_core::memory::Role::System => { /* hide system prompt */ }
        }
    }
    Html(html)
}

/// Render Markdown to HTML via pulldown-cmark with tables, strikethrough,
/// and fenced code blocks. Good enough for LLM output with headings, lists,
/// tables, and inline formatting.
pub fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::with_capacity(md.len() * 2);
    html::push_html(&mut out, parser);
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
        assert!(h.contains("href=\"https://example.com\""));
    }

    #[test]
    fn md_code_block() {
        let h = markdown_to_html("```rust\nlet x = 1;\n```");
        assert!(h.contains("<pre>"));
        assert!(h.contains("let x = 1"));
    }

    #[test]
    fn md_list_and_headings() {
        let h = markdown_to_html("## Focus today\n\n- first\n- second\n");
        assert!(h.contains("<h2>Focus today</h2>"));
        assert!(h.contains("<li>first</li>"));
    }
}
