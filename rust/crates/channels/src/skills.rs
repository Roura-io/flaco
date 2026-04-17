//! Skill registry — loads markdown-with-YAML-frontmatter skill files from
//! a directory at startup and makes them available for context injection
//! when a user's message matches the skill's domain.
//!
//! Skills are domain knowledge units (e.g., "bun-runtime", "frontend-slides",
//! "continuous-learning") that enrich the LLM prompt with relevant expertise.
//! They follow the same file format as agents (YAML frontmatter + markdown
//! body) but serve a different purpose: agents are invoked for action, skills
//! are injected for knowledge.
//!
//! ### Format
//!
//! ```markdown
//! ---
//! name: skill-name
//! description: One-line description used for relevance matching
//! version: 1.0.0
//! tags: [rust, testing, ci]
//! activation: [keyword match phrases]
//! ---
//! # Skill body (markdown)
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub activation: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    activation: Vec<String>,
}

pub fn parse_skill(content: &str) -> Result<Skill, String> {
    let (yaml, body) = crate::frontmatter::split(content)?;
    let fm: SkillFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| format!("skill YAML parse error: {e}"))?;

    Ok(Skill {
        name: fm.name,
        description: fm.description,
        content: body.trim_start().to_string(),
        version: fm.version,
        tags: fm.tags,
        activation: fm.activation,
    })
}

#[must_use]
pub fn load_skills_from_dir(dir: &Path) -> HashMap<String, Skill> {
    let mut skills = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                target: "skills",
                dir = %dir.display(),
                error = %e,
                "skills dir not readable"
            );
            return skills;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(skill_md) = find_skill_md(&path) {
                load_skill_file(&skill_md, &mut skills);
            }
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) == Some("md") {
            load_skill_file(&path, &mut skills);
        }
    }
    skills
}

fn find_skill_md(dir: &Path) -> Option<std::path::PathBuf> {
    let skill_md = dir.join("SKILL.md");
    if skill_md.exists() {
        return Some(skill_md);
    }
    let readme = dir.join("README.md");
    if readme.exists() {
        return Some(readme);
    }
    None
}

fn load_skill_file(path: &Path, skills: &mut HashMap<String, Skill>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                target: "skills",
                file = %path.display(),
                error = %e,
                "failed to read skill file"
            );
            return;
        }
    };
    match parse_skill(&content) {
        Ok(skill) => {
            tracing::info!(
                target: "skills",
                name = %skill.name,
                tags = ?skill.tags,
                "loaded skill"
            );
            skills.insert(skill.name.clone(), skill);
        }
        Err(e) => {
            tracing::warn!(
                target: "skills",
                file = %path.display(),
                error = %e,
                "failed to parse skill — skipping"
            );
        }
    }
}

pub fn skills_for_message<'a>(
    skills: &'a HashMap<String, Skill>,
    text: &str,
) -> Vec<&'a Skill> {
    let lower = text.to_ascii_lowercase();
    skills
        .values()
        .filter(|s| {
            s.activation
                .iter()
                .any(|a| lower.contains(&a.to_ascii_lowercase()))
                || s.tags
                    .iter()
                    .any(|t| lower.contains(&t.to_ascii_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_skill() {
        let content = "---\nname: test-skill\ndescription: A test skill\n---\n# Body\nHello";
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert!(skill.content.contains("# Body"));
        assert!(skill.tags.is_empty());
    }

    #[test]
    fn parses_skill_with_all_fields() {
        let content = "\
---
name: rust-testing
description: Rust testing patterns and conventions
version: 1.2.0
tags: [rust, testing, ci]
activation: [write a test, add tests, test coverage]
---
# Rust Testing

Use `cargo test` for unit tests.
";
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.name, "rust-testing");
        assert_eq!(skill.version.as_deref(), Some("1.2.0"));
        assert_eq!(skill.tags, vec!["rust", "testing", "ci"]);
        assert_eq!(skill.activation.len(), 3);
        assert!(skill.content.contains("cargo test"));
    }

    #[test]
    fn skill_activation_matching() {
        let mut skills = HashMap::new();
        skills.insert(
            "rust-testing".to_string(),
            Skill {
                name: "rust-testing".to_string(),
                description: "test".to_string(),
                content: String::new(),
                version: None,
                tags: vec!["rust".to_string()],
                activation: vec!["write a test".to_string()],
            },
        );
        assert_eq!(skills_for_message(&skills, "can you write a test for this?").len(), 1);
        assert_eq!(skills_for_message(&skills, "deploy the server").len(), 0);
        assert_eq!(skills_for_message(&skills, "this rust code needs help").len(), 1);
    }
}
