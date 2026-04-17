//! Rule registry — language and domain rules loaded from markdown files.
//!
//! Rules are always-active guidelines that apply when working with specific
//! file types or domains. Unlike skills (injected on keyword match) or agents
//! (invoked by command), rules are matched by file path patterns and injected
//! automatically whenever relevant files are mentioned.
//!
//! ### Format
//!
//! ```markdown
//! ---
//! name: rust-coding-style
//! description: Rust coding conventions
//! language: rust
//! paths: ["**/*.rs"]
//! category: coding-style
//! ---
//! # Rust Coding Style
//! ...
//! ```
//!
//! ### Directory structure
//!
//! ```text
//! rules/
//!   common/           # Language-agnostic rules
//!     coding-style.md
//!     security.md
//!     testing.md
//!   rust/
//!     coding-style.md # Extends common/coding-style.md
//!     security.md
//!     testing.md
//!   python/
//!     ...
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Rule {
    pub name: String,
    pub description: String,
    pub content: String,
    pub language: Option<String>,
    pub paths: Vec<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuleFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    category: Option<String>,
}

pub fn parse_rule(content: &str) -> Result<Rule, String> {
    let (yaml, body) = crate::frontmatter::split(content)?;
    let fm: RuleFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| format!("rule YAML parse error: {e}"))?;

    Ok(Rule {
        name: fm.name,
        description: fm.description,
        content: body.trim_start().to_string(),
        language: fm.language,
        paths: fm.paths,
        category: fm.category,
    })
}

#[must_use]
pub fn load_rules_from_dir(dir: &Path) -> HashMap<String, Rule> {
    let mut rules = HashMap::new();
    load_rules_recursive(dir, &mut rules);
    rules
}

fn load_rules_recursive(dir: &Path, rules: &mut HashMap<String, Rule>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_rules_recursive(&path, rules);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "rules",
                    file = %path.display(),
                    error = %e,
                    "failed to read rule file"
                );
                continue;
            }
        };
        match parse_rule(&content) {
            Ok(rule) => {
                tracing::info!(
                    target: "rules",
                    name = %rule.name,
                    language = ?rule.language,
                    paths = ?rule.paths,
                    "loaded rule"
                );
                rules.insert(rule.name.clone(), rule);
            }
            Err(e) => {
                tracing::warn!(
                    target: "rules",
                    file = %path.display(),
                    error = %e,
                    "failed to parse rule — skipping"
                );
            }
        }
    }
}

pub fn rules_for_language<'a>(
    rules: &'a HashMap<String, Rule>,
    language: &str,
) -> Vec<&'a Rule> {
    let lang = language.to_ascii_lowercase();
    rules
        .values()
        .filter(|r| {
            r.language
                .as_ref()
                .is_some_and(|l| l.eq_ignore_ascii_case(&lang))
                || r.name.starts_with(&format!("{lang}-"))
        })
        .collect()
}

pub fn rules_for_path<'a>(
    rules: &'a HashMap<String, Rule>,
    file_path: &str,
) -> Vec<&'a Rule> {
    rules
        .values()
        .filter(|r| {
            r.paths.iter().any(|pattern| {
                if pattern.starts_with("**/*.") {
                    let ext = &pattern[4..];
                    file_path.ends_with(ext)
                } else if pattern.starts_with("*.") {
                    let ext = &pattern[1..];
                    file_path.ends_with(ext)
                } else {
                    file_path.contains(pattern)
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_rule() {
        let content = "---\nname: test-rule\ndescription: A test rule\n---\n# Body";
        let rule = parse_rule(content).unwrap();
        assert_eq!(rule.name, "test-rule");
        assert!(rule.paths.is_empty());
        assert!(rule.language.is_none());
    }

    #[test]
    fn parses_rule_with_all_fields() {
        let content = "\
---
name: rust-security
description: Rust security rules
language: rust
paths: [\"**/*.rs\"]
category: security
---
# Rust Security
Never use unsafe without a SAFETY comment.
";
        let rule = parse_rule(content).unwrap();
        assert_eq!(rule.language.as_deref(), Some("rust"));
        assert_eq!(rule.paths, vec!["**/*.rs"]);
        assert_eq!(rule.category.as_deref(), Some("security"));
    }

    #[test]
    fn rules_for_language_filters_correctly() {
        let mut rules = HashMap::new();
        rules.insert(
            "rust-style".to_string(),
            Rule {
                name: "rust-style".to_string(),
                description: "test".to_string(),
                content: String::new(),
                language: Some("rust".to_string()),
                paths: vec![],
                category: None,
            },
        );
        rules.insert(
            "python-style".to_string(),
            Rule {
                name: "python-style".to_string(),
                description: "test".to_string(),
                content: String::new(),
                language: Some("python".to_string()),
                paths: vec![],
                category: None,
            },
        );
        assert_eq!(rules_for_language(&rules, "rust").len(), 1);
        assert_eq!(rules_for_language(&rules, "Python").len(), 1);
        assert_eq!(rules_for_language(&rules, "go").len(), 0);
    }

    #[test]
    fn rules_for_path_matches_extensions() {
        let mut rules = HashMap::new();
        rules.insert(
            "rust-rules".to_string(),
            Rule {
                name: "rust-rules".to_string(),
                description: "test".to_string(),
                content: String::new(),
                language: None,
                paths: vec!["**/*.rs".to_string()],
                category: None,
            },
        );
        assert_eq!(rules_for_path(&rules, "src/main.rs").len(), 1);
        assert_eq!(rules_for_path(&rules, "src/main.py").len(), 0);
    }
}
