#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    pub id: String,
    pub entity_id: String,
    pub date: String,
    pub description: String,
    pub source: String,
    pub status: String,
    pub entries: Vec<crate::ledger::JournalEntry>,
}
