use anyhow::Result;
use crate::ledger::LedgerEngine;

#[allow(dead_code)]
pub async fn generate_report(report_type: &str, ledger: &LedgerEngine, entity_id: &str) -> Result<String> {
    match report_type {
        "income_statement" => Ok(ledger.income_statement(entity_id).await?),
        "balance_sheet" => Ok(ledger.balance_sheet(entity_id).await?),
        "trial_balance" => {
            let tb = ledger.trial_balance(entity_id).await?;
            let mut out = String::from("Trial Balance\n=============\n");
            for (acc, bal) in tb {
                out.push_str(&format!("{}: {:.2}\n", acc.name, bal));
            }
            Ok(out)
        }
        _ => anyhow::bail!("Unknown report type: {}", report_type),
    }
}
