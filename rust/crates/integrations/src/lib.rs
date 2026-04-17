//! Built-in integrations for flacoAi — Perplexity-style connectors that work
//! out of the box. Each connector is a tool the model can call.

// Integration connectors interact with external CLIs and APIs where these
// lints are noisy without benefit.
#![allow(
    clippy::needless_lifetimes,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    clippy::unnecessary_literal_bound
)]

pub mod connectors;

use serde_json::Value;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Connector trait
// ---------------------------------------------------------------------------

/// A built-in integration connector.
pub trait Connector: Send + Sync {
    /// Short machine-readable name (e.g. `"github"`).
    fn name(&self) -> &str;

    /// Human-readable description shown to the model.
    fn description(&self) -> &str;

    /// JSON schema for the tool input parameters.
    fn input_schema(&self) -> Value;

    /// Execute the connector with the given input.
    fn execute(&self, input: &Value) -> Result<String, String>;

    /// Whether this connector is available on the current system (CLI present,
    /// env vars set, etc.). Connectors that return `false` are silently
    /// disabled.
    fn is_available(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Connector registry
// ---------------------------------------------------------------------------

/// Registry of all built-in connectors with their availability status.
pub struct ConnectorRegistry {
    connectors: Vec<Box<dyn Connector>>,
}

impl ConnectorRegistry {
    /// Build the registry with all built-in connectors.
    #[must_use]
    pub fn discover() -> Self {
        Self {
            connectors: connectors::all_connectors(),
        }
    }

    /// List all connectors and whether they're available.
    #[must_use]
    pub fn status(&self) -> Vec<ConnectorStatus> {
        self.connectors
            .iter()
            .map(|c| ConnectorStatus {
                name: c.name().to_string(),
                description: c.description().to_string(),
                available: c.is_available(),
            })
            .collect()
    }

    /// How many connectors are available vs total.
    #[must_use]
    pub fn availability_summary(&self) -> (usize, usize) {
        let total = self.connectors.len();
        let available = self.connectors.iter().filter(|c| c.is_available()).count();
        (available, total)
    }

    /// Execute a connector by name.
    pub fn execute(&self, name: &str, input: &Value) -> Result<String, String> {
        let connector = self
            .connectors
            .iter()
            .find(|c| c.name() == name)
            .ok_or_else(|| format!("unknown integration: {name}"))?;

        if !connector.is_available() {
            return Err(format!(
                "integration '{name}' is not available on this system. {}",
                connector.description()
            ));
        }

        connector.execute(input)
    }

    /// Get input schemas for all available connectors (for tool registration).
    #[must_use]
    pub fn available_schemas(&self) -> BTreeMap<String, (String, Value)> {
        self.connectors
            .iter()
            .filter(|c| c.is_available())
            .map(|c| {
                (
                    c.name().to_string(),
                    (c.description().to_string(), c.input_schema()),
                )
            })
            .collect()
    }

    /// Get all connector names.
    #[must_use]
    pub fn connector_names(&self) -> Vec<&str> {
        self.connectors.iter().map(|c| c.name()).collect()
    }
}

/// Status of a single connector.
#[derive(Debug, Clone)]
pub struct ConnectorStatus {
    pub name: String,
    pub description: String,
    pub available: bool,
}

/// Render a startup banner line showing integration status.
#[must_use]
pub fn render_integration_banner(registry: &ConnectorRegistry) -> String {
    let (available, total) = registry.availability_summary();
    let unavailable: Vec<String> = registry
        .status()
        .iter()
        .filter(|s| !s.available)
        .map(|s| s.name.clone())
        .collect();

    if unavailable.is_empty() {
        format!("Integrations     {available}/{total} active")
    } else {
        format!(
            "Integrations     {available}/{total} active (needs setup: {})",
            unavailable.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_discovers_connectors() {
        let registry = ConnectorRegistry::discover();
        let (_, total) = registry.availability_summary();
        assert!(
            total >= 10,
            "should have at least 10 connectors, got {total}"
        );
    }

    #[test]
    fn registry_status_has_names_and_descriptions() {
        let registry = ConnectorRegistry::discover();
        for status in registry.status() {
            assert!(
                !status.name.is_empty(),
                "connector name should not be empty"
            );
            assert!(
                !status.description.is_empty(),
                "connector '{}' description should not be empty",
                status.name
            );
        }
    }

    #[test]
    fn rejects_unknown_integration() {
        let registry = ConnectorRegistry::discover();
        let result = registry.execute("nonexistent", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown integration"));
    }

    #[test]
    fn banner_renders_summary() {
        let registry = ConnectorRegistry::discover();
        let banner = render_integration_banner(&registry);
        assert!(banner.contains("Integrations"));
        assert!(banner.contains("active"));
    }
}
