use anyhow::Result;
use std::path::Path;
use csv::ReaderBuilder;

#[allow(dead_code)]
pub fn reconcile_bank(entity_id: &str, statement_path: &str) -> Result<String> {
    let p = Path::new(statement_path);
    if !p.exists() {
        anyhow::bail!("Statement file not found: {}", statement_path);
    }

    let extension = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case("csv") {
        return Ok(format!("Not a CSV file: {}", statement_path));
    }

    let mut reader = ReaderBuilder::new().from_path(statement_path)?;
    let record_count = reader.records().count();

    Ok(format!(
        "Bank reconciliation summary for entity {}:\n- File: {}\n- CSV rows: {}",
        entity_id, statement_path, record_count
    ))
}
