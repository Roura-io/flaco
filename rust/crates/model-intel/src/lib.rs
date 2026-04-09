use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Ollama API discovery
// ---------------------------------------------------------------------------

/// Model info returned by Ollama `/api/tags`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TagsResponse {
    models: Vec<OllamaModel>,
}

/// Running model returned by Ollama `/api/ps`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PsResponse {
    models: Vec<RunningModel>,
}

/// Discovers models on an Ollama instance.
pub struct OllamaDiscovery {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl OllamaDiscovery {
    /// Create a new discovery instance. `base_url` should be the Ollama host
    /// (e.g. `http://10.0.1.3:11434`).
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// List all installed models on the Ollama host.
    pub fn list_models(&self) -> Result<Vec<OllamaModel>, String> {
        let url = format!("{}/api/tags", self.base_url);
        let response: TagsResponse = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("failed to reach Ollama at {url}: {e}"))?
            .json()
            .map_err(|e| format!("failed to parse Ollama response: {e}"))?;
        Ok(response.models)
    }

    /// List currently running/loaded models.
    pub fn running_models(&self) -> Result<Vec<RunningModel>, String> {
        let url = format!("{}/api/ps", self.base_url);
        let response: PsResponse = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("failed to reach Ollama at {url}: {e}"))?
            .json()
            .map_err(|e| format!("failed to parse Ollama ps response: {e}"))?;
        Ok(response.models)
    }

    /// Check if the Ollama host is reachable.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.client
            .get(&self.base_url)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Task classification
// ---------------------------------------------------------------------------

/// Categories of tasks the user might be doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Coding,
    Research,
    Reasoning,
    Creative,
    General,
}

impl TaskCategory {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Research => "research",
            Self::Reasoning => "reasoning",
            Self::Creative => "creative",
            Self::General => "general",
        }
    }
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Classify a user prompt into a task category using keyword heuristics.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn classify_task(prompt: &str) -> TaskCategory {
    let lower = prompt.to_lowercase();

    let coding_signals = [
        "fix",
        "bug",
        "error",
        "compile",
        "build",
        "test",
        "refactor",
        "implement",
        "function",
        "method",
        "class",
        "struct",
        "trait",
        "module",
        "import",
        "crate",
        "cargo",
        "npm",
        "pip",
        "git",
        "commit",
        "merge",
        "branch",
        "deploy",
        "ci",
        "lint",
        "format",
        "debug",
        "stack trace",
        "exception",
        "panic",
        "segfault",
        "api",
        "endpoint",
        "route",
        "handler",
        "middleware",
        "database",
        "query",
        "sql",
        "migration",
        "schema",
        "type",
        "interface",
        "enum",
        "async",
        "await",
        "thread",
        "mutex",
        "dockerfile",
        "yaml",
        "json",
        "toml",
        "config",
        "swift",
        "rust",
        "python",
        "typescript",
        "javascript",
        "code",
        "review",
    ];

    let research_signals = [
        "search",
        "find",
        "look up",
        "what is",
        "how does",
        "explain",
        "compare",
        "difference between",
        "pros and cons",
        "best practice",
        "documentation",
        "tutorial",
        "guide",
        "learn",
        "understand",
        "research",
        "investigate",
        "alternative",
        "recommendation",
        "benchmark",
    ];

    let reasoning_signals = [
        "calculate",
        "solve",
        "prove",
        "derive",
        "analyze",
        "evaluate",
        "optimize",
        "algorithm",
        "complexity",
        "math",
        "equation",
        "formula",
        "logic",
        "theorem",
        "probability",
        "statistics",
        "architecture decision",
        "trade-off",
        "design",
        "why does",
        "reason",
        "cause",
        "root cause",
    ];

    let creative_signals = [
        "write",
        "story",
        "poem",
        "essay",
        "blog",
        "article",
        "creative",
        "imagine",
        "brainstorm",
        "idea",
        "name",
        "tagline",
        "slogan",
        "pitch",
        "narrative",
        "draft",
        "rewrite",
        "tone",
        "voice",
        "style",
    ];

    let coding_score = coding_signals
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let research_score = research_signals
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let reasoning_score = reasoning_signals
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let creative_score = creative_signals
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();

    let max = coding_score
        .max(research_score)
        .max(reasoning_score)
        .max(creative_score);

    if max == 0 {
        return TaskCategory::General;
    }

    if coding_score == max {
        TaskCategory::Coding
    } else if reasoning_score == max {
        TaskCategory::Reasoning
    } else if research_score == max {
        TaskCategory::Research
    } else {
        TaskCategory::Creative
    }
}

// ---------------------------------------------------------------------------
// Model recommendation
// ---------------------------------------------------------------------------

/// Preferred model families per task category, in priority order.
const MODEL_PREFERENCES: &[(TaskCategory, &[&str])] = &[
    (
        TaskCategory::Coding,
        &[
            "qwen3",
            "deepseek-coder",
            "codellama",
            "codegemma",
            "starcoder",
        ],
    ),
    (
        TaskCategory::Research,
        &["qwen3", "llama3", "mistral", "gemma"],
    ),
    (
        TaskCategory::Reasoning,
        &["deepseek-r1", "qwen3", "llama3", "phi"],
    ),
    (
        TaskCategory::Creative,
        &["llama3", "mistral", "qwen3", "gemma"],
    ),
    (TaskCategory::General, &["qwen3", "llama3", "mistral"]),
];

/// A model recommendation.
#[derive(Debug, Clone)]
pub struct ModelRecommendation {
    /// The recommended model name (as known to Ollama).
    pub model: String,
    /// Why this model was chosen.
    pub reason: String,
    /// Whether the model is already installed on the host.
    pub installed: bool,
    /// Whether the user has previously confirmed this choice.
    pub previously_confirmed: bool,
}

/// Pick the best installed model for a given task category.
#[must_use]
pub fn recommend_model(
    category: TaskCategory,
    installed: &[OllamaModel],
    preferences: &ModelPreferences,
) -> ModelRecommendation {
    // Check for a saved preference first.
    if let Some(pref) = preferences.get(category) {
        if installed.iter().any(|m| m.name == pref.model) {
            return ModelRecommendation {
                model: pref.model.clone(),
                reason: format!("previously confirmed for {category} tasks"),
                installed: true,
                previously_confirmed: true,
            };
        }
    }

    let preference_list = MODEL_PREFERENCES
        .iter()
        .find(|(cat, _)| *cat == category)
        .map_or(&["qwen3"] as &[&str], |(_, families)| *families);

    // Try to find an installed model matching the preferred families.
    for family in preference_list {
        // Prefer larger variants first (they sort later alphabetically with size suffixes).
        let mut matching: Vec<&OllamaModel> = installed
            .iter()
            .filter(|m| {
                let base = m.name.split(':').next().unwrap_or(&m.name);
                base.starts_with(family)
            })
            .collect();
        matching.sort_by(|a, b| b.size.cmp(&a.size));

        if let Some(best) = matching.first() {
            return ModelRecommendation {
                model: best.name.clone(),
                reason: format!("{} is well-suited for {category} tasks", best.name),
                installed: true,
                previously_confirmed: false,
            };
        }
    }

    // No preferred model installed — recommend the largest available.
    if let Some(largest) = installed.iter().max_by_key(|m| m.size) {
        return ModelRecommendation {
            model: largest.name.clone(),
            reason: format!(
                "no preferred {} model installed; using {} as the best available",
                category, largest.name
            ),
            installed: true,
            previously_confirmed: false,
        };
    }

    // No models at all — suggest a download.
    let suggested = preference_list.first().copied().unwrap_or("qwen3");
    ModelRecommendation {
        model: format!("{suggested}:latest"),
        reason: format!("no models installed; run `ollama pull {suggested}` on your Ollama host"),
        installed: false,
        previously_confirmed: false,
    }
}

/// Suggest a model to download for a task category that the host doesn't have.
#[must_use]
pub fn suggest_download(category: TaskCategory, installed: &[OllamaModel]) -> Option<String> {
    let preference_list = MODEL_PREFERENCES
        .iter()
        .find(|(cat, _)| *cat == category)
        .map_or(&[] as &[&str], |(_, families)| *families);

    for family in preference_list {
        let has_family = installed.iter().any(|m| {
            let base = m.name.split(':').next().unwrap_or(&m.name);
            base.starts_with(family)
        });
        if !has_family {
            return Some(format!("ollama pull {family}"));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Preference persistence
// ---------------------------------------------------------------------------

/// A single saved preference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPref {
    pub model: String,
    pub confirmed: bool,
}

/// Persisted model preferences per task category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelPreferences {
    #[serde(flatten)]
    entries: BTreeMap<String, ModelPref>,
}

impl ModelPreferences {
    /// Load preferences from disk, returning empty defaults if the file
    /// doesn't exist or can't be parsed.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Save preferences to disk.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Get the preference for a task category.
    #[must_use]
    pub fn get(&self, category: TaskCategory) -> Option<&ModelPref> {
        self.entries.get(category.label())
    }

    /// Set a preference for a task category.
    pub fn set(&mut self, category: TaskCategory, model: String) {
        self.entries.insert(
            category.label().to_string(),
            ModelPref {
                model,
                confirmed: true,
            },
        );
    }

    /// Clear all preferences.
    pub fn reset(&mut self) {
        self.entries.clear();
    }

    /// Default path for the preferences file.
    #[must_use]
    pub fn default_path() -> PathBuf {
        dirs_fallback().join("model-preferences.json")
    }
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .map_or_else(|_| PathBuf::from("."), PathBuf::from)
        .join(".flaco")
}

// ---------------------------------------------------------------------------
// Convenience: full recommendation flow
// ---------------------------------------------------------------------------

/// Run the complete model recommendation flow: discover models, classify the
/// prompt, and return a recommendation.
#[must_use]
pub fn auto_select_model(
    ollama_url: &str,
    prompt: &str,
    preferences: &ModelPreferences,
) -> ModelRecommendation {
    let discovery = OllamaDiscovery::new(ollama_url);
    let installed = discovery.list_models().unwrap_or_default();
    let category = classify_task(prompt);
    recommend_model(category, &installed, preferences)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_coding_prompts() {
        assert_eq!(
            classify_task("fix the bug in main.rs"),
            TaskCategory::Coding
        );
        assert_eq!(
            classify_task("implement a new API endpoint"),
            TaskCategory::Coding
        );
        assert_eq!(
            classify_task("refactor the database query"),
            TaskCategory::Coding
        );
    }

    #[test]
    fn classifies_research_prompts() {
        assert_eq!(
            classify_task("what is the difference between TCP and UDP"),
            TaskCategory::Research
        );
        assert_eq!(
            classify_task("look up the documentation for serde"),
            TaskCategory::Research
        );
    }

    #[test]
    fn classifies_reasoning_prompts() {
        assert_eq!(
            classify_task("analyze the algorithm complexity"),
            TaskCategory::Reasoning
        );
        assert_eq!(
            classify_task("evaluate the trade-off between these designs"),
            TaskCategory::Reasoning
        );
    }

    #[test]
    fn classifies_creative_prompts() {
        assert_eq!(
            classify_task("write a blog post about AI"),
            TaskCategory::Creative
        );
        assert_eq!(
            classify_task("brainstorm tagline ideas for the product"),
            TaskCategory::Creative
        );
    }

    #[test]
    fn classifies_general_prompts() {
        assert_eq!(classify_task("hello"), TaskCategory::General);
        assert_eq!(classify_task("thanks"), TaskCategory::General);
    }

    #[test]
    fn recommends_installed_model_for_coding() {
        let installed = vec![
            OllamaModel {
                name: "qwen3:30b-a3b".into(),
                size: 30_000_000_000,
                parameter_size: None,
                quantization_level: None,
            },
            OllamaModel {
                name: "llama3:8b".into(),
                size: 8_000_000_000,
                parameter_size: None,
                quantization_level: None,
            },
        ];
        let prefs = ModelPreferences::default();
        let rec = recommend_model(TaskCategory::Coding, &installed, &prefs);

        assert!(rec.installed);
        assert!(rec.model.starts_with("qwen3"));
    }

    #[test]
    fn uses_saved_preference() {
        let installed = vec![OllamaModel {
            name: "llama3:8b".into(),
            size: 8_000_000_000,
            parameter_size: None,
            quantization_level: None,
        }];
        let mut prefs = ModelPreferences::default();
        prefs.set(TaskCategory::Coding, "llama3:8b".into());

        let rec = recommend_model(TaskCategory::Coding, &installed, &prefs);
        assert_eq!(rec.model, "llama3:8b");
        assert!(rec.previously_confirmed);
    }

    #[test]
    fn suggests_download_when_family_missing() {
        let installed = vec![OllamaModel {
            name: "llama3:8b".into(),
            size: 8_000_000_000,
            parameter_size: None,
            quantization_level: None,
        }];
        let suggestion = suggest_download(TaskCategory::Coding, &installed);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("qwen3"));
    }

    #[test]
    fn preferences_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "model-intel-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prefs.json");

        let mut prefs = ModelPreferences::default();
        prefs.set(TaskCategory::Coding, "qwen3:30b".into());
        prefs.save(&path).unwrap();

        let loaded = ModelPreferences::load(&path);
        let pref = loaded.get(TaskCategory::Coding).unwrap();
        assert_eq!(pref.model, "qwen3:30b");
        assert!(pref.confirmed);

        std::fs::remove_dir_all(dir).unwrap();
    }
}
