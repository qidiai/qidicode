use anyhow::Result;

#[allow(dead_code)]
pub fn reconcile_bank(_entity_id: &str, _statement_path: &str) -> Result<String> {
    anyhow::bail!("bank reconciliation not yet implemented");
}
