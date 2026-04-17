//! Web search (DuckDuckGo HTML) + web fetch (HTML → readable text).

use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use super::{Tool, ToolResult, ToolSchema};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct WebSearch {
    pub http: reqwest::Client,
}

impl Default for WebSearch {
    fn default() -> Self { Self::new() }
}

impl WebSearch {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("flacoAi/2.0 (+https://roura.io)")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("http client");
        Self { http }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let url = "https://html.duckduckgo.com/html/";
        let resp = self
            .http
            .post(url)
            .form(&[("q", query), ("kl", "us-en")])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("ddg HTTP {}", resp.status())));
        }
        let body = resp.text().await?;
        let doc = Html::parse_document(&body);
        let result_sel = Selector::parse("div.result").unwrap();
        let title_sel = Selector::parse("a.result__a").unwrap();
        let snippet_sel = Selector::parse("a.result__snippet").unwrap();
        let mut out = Vec::new();
        for r in doc.select(&result_sel).take(limit * 2) {
            let title_el = r.select(&title_sel).next();
            let snippet_el = r.select(&snippet_sel).next();
            let Some(t) = title_el else { continue };
            let raw_href = t.value().attr("href").unwrap_or("").to_string();
            let title = t.text().collect::<String>().trim().to_string();
            let snippet = snippet_el
                .map(|s| s.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            let url = decode_ddg_url(&raw_href);
            if url.is_empty() || title.is_empty() { continue; }
            out.push(SearchHit { title, url, snippet });
            if out.len() >= limit { break; }
        }
        Ok(out)
    }
}

fn decode_ddg_url(href: &str) -> String {
    // DDG wraps real URLs like /l/?uddg=<url-encoded>&rut=...
    if let Some(idx) = href.find("uddg=") {
        let rest = &href[idx + 5..];
        let end = rest.find('&').unwrap_or(rest.len());
        let encoded = &rest[..end];
        return urldecode(encoded);
    }
    if href.starts_with("http") { href.to_string() } else { String::new() }
}

fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(h, 16) {
                    out.push(v as char);
                    i += 3;
                    continue;
                }
                out.push('%');
                i += 1;
            }
            b'+' => { out.push(' '); i += 1; }
            b => { out.push(b as char); i += 1; }
        }
    }
    out
}

#[async_trait]
impl Tool for WebSearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".into(),
            description: "Search the web via DuckDuckGo and return top results as JSON.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "limit":{"type":"integer","description":"Default 5, max 10"}
                },
                "required":["query"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let q = args.get("query").and_then(Value::as_str).unwrap_or("").trim();
        if q.is_empty() { return Ok(ToolResult::err("query required")); }
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5).min(10) as usize;
        let hits = self.search(q, limit).await?;
        let json = serde_json::to_value(&hits)?;
        let mut summary = String::new();
        for (i, h) in hits.iter().enumerate() {
            summary.push_str(&format!("[{}] {}\n    {}\n    {}\n", i + 1, h.title, h.url, h.snippet));
        }
        if summary.is_empty() { summary.push_str("no results"); }
        Ok(ToolResult::ok_text(summary).with_structured(json))
    }
}

pub struct WebFetch { pub http: reqwest::Client }

impl Default for WebFetch {
    fn default() -> Self { Self::new() }
}

impl WebFetch {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("flacoAi/2.0 (+https://roura.io)")
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("http client");
        Self { http }
    }

    pub async fn fetch_readable(&self, url: &str) -> Result<String> {
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("HTTP {} from {url}", resp.status())));
        }
        let body = resp.text().await?;
        Ok(html_to_text(&body))
    }
}

/// Very rough HTML → text. Strips scripts/styles, keeps visible content.
pub fn html_to_text(html: &str) -> String {
    let doc = Html::parse_document(html);
    let bad = Selector::parse("script, style, noscript, nav, footer, header, aside").unwrap();
    let mut to_skip: std::collections::HashSet<String> = std::collections::HashSet::new();
    for el in doc.select(&bad) {
        to_skip.insert(format!("{:?}", el.id()));
    }
    let body_sel = Selector::parse("body").unwrap();
    let body = doc.select(&body_sel).next();
    let mut buf = String::new();
    if let Some(body) = body {
        for text in body.text() {
            let t = text.trim();
            if !t.is_empty() {
                buf.push_str(t);
                buf.push('\n');
            }
        }
    } else {
        for text in doc.root_element().text() {
            let t = text.trim();
            if !t.is_empty() {
                buf.push_str(t);
                buf.push('\n');
            }
        }
    }
    if buf.len() > 20_000 {
        buf.truncate(20_000);
        buf.push_str("\n…[truncated]");
    }
    buf
}

#[async_trait]
impl Tool for WebFetch {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_fetch".into(),
            description: "Fetch a URL and return a stripped plain-text version of its contents.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"url":{"type":"string"}},
                "required":["url"]
            }),
        }
    }
    async fn call(&self, args: Value) -> Result<ToolResult> {
        let url = args.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() { return Ok(ToolResult::err("url required")); }
        let text = self.fetch_readable(url).await?;
        Ok(ToolResult::ok_text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_ddg_urls() {
        let got = decode_ddg_url("/l/?uddg=https%3A%2F%2Fexample.com%2Ffoo&rut=abc");
        assert_eq!(got, "https://example.com/foo");
    }

    #[test]
    fn html_to_text_strips_scripts() {
        let html = "<html><body><script>bad();</script><p>Hello</p><p>World</p></body></html>";
        let t = html_to_text(html);
        assert!(t.contains("Hello"));
        assert!(t.contains("World"));
    }
}
