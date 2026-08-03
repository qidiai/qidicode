use anyhow::Result;
use std::path::Path;
use crate::models::Transaction;
use crate::ledger::JournalEntry;

#[allow(dead_code)]
pub async fn scan_invoice(path: &str) -> Result<Transaction> {
    let p = Path::new(path);
    if !p.exists() {
        anyhow::bail!("Invoice file not found: {}", path);
    }
    if !p.is_file() {
        anyhow::bail!("Path is not a file: {}", path);
    }

    let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");

    Ok(Transaction {
        id: String::new(),
        entity_id: String::new(),
        date: String::new(),
        description: format!("Invoice placeholder from file: {}", file_name),
        source: "invoice_ocr".to_string(),
        status: "pending".to_string(),
        entries: vec![JournalEntry {
            id: String::new(),
            transaction_id: String::new(),
            account_id: String::new(),
            amount: 0.0,
            currency: "CNY".to_string(),
            memo: Some("placeholder".to_string()),
        }],
    })
}
