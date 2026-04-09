//! Research & web connectors: Stack Overflow, docs fetch, YouTube transcript, PDF read.

use crate::Connector;
use serde_json::{json, Value};
use std::process::Command;

fn get_str<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// Stack Overflow (HTTP, no auth)
// ---------------------------------------------------------------------------

pub struct StackOverflow;

impl Connector for StackOverflow {
    fn name(&self) -> &str {
        "stack_overflow"
    }
    fn description(&self) -> &str {
        "Search Stack Overflow questions and answers"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "tagged": { "type": "string", "description": "Filter by tag (e.g. rust, python)" }
            },
            "required": ["query"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let query = get_str(input, "query");
        let tagged = get_str(input, "tagged");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        let mut url = format!(
            "https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&q={query}&site=stackoverflow&pagesize=5&filter=withbody"
        );
        if !tagged.is_empty() {
            use std::fmt::Write;
            let _ = write!(url, "&tagged={tagged}");
        }

        let resp: Value = client
            .get(&url)
            .send()
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| e.to_string())?;

        let items = resp["items"].as_array();
        let mut lines = Vec::new();
        if let Some(arr) = items {
            for item in arr {
                let title = item["title"].as_str().unwrap_or("?");
                let score = item["score"].as_i64().unwrap_or(0);
                let answered = item["is_answered"].as_bool().unwrap_or(false);
                let link = item["link"].as_str().unwrap_or("");
                let status = if answered { "answered" } else { "unanswered" };
                lines.push(format!("[{score}] {title} ({status})\n  {link}"));
            }
        }
        if lines.is_empty() {
            Ok("No results found.".into())
        } else {
            Ok(lines.join("\n\n"))
        }
    }
    fn is_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Docs fetch (extends WebFetch concept)
// ---------------------------------------------------------------------------

pub struct DocsFetch;

impl Connector for DocsFetch {
    fn name(&self) -> &str {
        "docs_fetch"
    }
    fn description(&self) -> &str {
        "Fetch and extract text from documentation URLs (man pages, docs.rs, MDN, etc.)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Documentation URL to fetch" }
            },
            "required": ["url"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let url = get_str(input, "url");
        if url.is_empty() {
            return Err("url is required".into());
        }
        // Use curl for simplicity — it handles redirects and TLS
        let output = Command::new("curl")
            .args(["-sL", "--max-time", "15", url])
            .output()
            .map_err(|e| format!("curl failed: {e}"))?;
        if !output.status.success() {
            return Err(format!("failed to fetch {url}"));
        }
        let body = String::from_utf8_lossy(&output.stdout);
        // Strip HTML tags for a rough text extraction
        let text = strip_html_tags(&body);
        // Truncate to avoid overwhelming the model
        let max_chars = 8000;
        if text.len() > max_chars {
            Ok(format!(
                "{}...\n\n[truncated at {max_chars} chars]",
                &text[..max_chars]
            ))
        } else {
            Ok(text)
        }
    }
    fn is_available(&self) -> bool {
        true
    }
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    // Collapse excessive whitespace
    let mut prev_blank = false;
    let mut clean = String::new();
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank {
                clean.push('\n');
                prev_blank = true;
            }
        } else {
            clean.push_str(trimmed);
            clean.push('\n');
            prev_blank = false;
        }
    }
    clean
}

// ---------------------------------------------------------------------------
// YouTube transcript (via yt-dlp if available, otherwise unavailable)
// ---------------------------------------------------------------------------

pub struct YouTubeTranscript;

impl Connector for YouTubeTranscript {
    fn name(&self) -> &str {
        "youtube_transcript"
    }
    fn description(&self) -> &str {
        "Extract transcripts/subtitles from YouTube videos (via yt-dlp)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "YouTube video URL" }
            },
            "required": ["url"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let url = get_str(input, "url");
        // yt-dlp can extract auto-generated subtitles
        let output = Command::new("yt-dlp")
            .args([
                "--write-auto-sub",
                "--sub-lang",
                "en",
                "--skip-download",
                "--sub-format",
                "txt",
                "-o",
                "-",
                "--print",
                "%(subtitles)j",
                url,
            ])
            .output()
            .map_err(|e| format!("yt-dlp failed: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err("failed to extract transcript (subtitles may not be available)".into())
        }
    }
    fn is_available(&self) -> bool {
        Command::new("which")
            .arg("yt-dlp")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// PDF read (local files, via pdftotext or basic extraction)
// ---------------------------------------------------------------------------

pub struct PdfRead;

impl Connector for PdfRead {
    fn name(&self) -> &str {
        "pdf_read"
    }
    fn description(&self) -> &str {
        "Extract text from local PDF files (via pdftotext)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the PDF file" },
                "pages": { "type": "string", "description": "Page range (e.g. '1-5')" }
            },
            "required": ["path"]
        })
    }
    fn execute(&self, input: &Value) -> Result<String, String> {
        let path = get_str(input, "path");
        let pages = get_str(input, "pages");
        let mut args = vec![path, "-"];
        if !pages.is_empty() {
            // pdftotext uses -f first -l last
            let parts: Vec<&str> = pages.split('-').collect();
            if parts.len() == 2 {
                args = vec!["-f", parts[0], "-l", parts[1], path, "-"];
            }
        }
        let output = Command::new("pdftotext")
            .args(&args)
            .output()
            .map_err(|e| format!("pdftotext failed: {e}"))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            let max = 10000;
            if text.len() > max {
                Ok(format!("{}...\n\n[truncated at {max} chars]", &text[..max]))
            } else {
                Ok(text)
            }
        } else {
            Err("pdftotext failed to extract text".into())
        }
    }
    fn is_available(&self) -> bool {
        Command::new("which")
            .arg("pdftotext")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn connectors() -> Vec<Box<dyn Connector>> {
    vec![
        Box::new(StackOverflow),
        Box::new(DocsFetch),
        Box::new(YouTubeTranscript),
        Box::new(PdfRead),
    ]
}
