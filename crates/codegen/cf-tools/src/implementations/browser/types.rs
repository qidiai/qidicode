//! Input types for the browser tool family.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserNavigateInput {
    /// Absolute URL to load (http:// or https:// only).
    pub url: String,
    /// Extra settle time in milliseconds after the page `load` event before
    /// returning (0..=10000). Default 300.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserSnapshotInput {
    /// When true, return only the interactive-element listing (cheaper);
    /// omit the markdown body.
    #[serde(default)]
    pub refs_only: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserClickInput {
    /// Snapshot ref number (e.g. "7") or CSS selector (e.g. "a.login").
    pub selector: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserTypeInput {
    /// Snapshot ref number (e.g. "7") or CSS selector (e.g. "#search").
    pub selector: String,
    /// Text to type into the field.
    pub text: String,
    /// Press Enter after typing (submits forms, triggers searches).
    #[serde(default)]
    pub submit: bool,
    /// Clear the field before typing. Default true.
    #[serde(default = "default_true")]
    pub clear: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserReadInput {
    /// CSS selector of the element whose inner text to read. Omit (or null)
    /// for the whole page rendered as markdown.
    #[serde(default)]
    pub selector: Option<String>,
}