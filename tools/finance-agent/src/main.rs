use std::path::PathBuf;
use clap::{Parser, ValueEnum};
use anyhow::Result;

mod config;
mod db;
mod ledger;
mod agent;
mod skills;
mod models;
mod export;

use config::Config;
use db::Database;
use ledger::LedgerEngine;
use agent::FinanceAgent;

#[derive(Parser, Debug)]
#[command(author, version, about = "Local-first AI Finance Agent (Rust)", long_about = None)]
struct Args {
    /// Company/entity name
    #[arg(short, long)]
    entity: Option<String>,

    /// Database path
    #[arg(short, long)]
    db_path: Option<PathBuf>,

    /// Start web API server
    #[arg(long)]
    serve: bool,

    /// API server port
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Natural language input for a transaction
    #[arg(short = 'n', long)]
    input: Option<String>,

    /// Generate a report
    #[arg(long, value_enum)]
    report: Option<ReportType>,

    /// Interactive REPL mode
    #[arg(short, long)]
    interactive: bool,
}

#[derive(Clone, ValueEnum, Debug)]
#[clap(rename_all = "snake_case")]
enum ReportType {
    IncomeStatement,
    BalanceSheet,
    TrialBalance,
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let config = Config::load()?;
    let entity = args.entity.unwrap_or_else(|| config.default_entity());
    let db_path = args.db_path.unwrap_or_else(|| PathBuf::from(config.database_path()));

    tracing::info!("Starting Finance Agent v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Entity: {}", entity);
    tracing::info!("Database: {:?}", db_path);

    let db = Database::new(&db_path).await?;
    db.init().await?;
    let ledger = LedgerEngine::new(db.clone());
    let mut agent = FinanceAgent::new(ledger, db.clone(), config)?;

    // Load entity context if exists
    agent.load_entity_context(&entity).await?;

    if let Some(text) = args.input {
        let result = agent.process_natural_language(&text).await?;
        println!("{}", result);
        return Ok(());
    }

    if let Some(report_type) = args.report {
        let report = match report_type {
            ReportType::IncomeStatement => agent.ledger.income_statement(&agent.entity_id).await?,
            ReportType::BalanceSheet => agent.ledger.balance_sheet(&agent.entity_id).await?,
            ReportType::TrialBalance => {
                let tb = agent.ledger.trial_balance(&agent.entity_id).await?;
                let mut out = String::from("Trial Balance\n=============\n");
                for (acc, bal) in tb {
                    out.push_str(&format!("{}: {:.2}\n", acc.name, bal));
                }
                out
            }
        };
        println!("{}", report);
        return Ok(());
    }

    if args.interactive {
        agent.run_repl().await?;
        return Ok(());
    }

    if args.serve {
        agent.run_server(args.port).await?;
        return Ok(());
    }

    println!("Finance Agent ready. Use --help for options.");
    Ok(())
}
