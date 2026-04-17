//! Shared YAML frontmatter parser used by agents, skills, commands, and rules.
//!
//! All four registries share the same file format:
//! ```text
//! ---
//! <yaml frontmatter>
//! ---
//! <markdown body>
//! ```

/// Split a markdown-with-YAML-frontmatter document into `(yaml, body)`.
///
/// Handles UTF-8 BOM, `\n` / `\r\n` line endings, and end-of-file
/// closers (no trailing newline after `---`).
pub fn split(content: &str) -> Result<(&str, &str), String> {
    let trimmed = content.trim_start_matches('\u{FEFF}').trim_start();

    let after_open = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
        .ok_or_else(|| "missing frontmatter opener (first line must be `---`)".to_string())?;

    split_on_closer(after_open)
        .ok_or_else(|| "missing frontmatter closer (no `---` on its own line)".to_string())
}

fn split_on_closer(after_open: &str) -> Option<(&str, &str)> {
    if let Some((y, b)) = after_open.split_once("\n---\n") {
        return Some((y, b));
    }
    if let Some((y, b)) = after_open.split_once("\n---\r\n") {
        return Some((y, b));
    }
    if let Some((y, b)) = after_open.split_once("\n---") {
        if b.is_empty() || b.starts_with('\n') || b.starts_with('\r') {
            return Some((y, b.trim_start_matches(['\r', '\n'])));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_standard_frontmatter() {
        let (yaml, body) = split("---\nname: test\n---\nbody here").unwrap();
        assert_eq!(yaml, "name: test");
        assert_eq!(body, "body here");
    }

    #[test]
    fn handles_bom() {
        let content = "\u{FEFF}---\nname: bom\n---\nok";
        let (yaml, _) = split(content).unwrap();
        assert_eq!(yaml, "name: bom");
    }

    #[test]
    fn handles_no_trailing_newline() {
        let (yaml, body) = split("---\nname: eof\n---").unwrap();
        assert_eq!(yaml, "name: eof");
        assert!(body.is_empty());
    }

    #[test]
    fn rejects_missing_opener() {
        assert!(split("no frontmatter here").is_err());
    }

    #[test]
    fn rejects_missing_closer() {
        assert!(split("---\nname: unclosed\n").is_err());
    }
}
