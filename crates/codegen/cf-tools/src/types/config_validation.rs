//! Configuration validation helpers (stub — originally protobuf-generated).
//! Replaced with plain Rust types for workspace compilation.



/// Error when validating a ToolConfigEntry.
#[derive(Debug, Clone)]
pub struct ToolConfigEntryError {
    pub index: usize,
    pub tool_id: String,
    pub kind: ToolConfigEntryErrorKind,
}

/// Kind of validation error.
#[derive(Debug, Clone)]
pub enum ToolConfigEntryErrorKind {
    ParamsJsonParse { raw: String, error: String },
    ParamsJsonNotObject { value: String },
    NameOverrideInvalid { name: String, error: String },
}

impl ToolConfigEntryError {
    pub fn new(index: usize, tool_id: String, kind: ToolConfigEntryErrorKind) -> Self {
        Self { index, tool_id, kind }
    }
}

/// Parse a params JSON string into a HashMap<String, Value>.
pub fn parse_params_json(index: usize, tool_id: &str, raw: Option<&str>) -> Result<Option<serde_json::Map<String, serde_json::Value>>, ToolConfigEntryError> {
    match raw {
        Some(r) if !r.is_empty() => {
            let parsed: serde_json::Map<String, serde_json::Value> = serde_json::from_str(r)
                .map_err(|e| ToolConfigEntryError::new(
                    index,
                    tool_id.to_string(),
                    ToolConfigEntryErrorKind::ParamsJsonParse {
                        raw: r.to_string(),
                        error: e.to_string(),
                    },
                ))?;
            Ok(Some(parsed))
        }
        _ => Ok(None),
    }
}

/// Validate a name override value.
pub fn validate_name_override(index: usize, tool_id: &str, name: Option<&str>, _raw: Option<&str>) -> Result<(), ToolConfigEntryError> {
    if let Some(n) = name {
        if n.is_empty() {
            return Err(ToolConfigEntryError::new(
                index,
                tool_id.to_string(),
                ToolConfigEntryErrorKind::NameOverrideInvalid {
                    name: n.to_string(),
                    error: "name override cannot be empty".to_string(),
                },
            ));
        }
    }
    Ok(())
}

/// Validate a tool name override.
pub fn validate_tool_name(name: &str) -> Result<(), ToolConfigEntryError> {
    if name.is_empty() {
        return Err(ToolConfigEntryError::new(0, String::new(), ToolConfigEntryErrorKind::NameOverrideInvalid {
            name: name.to_string(),
            error: "tool name cannot be empty".to_string(),
        }));
    }
    Ok(())
}

impl std::fmt::Display for ToolConfigEntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool config entry error at index {} (tool_id={}): {:?}", self.index, self.tool_id, self.kind)
    }
}

impl std::error::Error for ToolConfigEntryError {}

