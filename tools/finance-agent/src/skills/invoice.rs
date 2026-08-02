use anyhow::Result;

pub async fn scan_invoice(path: &str) -> Result<crate::models::Transaction> {
    anyhow::bail!("invoice OCR not yet implemented");
}
