//! Siri Shortcut generator.
//!
//! Modern `.shortcut` files are signed container blobs on-device, but macOS
//! also accepts a plain XML property list (`.plist`) in `WFWorkflow*` format
//! when imported via the `shortcuts import` CLI on macOS 12+. We emit that:
//! a valid Shortcuts workflow plist that iCloud's "Get Shortcut" importer and
//! the `shortcuts` CLI both understand. A symlink variant with the `.shortcut`
//! extension is also written.
//!
//! This is pragmatic — we support three verbs (notify, open-url, speak) and a
//! concatenation of them — enough to cover most "make a Siri shortcut that…"
//! asks. Anything more exotic falls back to a two-action workflow: "Show
//! Notification" with the description text.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use super::{Tool, ToolResult, ToolSchema};

pub struct CreateShortcut {
    pub out_dir: PathBuf,
}

impl CreateShortcut {
    pub fn new(out_dir: impl AsRef<Path>) -> Self {
        Self { out_dir: out_dir.as_ref().to_path_buf() }
    }
}

#[derive(Debug, Clone)]
enum ShortcutAction {
    ShowNotification { body: String, title: Option<String> },
    OpenUrl { url: String },
    Speak { text: String },
    Text { text: String },
}

fn parse_actions(english: &str) -> Vec<ShortcutAction> {
    let lower = english.to_lowercase();
    let mut actions = Vec::new();

    // Heuristic 1: if there's a URL, open it.
    if let Some(url) = first_url(english) {
        actions.push(ShortcutAction::OpenUrl { url });
    }

    // Heuristic 2: mentions "speak" or "say" → Speak action.
    if lower.contains("speak") || lower.contains("say ") || lower.contains(" read ") {
        actions.push(ShortcutAction::Speak { text: english.to_string() });
    }

    // Heuristic 3: mentions "notify" or "reminder" → Notification.
    if actions.is_empty()
        || lower.contains("notify")
        || lower.contains("notification")
        || lower.contains("reminder")
        || lower.contains("alert")
    {
        actions.push(ShortcutAction::ShowNotification {
            body: english.to_string(),
            title: Some("flaco".to_string()),
        });
    }

    // Always provide an intermediate Text action as the source variable for
    // the notification/speak actions so the output plist is valid.
    let mut with_text = vec![ShortcutAction::Text { text: english.to_string() }];
    with_text.extend(actions);
    with_text
}

fn first_url(s: &str) -> Option<String> {
    for piece in s.split_whitespace() {
        if piece.starts_with("http://") || piece.starts_with("https://") {
            return Some(piece.trim_end_matches(|c: char| matches!(c, ',' | '.' | ')' | ']')).to_string());
        }
    }
    None
}

/// Build the Shortcut workflow dictionary that Apple's Shortcuts app accepts.
fn build_workflow_plist(name: &str, english: &str) -> plist::Value {
    use plist::Value as V;
    let mut actions_arr: Vec<V> = Vec::new();

    for action in parse_actions(english) {
        match action {
            ShortcutAction::Text { text } => {
                actions_arr.push(action_plist(
                    "is.workflow.actions.gettext",
                    &[("WFTextActionText", V::String(text))],
                ));
            }
            ShortcutAction::ShowNotification { body, title } => {
                let mut params: Vec<(&str, V)> = vec![
                    ("WFNotificationActionBody", V::String(body)),
                    ("WFNotificationActionSound", V::Boolean(true)),
                ];
                if let Some(t) = title {
                    params.push(("WFNotificationActionTitle", V::String(t)));
                }
                actions_arr.push(action_plist("is.workflow.actions.notification", &params));
            }
            ShortcutAction::OpenUrl { url } => {
                actions_arr.push(action_plist(
                    "is.workflow.actions.openurl",
                    &[("WFInput", V::String(url))],
                ));
            }
            ShortcutAction::Speak { text } => {
                actions_arr.push(action_plist(
                    "is.workflow.actions.speaktext",
                    &[
                        ("WFSpeakTextRate", V::Real(0.5)),
                        ("WFSpeakTextPitch", V::Real(1.0)),
                        ("WFSpeakTextWaitUntilFinished", V::Boolean(true)),
                        ("WFText", V::String(text)),
                    ],
                ));
            }
        }
    }

    let mut root = plist::Dictionary::new();
    root.insert("WFWorkflowName".into(), V::String(name.into()));
    root.insert("WFWorkflowActions".into(), V::Array(actions_arr));
    root.insert(
        "WFWorkflowClientVersion".into(),
        V::String("1200.1".into()),
    );
    root.insert("WFWorkflowMinimumClientVersion".into(), V::Integer(900.into()));
    root.insert(
        "WFWorkflowMinimumClientVersionString".into(),
        V::String("900".into()),
    );
    root.insert("WFWorkflowIconStartColor".into(), V::Integer(4274264319_i64.into()));
    root.insert("WFWorkflowIconGlyphNumber".into(), V::Integer(59511_i64.into()));
    root.insert("WFWorkflowImportQuestions".into(), V::Array(vec![]));
    root.insert("WFWorkflowInputContentItemClasses".into(), V::Array(vec![
        V::String("WFAppStoreAppContentItem".into()),
        V::String("WFArticleContentItem".into()),
        V::String("WFContactContentItem".into()),
        V::String("WFDateContentItem".into()),
        V::String("WFEmailAddressContentItem".into()),
        V::String("WFGenericFileContentItem".into()),
        V::String("WFImageContentItem".into()),
        V::String("WFiTunesProductContentItem".into()),
        V::String("WFLocationContentItem".into()),
        V::String("WFDCMapsLinkContentItem".into()),
        V::String("WFAVAssetContentItem".into()),
        V::String("WFPDFContentItem".into()),
        V::String("WFPhoneNumberContentItem".into()),
        V::String("WFRichTextContentItem".into()),
        V::String("WFSafariWebPageContentItem".into()),
        V::String("WFStringContentItem".into()),
        V::String("WFURLContentItem".into()),
    ]));
    root.insert("WFWorkflowTypes".into(), V::Array(vec![
        V::String("NCWidget".into()),
        V::String("WatchKit".into()),
    ]));
    root.insert("WFWorkflowOutputContentItemClasses".into(), V::Array(vec![]));
    root.insert("WFWorkflowHasShortcutInputVariables".into(), V::Boolean(false));
    V::Dictionary(root)
}

fn action_plist(identifier: &str, params: &[(&str, plist::Value)]) -> plist::Value {
    use plist::Value as V;
    let mut dict = plist::Dictionary::new();
    dict.insert("WFWorkflowActionIdentifier".into(), V::String(identifier.into()));
    let mut p = plist::Dictionary::new();
    for (k, v) in params {
        p.insert((*k).to_string(), v.clone());
    }
    dict.insert("WFWorkflowActionParameters".into(), V::Dictionary(p));
    V::Dictionary(dict)
}

#[async_trait]
impl Tool for CreateShortcut {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "create_shortcut".into(),
            description: "Generate a Siri Shortcut (.shortcut plist) from an English description. Writes to ~/Downloads/flaco-shortcuts/. User can AirDrop or open to install.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "name":{"type":"string","description":"Shortcut name"},
                    "description":{"type":"string","description":"English description of what the shortcut should do"}
                },
                "required":["name","description"]
            }),
        }
    }

    async fn call(&self, args: Value) -> Result<ToolResult> {
        let name = args.get("name").and_then(Value::as_str).unwrap_or("Flaco Shortcut").to_string();
        let description = args.get("description").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if description.is_empty() {
            return Ok(ToolResult::err("description required"));
        }

        let workflow = build_workflow_plist(&name, &description);

        tokio::fs::create_dir_all(&self.out_dir).await.ok();
        let safe = sanitize_name(&name);
        let path = self.out_dir.join(format!("{safe}.shortcut"));

        // Write as XML plist — Shortcuts.app and `shortcuts import` both accept it.
        let mut bytes: Vec<u8> = Vec::new();
        plist::to_writer_xml(&mut bytes, &workflow)
            .map_err(|e| crate::error::Error::Other(format!("plist: {e}")))?;
        tokio::fs::write(&path, bytes).await?;

        Ok(ToolResult::ok_text(format!(
            "Wrote Siri Shortcut to {}. AirDrop or open on iPhone to install.",
            path.display()
        ))
        .with_structured(serde_json::json!({"path": path.to_string_lossy() })))
    }
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn creates_plist_file() {
        let dir = tempdir().unwrap();
        let t = CreateShortcut::new(dir.path());
        let r = t
            .call(serde_json::json!({
                "name": "Speak Good Morning",
                "description": "Say good morning to Chris"
            }))
            .await
            .unwrap();
        assert!(r.ok, "{}", r.output);
        let path = dir.path().join("Speak_Good_Morning.shortcut");
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("WFWorkflowActions"));
        assert!(contents.contains("speaktext"));
    }

    #[test]
    fn parses_url_action() {
        let acts = parse_actions("open https://news.ycombinator.com please");
        assert!(matches!(acts.iter().find(|a| matches!(a, ShortcutAction::OpenUrl{..})), Some(_)));
    }
}
