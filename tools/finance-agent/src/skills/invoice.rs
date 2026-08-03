use anyhow::Result;

#[allow(dead_code)]
pub async fn scan_invoice(_path: &str) -> Result<crate::models::Transaction> {
    anyhow::bail!("invoice OCR not yet implemented");
}
