use anyhow::Result;

#[allow(dead_code)]
pub fn check_budget(entity_id: &str) -> Result<String> {
    Ok(format!(
        "Budget check for entity '{}' is not yet implemented.\n\
         This feature will analyze actual spending against budget plans.",
        entity_id
    ))
}
