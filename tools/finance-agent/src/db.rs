use rusqlite::{Connection, params, OptionalExtension};
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
pub struct Database {
    pub conn: std::sync::Arc<tokio::sync::Mutex<Connection>>,
}

impl Database {
    pub async fn new(path: &std::path::Path) -> anyhow::Result<Self> {
        let path = path.to_path_buf();
        let conn = tokio::task::spawn_blocking(move || Connection::open(path)).await??;
        Ok(Self {
            conn: std::sync::Arc::new(tokio::sync::Mutex::new(conn)),
        })
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let sql = r#"
            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                config TEXT
            );

            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                name TEXT NOT NULL,
                account_type TEXT NOT NULL,
                parent_id TEXT,
                description TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(id),
                FOREIGN KEY (parent_id) REFERENCES accounts(id)
            );

            CREATE TABLE IF NOT EXISTS transactions (
                id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                date TEXT NOT NULL,
                description TEXT,
                source TEXT,
                status TEXT DEFAULT 'pending',
                review_notes TEXT,
                created_at TEXT NOT NULL,
                created_by TEXT,
                FOREIGN KEY (entity_id) REFERENCES entities(id)
            );

            CREATE TABLE IF NOT EXISTS journal_entries (
                id TEXT PRIMARY KEY,
                transaction_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                amount REAL NOT NULL,
                currency TEXT DEFAULT 'CNY',
                memo TEXT,
                FOREIGN KEY (transaction_id) REFERENCES transactions(id),
                FOREIGN KEY (account_id) REFERENCES accounts(id)
            );

            CREATE TABLE IF NOT EXISTS invoices (
                id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                transaction_id TEXT,
                file_path TEXT,
                extracted_text TEXT,
                amount REAL,
                tax_amount REAL,
                counterparty TEXT,
                invoice_date TEXT,
                invoice_type TEXT,
                status TEXT DEFAULT 'pending',
                created_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(id),
                FOREIGN KEY (transaction_id) REFERENCES transactions(id)
            );

            CREATE TABLE IF NOT EXISTS review_queue (
                id TEXT PRIMARY KEY,
                transaction_id TEXT NOT NULL,
                reason TEXT,
                priority INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                resolved_at TEXT,
                FOREIGN KEY (transaction_id) REFERENCES transactions(id)
            );

            CREATE TABLE IF NOT EXISTS learned_context (
                id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                confidence REAL DEFAULT 1.0,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(id)
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                action TEXT NOT NULL,
                target_type TEXT,
                target_id TEXT,
                old_value TEXT,
                new_value TEXT,
                performed_by TEXT,
                performed_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_accounts_entity ON accounts(entity_id);
            CREATE INDEX IF NOT EXISTS idx_transactions_entity_date ON transactions(entity_id, date);
            CREATE INDEX IF NOT EXISTS idx_journal_transaction ON journal_entries(transaction_id);
            CREATE INDEX IF NOT EXISTS idx_invoices_entity ON invoices(entity_id);
            CREATE INDEX IF NOT EXISTS idx_review_queue ON review_queue(transaction_id);
        "#;
        tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            c.execute_batch(sql)
        }).await??;
        tracing::info!("Database initialized");
        Ok(())
    }

    pub async fn create_entity(&self, name: &str) -> anyhow::Result<String> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let id = tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            let id = Uuid::new_v4().to_string();
            c.execute(
                "INSERT INTO entities (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![id, name, Utc::now().to_rfc3339()],
            )?;
            Ok::<_, anyhow::Error>(id)
        }).await??;
        Ok(id)
    }

    pub async fn get_entity_id(&self, name: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let result = tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            let mut stmt = c.prepare("SELECT id FROM entities WHERE name = ?1")?;
            let result = stmt.query_row(params![name], |row| row.get(0)).optional()?;
            Ok::<_, anyhow::Error>(result)
        }).await??;
        Ok(result)
    }

    pub async fn ensure_entity(&self, name: &str) -> anyhow::Result<String> {
        match self.get_entity_id(name).await? {
            Some(id) => Ok(id),
            None => self.create_entity(name).await,
        }
    }

    pub async fn insert_transaction(
        &self,
        entity_id: &str,
        date: &str,
        description: &str,
        entries: &[(String, f64, String)],
        source: &str,
    ) -> anyhow::Result<String> {
        let conn = self.conn.clone();
        let entity_id = entity_id.to_string();
        let date = date.to_string();
        let description = description.to_string();
        let source = source.to_string();
        let entries = entries.to_vec();
        let entry_count = entries.len();
        let tx_id = tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            let tx_id = Uuid::new_v4().to_string();
            c.execute(
                "INSERT INTO transactions (id, entity_id, date, description, source, status, created_at, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, 'system')",
                params![tx_id, entity_id, date, description, source, Utc::now().to_rfc3339()],
            )?;

            for (account_id, amount, memo) in &entries {
                let entry_id = Uuid::new_v4().to_string();
                c.execute(
                    "INSERT INTO journal_entries (id, transaction_id, account_id, amount, memo)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![entry_id, tx_id, account_id, amount, memo],
                )?;
            }

            let total: f64 = entries.iter().map(|(_, amt, _)| amt).sum();
            if total.abs() > 0.01 || entries.iter().map(|(_, amt, _)| amt).filter(|a| a.abs() < 0.01).count() > 1 {
                let queue_id = Uuid::new_v4().to_string();
                c.execute(
                    "INSERT INTO review_queue (id, transaction_id, reason, priority, created_at)
                     VALUES (?1, ?2, 'unbalanced_transaction', 10, ?3)",
                    params![queue_id, tx_id, Utc::now().to_rfc3339()],
                )?;
            }
            Ok::<_, anyhow::Error>(tx_id)
        }).await??;
        tracing::info!("Inserted transaction {} with {} entries", tx_id, entry_count);
        Ok(tx_id)
    }

    pub async fn get_account_balance(&self, entity_id: &str, account_id: &str) -> anyhow::Result<f64> {
        let conn = self.conn.clone();
        let entity_id = entity_id.to_string();
        let account_id = account_id.to_string();
        let balance = tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            let balance: f64 = c.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM journal_entries je
                 JOIN transactions t ON je.transaction_id = t.id
                 WHERE t.entity_id = ?1 AND je.account_id = ?2",
                params![entity_id, account_id],
                |row| row.get(0),
            )?;
            Ok::<_, anyhow::Error>(balance)
        }).await??;
        Ok(balance)
    }

    pub async fn get_trial_balance(&self, entity_id: &str) -> anyhow::Result<Vec<(String, String, f64)>> {
        let conn = self.conn.clone();
        let entity_id = entity_id.to_string();
        let rows = tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            let mut stmt = c.prepare(
                "SELECT a.id, a.name, COALESCE(SUM(je.amount), 0) as balance
                 FROM accounts a
                 LEFT JOIN journal_entries je ON a.id = je.account_id
                 LEFT JOIN transactions t ON je.transaction_id = t.id AND t.entity_id = ?1
                 WHERE a.entity_id = ?1
                 GROUP BY a.id, a.name
                 ORDER BY a.account_type, a.name"
            )?;
            let mut result = Vec::new();
            for row in stmt.query_map(params![entity_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?))
            })? {
                result.push(row?);
            }
            Ok::<_, anyhow::Error>(result)
        }).await??;
        Ok(rows)
    }

    pub async fn log_audit(&self, entity_id: &str, action: &str, target_type: &str, target_id: &str, old_value: Option<&str>, new_value: Option<&str>) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let entity_id = entity_id.to_string();
        let action = action.to_string();
        let target_type = target_type.to_string();
        let target_id = target_id.to_string();
        let old_value = old_value.map(|s| s.to_string());
        let new_value = new_value.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let c = conn.blocking_lock();
            c.execute(
                "INSERT INTO audit_log (id, entity_id, action, target_type, target_id, old_value, new_value, performed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    entity_id,
                    action,
                    target_type,
                    target_id,
                    old_value,
                    new_value,
                    Utc::now().to_rfc3339()
                ],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await??;
        Ok(())
    }
}
