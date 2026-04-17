//! Command registry — loads markdown-with-YAML-frontmatter command files.
//!
//! Commands are workflow scaffolds that guide flacoAi through multi-step
//! processes (feature development, database migration, code review, etc.).
//! Users invoke them via `/command-name` in Slack.
//!
//! ### Format
//!
//! ```markdown
//! ---
//! name: command-name
//! description: Brief description
//! allowed_tools: [bash, fs_read, fs_write, grep, glob]
//! ---
//! # /command-name workflow body
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub content: String,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CommandFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
}

pub fn parse_command(content: &str) -> Result<Command, String> {
    let (yaml, body) = crate::frontmatter::split(content)?;
    let fm: CommandFrontmatter =
        serde_yml::from_str(yaml).map_err(|e| format!("command YAML parse error: {e}"))?;

    Ok(Command {
        name: fm.name,
        description: fm.description,
        content: body.trim_start().to_string(),
        allowed_tools: fm.allowed_tools,
    })
}

#[must_use]
pub fn load_commands_from_dir(dir: &Path) -> HashMap<String, Command> {
    let mut commands = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                target: "commands",
                dir = %dir.display(),
                error = %e,
                "commands dir not readable"
            );
            return commands;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "commands",
                    file = %path.display(),
                    error = %e,
                    "failed to read command file"
                );
                continue;
            }
        };
        match parse_command(&content) {
            Ok(cmd) => {
                tracing::info!(
                    target: "commands",
                    name = %cmd.name,
                    "loaded command"
                );
                commands.insert(cmd.name.clone(), cmd);
            }
            Err(e) => {
                tracing::warn!(
                    target: "commands",
                    file = %path.display(),
                    error = %e,
                    "failed to parse command — skipping"
                );
            }
        }
    }
    commands
}

pub fn command_for_name<'a>(
    commands: &'a HashMap<String, Command>,
    name: &str,
) -> Option<&'a Command> {
    let normalized = name.trim_start_matches('/').to_ascii_lowercase();
    commands.get(&normalized).or_else(|| {
        commands
            .values()
            .find(|c| c.name.eq_ignore_ascii_case(&normalized))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_command() {
        let content = "---\nname: deploy\ndescription: Deploy workflow\n---\n# Steps\n1. Build";
        let cmd = parse_command(content).unwrap();
        assert_eq!(cmd.name, "deploy");
        assert!(cmd.content.contains("# Steps"));
        assert!(cmd.allowed_tools.is_empty());
    }

    #[test]
    fn parses_command_with_tools() {
        let content = "\
---
name: review
description: Code review workflow
allowed_tools: [bash, grep, glob]
---
# Review
Check the diff.
";
        let cmd = parse_command(content).unwrap();
        assert_eq!(cmd.allowed_tools, vec!["bash", "grep", "glob"]);
    }

    #[test]
    fn command_lookup_case_insensitive() {
        let mut commands = HashMap::new();
        commands.insert(
            "deploy".to_string(),
            Command {
                name: "deploy".to_string(),
                description: "test".to_string(),
                content: String::new(),
                allowed_tools: vec![],
            },
        );
        assert!(command_for_name(&commands, "/Deploy").is_some());
        assert!(command_for_name(&commands, "DEPLOY").is_some());
        assert!(command_for_name(&commands, "unknown").is_none());
    }
}
