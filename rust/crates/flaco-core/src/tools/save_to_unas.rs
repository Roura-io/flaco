//! `save_to_unas` — first-class save tool that writes user-generated
//! artifacts to the UNAS shared drive under a per-user folder, so the
//! output is instantly accessible from any device on the network
//! (iPad Files.app, Mac Finder, Walter's phone, etc.).
//!
//! Directory scheme inside the `Roura.io` shared drive:
//!
//!   Roura.io/
//!     cjroura/         ← Chris's folder
//!       flaco/
//!         shortcuts/   ← /shortcut output
//!         research/    ← /research output (+ .md + cited sources)
//!         scaffolds/   ← /scaffold output
//!         notes/       ← /remember → markdown notes
//!     wroura/          ← Walter's folder
//!       flaco/
//!         …same subdirs…
//!     <other-user>/
//!
//! Why this design:
//!
//! 1. **One SMB mount, many users.** A single `flaco` service user in
//!    UniFi Identity holds the SMB credential that mac-server uses to
//!    mount `/Volumes/Roura.io`. Per-user attribution happens at the
//!    filesystem path layer, not the auth layer. Simpler ops, clean
//!    audit: the directory owner tells you who saved it.
//!
//! 2. **Walter friendly.** The path scheme means Walter's research,
//!    shortcut files, and memory notes all land under a single
//!    folder he (or Chris on his behalf) can access from any Apple
//!    device without SCP gymnastics.
//!
//! 3. **Configurable via env.** The mount path, share name, and
//!    user→folder mapping all come from env vars with safe defaults.
//!    No hardcoded usernames in the binary.
//!
//! Environment variables this tool honors:
//!
//! - `FLACO_UNAS_MOUNT` — mount path of the shared drive on mac-server.
//!   Default: `/Volumes/Roura.io`.
//! - `FLACO_UNAS_USER_MAP` — comma-separated `slack_user_id=folder`
//!   pairs mapping a canonical user id to a UNAS subfolder name.
//!   Example: `chris=cjroura,U0AS9PLFLCD=wroura`. Case-sensitive.
//! - `FLACO_UNAS_DEFAULT_FOLDER` — fallback folder used when the
//!   current user isn't in the user map. Default: `shared`.
//!
//! The tool NEVER auto-mounts. If `/Volumes/Roura.io` isn't mounted,
//! the call returns a clear, actionable error — mounting is an ops
//! concern that happens once via launchd or a small shell helper,
//! not per-tool-call.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

use super::{Tool, ToolResult, ToolSchema};
use crate::error::Result;

pub struct SaveToUnas;

#[async_trait]
impl Tool for SaveToUnas {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "save_to_unas".into(),
            description: "Save a text or markdown artifact to the user's UNAS \
                folder so it's instantly accessible from any Apple device on \
                the network. Use this for research writeups, drafted emails, \
                meeting notes, scaffolded READMEs, or anything the user asked \
                you to 'save' or 'put in my unas'. The file lands under \
                Roura.io/<user-folder>/flaco/<category>/<filename>. Always \
                returns a Finder-friendly path the user can click."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string",
                        "description": "Canonical id of the user requesting \
                            the save. Usually the Slack user id (Uxxxxxx) or \
                            'chris' / 'walter' etc. for non-Slack surfaces. \
                            The tool maps this to a UNAS subfolder via \
                            FLACO_UNAS_USER_MAP — never invent one yourself."
                    },
                    "category": {
                        "type": "string",
                        "enum": ["shortcuts", "research", "scaffolds", "notes", "drafts", "other"],
                        "description": "Which subfolder under the user's flaco/ \
                            directory this belongs in. Pick the most specific \
                            one. 'other' is the escape hatch."
                    },
                    "filename": {
                        "type": "string",
                        "description": "File name with extension (e.g. \
                            'yankees_trade_rumors.md', 'meeting_notes.txt'). \
                            Will be slugged to remove path separators. \
                            No leading slashes."
                    },
                    "content": {
                        "type": "string",
                        "description": "The full text/markdown body to write. \
                            Binary data is NOT supported — base64-decode \
                            client-side and pass the text form."
                    }
                },
                "required": ["user_id", "category", "filename", "content"]
            }),
        }
    }

    async fn call(&self, args: Value) -> Result<ToolResult> {
        let user_id = args.get("user_id").and_then(Value::as_str).unwrap_or("");
        let category = args.get("category").and_then(Value::as_str).unwrap_or("other");
        let filename_raw = args.get("filename").and_then(Value::as_str).unwrap_or("");
        let content = args.get("content").and_then(Value::as_str).unwrap_or("");

        if user_id.is_empty() {
            return Ok(ToolResult::err("save_to_unas: user_id is required"));
        }
        if filename_raw.is_empty() {
            return Ok(ToolResult::err("save_to_unas: filename is required"));
        }
        if content.is_empty() {
            return Ok(ToolResult::err("save_to_unas: refusing to write empty content"));
        }

        let plan = match resolve_plan(user_id, category, filename_raw) {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::err(format!("save_to_unas: {e}"))),
        };

        // Ensure the mount exists before we even try to create dirs. Doing
        // this via a metadata check (not a write probe) because we don't
        // want to create a stray file on the local filesystem if the
        // SMB share isn't mounted.
        if !plan.mount.exists() {
            return Ok(ToolResult::err(format!(
                "save_to_unas: UNAS mount `{}` is not present. Mount the \
                 Roura.io share on mac-server first (see README or ask the \
                 operator to run /opt/homebrew/bin/flaco-mount-unas) — then \
                 try again. Nothing written.",
                plan.mount.display()
            )));
        }

        // Make sure every directory on the path exists. mkdir -p is fine
        // because we're inside the mount and failures bubble up as real
        // errors.
        if let Some(parent) = plan.full_path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return Ok(ToolResult::err(format!(
                    "save_to_unas: could not create directory `{}`: {e}",
                    parent.display()
                )));
            }
        }

        if let Err(e) = fs::write(&plan.full_path, content).await {
            return Ok(ToolResult::err(format!(
                "save_to_unas: write to `{}` failed: {e}",
                plan.full_path.display()
            )));
        }

        let bytes = content.len();
        let smb_url = format!(
            "smb://10.0.1.2/Roura.io/{folder}/flaco/{category}/{filename}",
            folder = plan.folder,
            category = plan.category,
            filename = plan.filename
        );
        let summary = format!(
            "Saved {bytes} bytes to {}.\n\
             \n\
             Open in Finder / Files.app: `{}`\n\
             SMB URL for any other device: {}",
            plan.full_path.display(),
            plan.full_path.display(),
            smb_url
        );

        Ok(ToolResult::ok_text(summary).with_structured(json!({
            "user_id": user_id,
            "folder": plan.folder,
            "category": plan.category,
            "filename": plan.filename,
            "local_path": plan.full_path.to_string_lossy(),
            "smb_url": smb_url,
            "bytes_written": bytes,
        })))
    }
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct SavePlan {
    mount: PathBuf,
    folder: String,
    category: String,
    filename: String,
    full_path: PathBuf,
}

fn resolve_plan(user_id: &str, category: &str, filename: &str) -> std::result::Result<SavePlan, String> {
    let mount = unas_mount();
    let folder = folder_for(user_id);
    let category = sanitize_segment(category);
    if category.is_empty() {
        return Err("category resolves to empty after sanitization".into());
    }
    let filename = sanitize_filename(filename);
    if filename.is_empty() {
        return Err("filename resolves to empty after sanitization".into());
    }

    let full_path = mount
        .join(&folder)
        .join("flaco")
        .join(&category)
        .join(&filename);

    Ok(SavePlan {
        mount,
        folder,
        category,
        filename,
        full_path,
    })
}

/// The mount root of the Roura.io shared drive on mac-server. Overridable
/// via `FLACO_UNAS_MOUNT`.
fn unas_mount() -> PathBuf {
    PathBuf::from(
        std::env::var("FLACO_UNAS_MOUNT").unwrap_or_else(|_| "/Volumes/Roura.io".to_string()),
    )
}

/// Resolve a user id to their UNAS folder name. The mapping lives in
/// `FLACO_UNAS_USER_MAP` as comma-separated `key=value` pairs. Example:
///
///   FLACO_UNAS_USER_MAP=chris=cjroura,U0AS9PLFLCD=wroura,walter=wroura
///
/// Falls back to `FLACO_UNAS_DEFAULT_FOLDER` (default `shared`) if the
/// user id isn't in the map. The fallback is a deliberate choice: never
/// silently drop a save, always give the user SOMEWHERE to look.
fn folder_for(user_id: &str) -> String {
    if let Ok(map) = std::env::var("FLACO_UNAS_USER_MAP") {
        for entry in map.split(',') {
            let entry = entry.trim();
            if let Some((k, v)) = entry.split_once('=') {
                if k.trim() == user_id {
                    return sanitize_segment(v.trim());
                }
            }
        }
    }
    // Default fallback — NEVER use the literal user_id as a folder name
    // because Slack user ids (`U0AS9PLFLCD`) aren't human-friendly.
    std::env::var("FLACO_UNAS_DEFAULT_FOLDER").unwrap_or_else(|_| "shared".to_string())
}

/// Strip path separators, parent-dir traversal, and any other chars that
/// shouldn't appear in a directory or file name. Keeps letters, digits,
/// `_`, `-`, `.`, and space. Explicitly neutralizes `..` so a crafted
/// `../etc/passwd` can't sanitize to something that still escapes the
/// mount root once the path components are joined.
fn sanitize_segment(s: &str) -> String {
    // Step 1: drop everything that isn't in the allow-list.
    let raw: String = s
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        .collect();
    // Step 2: collapse repeated dots so `..` / `...` / `....` all become
    // a single literal dot that can't be interpreted as traversal.
    let mut out = String::with_capacity(raw.len());
    let mut last_dot = false;
    for c in raw.chars() {
        if c == '.' {
            if !last_dot {
                out.push('.');
            }
            last_dot = true;
        } else {
            out.push(c);
            last_dot = false;
        }
    }
    // Step 3: trim leading dots so the result is never hidden-file-ish
    // or mistakeable for a relative path.
    out.trim().trim_start_matches('.').to_string()
}

/// A slightly looser sanitizer for file names — same allow-list as
/// `sanitize_segment` but runs after the caller has already stripped
/// absolute-path prefixes. The double-dot collapsing inside
/// sanitize_segment is the real defense.
fn sanitize_filename(s: &str) -> String {
    let trimmed = s.trim().trim_start_matches('/');
    sanitize_segment(trimmed)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    // Every test in this module that touches env vars must first acquire
    // this mutex. Rust's test runner runs tests in parallel threads of
    // the same process, which means env vars are shared state — without
    // serialization, one test's `set_var` races another's `remove_var`
    // and the assertions flap. The mutex is returned as a guard so the
    // caller doesn't need to remember to drop it explicitly.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let saved = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn default_mount_is_volumes_rouraio() {
        let _g = env_lock();
        with_env("FLACO_UNAS_MOUNT", None, || {
            assert_eq!(unas_mount(), PathBuf::from("/Volumes/Roura.io"));
        });
    }

    #[test]
    fn mount_env_override() {
        let _g = env_lock();
        with_env("FLACO_UNAS_MOUNT", Some("/tmp/fake-unas"), || {
            assert_eq!(unas_mount(), PathBuf::from("/tmp/fake-unas"));
        });
    }

    #[test]
    fn folder_map_resolves_known_user() {
        let _g = env_lock();
        with_env(
            "FLACO_UNAS_USER_MAP",
            Some("chris=cjroura,U0AS9PLFLCD=wroura,walter=wroura"),
            || {
                assert_eq!(folder_for("chris"), "cjroura");
                assert_eq!(folder_for("U0AS9PLFLCD"), "wroura");
                assert_eq!(folder_for("walter"), "wroura");
            },
        );
    }

    #[test]
    fn folder_map_falls_back_to_shared_default() {
        let _g = env_lock();
        with_env("FLACO_UNAS_USER_MAP", Some("chris=cjroura"), || {
            with_env("FLACO_UNAS_DEFAULT_FOLDER", None, || {
                // Unknown id should NOT leak the raw id into the path —
                // it should land in the safe default folder.
                assert_eq!(folder_for("Ufoobar"), "shared");
            });
        });
    }

    #[test]
    fn folder_map_respects_custom_default() {
        let _g = env_lock();
        with_env("FLACO_UNAS_USER_MAP", Some("chris=cjroura"), || {
            with_env("FLACO_UNAS_DEFAULT_FOLDER", Some("guests"), || {
                assert_eq!(folder_for("Usomeone"), "guests");
            });
        });
    }

    #[test]
    fn sanitize_strips_path_traversal() {
        assert_eq!(sanitize_segment("../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_segment("normal_name-1.0"), "normal_name-1.0");
        assert_eq!(sanitize_segment("has spaces.md"), "has spaces.md");
    }

    #[test]
    fn sanitize_filename_rejects_dot_prefix_and_slashes() {
        assert_eq!(sanitize_filename("./notes.md"), "notes.md");
        assert_eq!(sanitize_filename("/absolute/path.txt"), "absolutepath.txt");
        assert_eq!(sanitize_filename("...hidden"), "hidden");
    }

    #[test]
    fn resolve_plan_builds_full_path() {
        let _g = env_lock();
        with_env("FLACO_UNAS_MOUNT", Some("/tmp/unas-test"), || {
            with_env("FLACO_UNAS_USER_MAP", Some("chris=cjroura"), || {
                let plan = resolve_plan("chris", "research", "yankees.md").unwrap();
                assert_eq!(plan.folder, "cjroura");
                assert_eq!(plan.category, "research");
                assert_eq!(plan.filename, "yankees.md");
                assert_eq!(
                    plan.full_path,
                    PathBuf::from("/tmp/unas-test/cjroura/flaco/research/yankees.md")
                );
            });
        });
    }

    #[test]
    fn resolve_plan_errors_on_empty_filename() {
        let _g = env_lock();
        with_env("FLACO_UNAS_USER_MAP", Some("chris=cjroura"), || {
            let err = resolve_plan("chris", "notes", "").unwrap_err();
            assert!(err.contains("filename"));
        });
    }

    #[test]
    fn resolve_plan_errors_on_empty_category() {
        let _g = env_lock();
        with_env("FLACO_UNAS_USER_MAP", Some("chris=cjroura"), || {
            let err = resolve_plan("chris", "", "x.md").unwrap_err();
            assert!(err.contains("category"));
        });
    }

    // NB: these two async tests hold the env_lock across an `.await`,
    // which clippy flags as `await_holding_lock`. In the general async
    // case that's a real deadlock hazard — but these are test helpers
    // that serialize env-var mutation across parallel test threads,
    // and the scope is small and bounded. The allow is scoped per-test
    // rather than applied module-wide to keep the signal high for real
    // code.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn tool_refuses_when_mount_missing() {
        // Env mutation has to happen inline here rather than via with_env,
        // because #[tokio::test] already owns the runtime and we can't
        // nest a block_on inside it. The env_lock guard still serializes
        // us against other env-touching tests.
        let _g = env_lock();
        let saved_mount = std::env::var("FLACO_UNAS_MOUNT").ok();
        let saved_map = std::env::var("FLACO_UNAS_USER_MAP").ok();
        std::env::set_var("FLACO_UNAS_MOUNT", "/tmp/flaco-unas-does-not-exist-xyz");
        std::env::set_var("FLACO_UNAS_USER_MAP", "chris=cjroura");

        let tool = SaveToUnas;
        let out = tool
            .call(json!({
                "user_id": "chris",
                "category": "research",
                "filename": "test.md",
                "content": "hello"
            }))
            .await
            .unwrap();
        assert!(!out.ok);
        assert!(out.output.to_lowercase().contains("mount"));

        // Restore env for the next test that might depend on defaults.
        match saved_mount {
            Some(v) => std::env::set_var("FLACO_UNAS_MOUNT", v),
            None => std::env::remove_var("FLACO_UNAS_MOUNT"),
        }
        match saved_map {
            Some(v) => std::env::set_var("FLACO_UNAS_USER_MAP", v),
            None => std::env::remove_var("FLACO_UNAS_USER_MAP"),
        }
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn tool_writes_file_when_mount_exists() {
        let _g = env_lock();
        let tmp = tempdir();
        let saved_mount = std::env::var("FLACO_UNAS_MOUNT").ok();
        let saved_map = std::env::var("FLACO_UNAS_USER_MAP").ok();
        std::env::set_var("FLACO_UNAS_MOUNT", &tmp);
        std::env::set_var("FLACO_UNAS_USER_MAP", "chris=cjroura");

        let tool = SaveToUnas;
        let out = tool
            .call(json!({
                "user_id": "chris",
                "category": "research",
                "filename": "integration_test.md",
                "content": "# hello\nthis is a test"
            }))
            .await
            .unwrap();
        assert!(out.ok, "write failed: {}", out.output);

        let expected = Path::new(&tmp).join("cjroura/flaco/research/integration_test.md");
        assert!(expected.exists(), "file was not written to {expected:?}");
        let content = std::fs::read_to_string(&expected).unwrap();
        assert_eq!(content, "# hello\nthis is a test");

        let _ = std::fs::remove_dir_all(&tmp);
        match saved_mount {
            Some(v) => std::env::set_var("FLACO_UNAS_MOUNT", v),
            None => std::env::remove_var("FLACO_UNAS_MOUNT"),
        }
        match saved_map {
            Some(v) => std::env::set_var("FLACO_UNAS_USER_MAP", v),
            None => std::env::remove_var("FLACO_UNAS_USER_MAP"),
        }
    }

    // Tests that need a temp dir use a simple helper so we don't pull in
    // `tempfile` as a dependency for just this module.
    fn tempdir() -> String {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = base.join(format!("flaco-unas-test-{pid}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }
}
