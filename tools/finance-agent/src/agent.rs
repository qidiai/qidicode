use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use axum::{
    routing::{get, post},
    Json,
    Router,
    extract::State,
};
use crate::config::Config;
use crate::db::Database;
use crate::ledger::LedgerEngine;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LLMResponse {
    pub choices: Vec<LLMChoice>,
    pub usage: Option<LLMUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LLMChoice {
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LLMUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct FinanceAgent {
    pub ledger: LedgerEngine,
    pub db: Database,
    pub entity_id: String,
    pub client: Client,
    pub model: String,
    pub api_base: String,
    pub api_key: Option<String>,
    pub context: Arc<Mutex<HashMap<String, String>>>,
}

impl FinanceAgent {
    pub fn new(ledger: LedgerEngine, db: Database, config: Config) -> Result<Self> {
        Ok(Self {
            ledger,
            db,
            entity_id: String::new(),
            client: Client::new(),
            model: config.model(),
            api_base: config.api_base(),
            api_key: config.api_key(),
            context: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn load_entity_context(&mut self, name: &str) -> Result<()> {
        self.entity_id = self.db.ensure_entity(name).await?;
        tracing::info!("Loaded entity: {} ({})", name, self.entity_id);
        Ok(())
    }

    pub async fn process_natural_language(&self, text: &str) -> Result<String> {
        if self.api_key.is_none() {
            return Ok("No LLM API key configured. Set LLM_API_KEY environment variable.".to_string());
        }

        let system_prompt = self.build_system_prompt();
        let messages = vec![
            ChatMessage { role: "system".into(), content: system_prompt },
            ChatMessage { role: "user".into(), content: text.to_string() },
        ];

        let req = LLMRequest {
            model: self.model.clone(),
            messages,
            tools: Some(self.build_tools_schema()),
        };

        let resp = self.client
            .post(&format!("{}/chat/completions", self.api_base))
            .bearer_auth(self.api_key.as_ref().unwrap())
            .json(&req)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let content = json["choices"][0]["message"]["content"].as_str()
            .unwrap_or("No response")
            .to_string();
        let tool_calls = json["choices"][0]["message"].get("tool_calls");

        if let Some(calls) = tool_calls {
            let mut results = Vec::new();
            if let Some(calls_array) = calls.as_array() {
                for call in calls_array {
                    let name = call["function"]["name"].as_str().unwrap_or("");
                    let args: serde_json::Value = serde_json::from_str(
                        call["function"]["arguments"].as_str().unwrap_or("{}")
                    )?;
                    let result = self.handle_tool_call(name, args).await?;
                    results.push(format!("{}: {}", name, result));
                }
            }
            Ok(format!("{}\n\nActions:\n{}", content, results.join("\n")))
        } else {
            Ok(content)
        }
    }

    fn build_system_prompt(&self) -> String {
        r#"You are a professional accounting assistant for a local-first finance agent.
Your job is to understand natural language transaction descriptions and convert them into proper double-entry journal entries.

Accounting rules:
- Every transaction must have at least two entries (debits = credits).
- Assets and Expenses increase with debit, decrease with credit.
- Liabilities, Equity, and Income increase with credit, decrease with debit.
- Use the exact account IDs provided in the account list.

When a user describes a transaction:
1. Identify the accounts involved.
2. Determine the amounts and direction (debit/credit).
3. Call the `record_transaction` tool with the balanced entries.

If the user asks for reports, balances, or analysis, use the appropriate tool.
If unsure, ask for clarification instead of guessing."#.to_string()
    }

    fn build_tools_schema(&self) -> serde_json::Value {
        serde_json::json!([
            {
                "type": "function",
                "function": {
                    "name": "record_transaction",
                    "description": "Record a double-entry transaction",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "date": { "type": "string", "description": "Transaction date (YYYY-MM-DD)" },
                            "description": { "type": "string", "description": "What happened" },
                            "entries": {
                                "type": "array",
                                "description": "Journal entries: [account_id, amount, memo]",
                                "items": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            }
                        },
                        "required": ["date", "description", "entries"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "get_balance",
                    "description": "Get account balance",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "account_id": { "type": "string" }
                        },
                        "required": ["account_id"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "list_accounts",
                    "description": "List all accounts",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "generate_report",
                    "description": "Generate financial report",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "report_type": {
                                "type": "string",
                                "enum": ["income_statement", "balance_sheet", "trial_balance"]
                            }
                        },
                        "required": ["report_type"]
                    }
                }
            }
        ])
    }

    async fn handle_tool_call(&self, name: &str, args: serde_json::Value) -> Result<String> {
        match name {
            "record_transaction" => {
                let date = args["date"].as_str().unwrap_or_default();
                let desc = args["description"].as_str().unwrap_or_default();
                let entries_raw = args["entries"].as_array().unwrap();
                let mut entries = Vec::new();
                for e in entries_raw {
                    let arr = e.as_array().unwrap();
                    let acc_id = arr[0].as_str().unwrap_or_default().to_string();
                    let amount = arr[1].as_f64().unwrap_or_default();
                    let memo = arr[2].as_str().unwrap_or("").to_string();
                    entries.push((acc_id, amount, memo));
                }
                let tx_id = self.ledger.record_transaction(&self.entity_id, date, desc, &entries, "ai_generated").await?;
                Ok(format!("Recorded transaction {}", tx_id))
            }
            "get_balance" => {
                let account_id = args["account_id"].as_str().unwrap_or_default();
                let balance = self.ledger.get_balance(&self.entity_id, account_id).await?;
                Ok(format!("Balance for {}: {:.2}", account_id, balance))
            }
            "list_accounts" => {
                let conn = self.db.conn.lock().await;
                let mut stmt = conn.prepare("SELECT id, name, account_type FROM accounts WHERE entity_id = ?1")?;
                let rows = stmt.query_map(rusqlite::params![self.entity_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })?;
                let mut out = Vec::new();
                for r in rows {
                    let (_id, name, atype) = r?;
                    out.push(format!("{} ({})", name, atype));
                }
                Ok(format!("Accounts:\n{}", out.join("\n")))
            }
            "generate_report" => {
                let report_type = args["report_type"].as_str().unwrap_or("trial_balance");
                Ok(crate::skills::report::generate_report(report_type, &self.ledger, &self.entity_id).await?)
            }
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }

    pub async fn run_repl(&mut self) -> Result<()> {
        use std::io::{self, Write};
        println!("Finance Agent REPL (type 'exit' to quit)");
        loop {
            print!("> ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let text = input.trim();
            if text.eq_ignore_ascii_case("exit") || text.eq_ignore_ascii_case("quit") {
                break;
            }
            if text.is_empty() {
                continue;
            }
            match self.process_natural_language(text).await {
                Ok(resp) => println!("{}\n", resp),
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Ok(())
    }

    pub async fn run_server(&self, port: u16) -> Result<()> {
        use std::net::SocketAddr;

        let state = self.clone_for_server();

        let app = Router::new()
            .route("/", get(|| async { "Finance Agent API" }))
            .route("/api/chat", post(chat_handler))
            .route("/api/accounts", get(list_accounts_handler))
            .route("/api/reports/{report_type}", get(report_handler))
            .with_state(state);

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        tracing::info!("Server listening on http://{}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app.into_make_service()).await?;
        Ok(())
    }

    fn clone_for_server(&self) -> ServerState {
        ServerState {
            ledger: self.ledger.clone(),
            db: self.db.clone(),
            entity_id: self.entity_id.clone(),
            client: self.client.clone(),
            model: self.model.clone(),
            api_base: self.api_base.clone(),
            api_key: self.api_key.clone(),
            context: self.context.clone(),
        }
    }
}

#[derive(Clone)]
struct ServerState {
    ledger: LedgerEngine,
    db: Database,
    entity_id: String,
    client: reqwest::Client,
    model: String,
    api_base: String,
    api_key: Option<String>,
    context: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl ServerState {
    fn check_api_key(&self, headers: &axum::http::HeaderMap) -> Option<Json<serde_json::Value>> {
        if let Ok(expected) = std::env::var("FINANCE_API_KEY") {
            if !expected.is_empty() {
                let provided = headers.get("X-API-Key").and_then(|v| v.to_str().ok()).unwrap_or("");
                if provided != expected {
                    return Some(Json(serde_json::json!({ "error": "Invalid or missing X-API-Key" })));
                }
            }
        }
        None
    }
}

async fn chat_handler(
    State(state): State<ServerState>, headers: axum::http::HeaderMap, Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Some(e) = state.check_api_key(&headers) { return e; }
        let text = body["message"].as_str().unwrap_or_default();
    let agent = FinanceAgent {
        ledger: state.ledger.clone(),
        db: state.db.clone(),
        entity_id: state.entity_id.clone(),
        client: state.client.clone(),
        model: state.model.clone(),
        api_base: state.api_base.clone(),
        api_key: state.api_key.clone(),
        context: state.context.clone(),
    };
    match agent.process_natural_language(text).await {
        Ok(resp) => Json(serde_json::json!({ "response": resp })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn list_accounts_handler(
    State(state): State<ServerState>, headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    if let Some(e) = state.check_api_key(&headers) { return e; }
    let conn = state.db.conn.lock().await;
    let mut stmt = conn.prepare("SELECT id, name, account_type FROM accounts WHERE entity_id = ?1").unwrap();
    let rows = stmt.query_map(rusqlite::params![state.entity_id], |row| {
        Ok((row.get::<_, String>(0).unwrap(), row.get::<_, String>(1).unwrap(), row.get::<_, String>(2).unwrap()))
    }).unwrap();
    let mut out = Vec::new();
    for r in rows {
        if let Ok((id, name, atype)) = r {
            out.push(serde_json::json!({"id": id, "name": name, "type": atype}));
        }
    }
    Json(serde_json::json!({ "accounts": out }))
}

async fn report_handler(
    State(state): State<ServerState>, headers: axum::http::HeaderMap, axum::extract::Path(report_type): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    if let Some(e) = state.check_api_key(&headers) { return e; }
    match crate::skills::report::generate_report(&report_type, &state.ledger, &state.entity_id).await {
        Ok(report) => Json(serde_json::json!({ "report": report })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

