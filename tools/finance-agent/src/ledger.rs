use anyhow::Result;
use crate::db::Database;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccountType {
    Asset,
    Liability,
    Equity,
    Income,
    Expense,
}

#[allow(dead_code)]
impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountType::Asset => "asset",
            AccountType::Liability => "liability",
            AccountType::Equity => "equity",
            AccountType::Income => "income",
            AccountType::Expense => "expense",
        }
    }

    pub fn normal_side(&self) -> &'static str {
        match self {
            AccountType::Asset => "debit",
            AccountType::Liability => "credit",
            AccountType::Equity => "credit",
            AccountType::Income => "credit",
            AccountType::Expense => "debit",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: String,
    pub entity_id: String,
    pub name: String,
    pub account_type: AccountType,
    pub parent_id: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub transaction_id: String,
    pub account_id: String,
    pub amount: f64,
    pub currency: String,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct Transaction {
    pub id: String,
    pub entity_id: String,
    pub date: String,
    pub description: String,
    pub source: String,
    pub status: String,
    pub entries: Vec<JournalEntry>,
}

#[derive(Clone)]
pub struct LedgerEngine {
    db: Database,
}

impl LedgerEngine {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

#[allow(dead_code)]
    pub async fn create_account(&self, entity_id: &str, name: &str, account_type: AccountType, parent_id: Option<&str>, description: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.db.conn.lock().await;
        conn.execute(
            "INSERT INTO accounts (id, entity_id, name, account_type, parent_id, description, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, entity_id, name, account_type.as_str(), parent_id, description, chrono::Utc::now().to_rfc3339()],
        )?;
        self.db.log_audit(entity_id, "create_account", "account", &id, None, Some(&format!("{} ({})", name, account_type.as_str()))).await?;
        Ok(id)
    }

    pub async fn record_transaction(
        &self,
        entity_id: &str,
        date: &str,
        description: &str,
        entries: &[(String, f64, String)], // (account_id, amount, memo)
        source: &str,
    ) -> Result<String> {
        // Validate double-entry: sum must be zero
        let total: f64 = entries.iter().map(|(_, amt, _)| amt).sum();
        if total.abs() > 0.01 {
            anyhow::bail!("Transaction not balanced: total = {}", total);
        }

        let tx_id = self.db.insert_transaction(entity_id, date, description, entries, source).await?;
        self.db.log_audit(entity_id, "record_transaction", "transaction", &tx_id, None, Some(description)).await?;
        Ok(tx_id)
    }

    pub async fn get_balance(&self, entity_id: &str, account_id: &str) -> Result<f64> {
        self.db.get_account_balance(entity_id, account_id).await
    }

    pub async fn trial_balance(&self, entity_id: &str) -> Result<Vec<(Account, f64)>> {
        let rows = self.db.get_trial_balance(entity_id).await?;
        let mut result = Vec::new();
        for (acc_id, acc_name, balance) in rows {
            // Fetch account type
            let conn = self.db.conn.lock().await;
            let acc_type_str: String = conn.query_row(
                "SELECT account_type FROM accounts WHERE id = ?1",
                rusqlite::params![acc_id],
                |row| row.get(0),
            )?;
            let account_type = match acc_type_str.as_str() {
                "asset" => AccountType::Asset,
                "liability" => AccountType::Liability,
                "equity" => AccountType::Equity,
                "income" => AccountType::Income,
                "expense" => AccountType::Expense,
                _ => anyhow::bail!("Unknown account type: {}", acc_type_str),
            };
            result.push((Account {
                id: acc_id,
                entity_id: entity_id.to_string(),
                name: acc_name,
                account_type,
                parent_id: None,
                description: None,
            }, balance));
        }
        Ok(result)
    }

    pub async fn income_statement(&self, entity_id: &str) -> Result<String> {
        let accounts = self.trial_balance(entity_id).await?;
        let mut income = 0.0;
        let mut expenses = 0.0;
        for (acc, balance) in &accounts {
            match acc.account_type {
                AccountType::Income => income += balance,
                AccountType::Expense => expenses += balance,
                _ => {}
            }
        }
        let net = income - expenses;
        Ok(format!(
            "Income Statement\n\
             ================\n\
             Total Income:  {:>12.2}\n\
             Total Expenses:{:>12.2}\n\
             Net Income:    {:>12.2}\n",
            income, expenses, net
        ))
    }

    pub async fn balance_sheet(&self, entity_id: &str) -> Result<String> {
        let accounts = self.trial_balance(entity_id).await?;
        let mut assets = 0.0;
        let mut liabilities = 0.0;
        let mut equity = 0.0;
        for (acc, balance) in &accounts {
            match acc.account_type {
                AccountType::Asset => assets += balance,
                AccountType::Liability => liabilities += balance,
                AccountType::Equity => equity += balance,
                _ => {}
            }
        }
        Ok(format!(
            "Balance Sheet\n\
             =============\n\
             Assets:        {:>12.2}\n\
             Liabilities:   {:>12.2}\n\
             Equity:        {:>12.2}\n\
             Check (A=L+E): {:>12.2}\n",
            assets, liabilities, equity, liabilities + equity - assets
        ))
    }
}



