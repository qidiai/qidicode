use serde::{Deserialize, Deserializer};
use serde::de::IntoDeserializer;

use crate::types::FinishReason;

pub fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Some relays (observed in production: luyou.qidiai.ltd → kimi-k3 streams)
/// emit chunks with `finish_reason: ""` instead of `null` or a valid variant.
/// `FinishReason` is a fieldless enum, so the empty string hard-fails with
/// `unknown variant ''` and kills the whole session. Map `""` to `None`
/// (same semantics as "no finish reason yet") while keeping unknown non-empty
/// values a hard deserialization error (do not silently swallow real anomalies).
pub fn empty_finish_reason_as_none<'de, D>(
    deserializer: D,
) -> Result<Option<FinishReason>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => FinishReason::deserialize(s.into_deserializer()).map(Some),
    }
}
