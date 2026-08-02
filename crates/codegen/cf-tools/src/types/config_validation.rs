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
#[derive(Debug, Clone, PartialEq)]
pub enum ToolConfigEntryErrorKind {
    ParamsJsonParse { raw: String, error: String },
    ParamsJsonNotObject { value: String },
    NameOverrideInvalid { name: String, error: String },
}

impl ToolConfigEntryError {
    pub fn new(index: usize, tool_id: String, kind: ToolConfigEntryErrorKind) -> Self {
        Self { index, tool_id, kind }
    }

    /// Path of the offending field within the wire tool-config list, e.g.
    /// `tools[3].params_json`. Used by callers to report precise error locations.
    pub fn field_path(&self) -> String {
        let field = match &self.kind {
            ToolConfigEntryErrorKind::ParamsJsonParse { .. }
            | ToolConfigEntryErrorKind::ParamsJsonNotObject { .. } => "params_json",
            ToolConfigEntryErrorKind::NameOverrideInvalid { .. } => "name_override",
        };
        format!("tools[{}].{}", self.index, field)
    }
}

/// Parse a params JSON string into a HashMap<String, Value>.
pub fn parse_params_json(index: usize, tool_id: &str, raw: Option<&str>) -> Result<Option<serde_json::Map<String, serde_json::Value>>, ToolConfigEntryError> {
    match raw {
        Some(r) if !r.is_empty() => {
            let parsed: serde_json::Value = serde_json::from_str(r)
                .map_err(|e| ToolConfigEntryError::new(
                    index,
                    tool_id.to_string(),
                    ToolConfigEntryErrorKind::ParamsJsonParse {
                        raw: r.to_string(),
                        error: e.to_string(),
                    },
                ))?;
            match parsed {
                serde_json::Value::Object(map) => Ok(Some(map)),
                other => Err(ToolConfigEntryError::new(
                    index,
                    tool_id.to_string(),
                    ToolConfigEntryErrorKind::ParamsJsonNotObject {
                        value: other.to_string(),
                    },
                )),
            }
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

