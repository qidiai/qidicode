use std::path::PathBuf;
use clap::Parser;
use anyhow::Result;

mod db;
mod ledger;
mod agent;
mod skills;
mod models;
mod export;

use db::Database;
use ledger::LedgerEngine;
use agent::FinanceAgent;

#[derive(Parser, Debug)]
#[command(author, version, about = "Local-first AI Finance Agent (Rust)", long_about = None)]
struct Args {
    /// Company/entity name
    #[arg(short, long, default_value = "default")]
    entity: String,

    /// Database path
    #[arg(short, long, default_value = "./finance.db")]
    db_path: PathBuf,

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
    report: Option<String>,

    /// Interactive REPL mode
    #[arg(short, long)]
    interactive: bool,
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    tracing::info!("Starting Finance Agent v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Entity: {}", args.entity);
    tracing::info!("Database: {:?}", args.db_path);

    let db = Database::new(&args.db_path).await?;
    db.init().await?;
    let ledger = LedgerEngine::new(db.clone());
    let mut agent = FinanceAgent::new(ledger, db.clone())?;

    // Load entity context if exists
    agent.load_entity_context(&args.entity).await?;

    if let Some(text) = args.input {
        let result = agent.process_natural_language(&text).await?;
        println!("{}", result);
        return Ok(());
    }

    if let Some(_report_type) = args.report {
        let report = agent.ledger.income_statement(&agent.entity_id).await?;
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
