use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(author, version, about = "Local token usage proxy + stats service")]
struct Args {
    /// Upstream API base URL, e.g. https://api.deepseek.com
    #[arg(long)]
    upstream_base_url: String,

    /// Upstream API key
    #[arg(long)]
    upstream_api_key: String,

    /// Listen host
    #[arg(long, default_value = "127.0.0.1")]
    listen_host: String,

    /// Listen port
    #[arg(long, default_value_t = 9800)]
    listen_port: u16,

    /// Path to JSONL stats file
    #[arg(long, default_value = "token_stats.jsonl")]
    db_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StatsRecord {
    ts: String,
    session_id: Option<String>,
    model: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    status: u16,
    upstream_ms: u128,
    endpoint: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StatsResponse {
    rows: Vec<AggregatedModel>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AggregatedModel {
    model: Option<String>,
    cnt: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    avg_ms: u128,
}

struct AppState {
    client: Client,
    upstream_base_url: String,
    upstream_api_key: String,
    stats_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    fs::create_dir_all(".")?;

    let client = Client::builder().build()?;
    let state = Arc::new(AppState {
        client,
        upstream_base_url: args.upstream_base_url.trim_end_matches('/').to_string(),
        upstream_api_key: args.upstream_api_key,
        stats_path: PathBuf::from(&args.db_path),
    });

    let app = Router::new()
        .route("/stats", get(stats_handler))
        .route("/v1/chat/completions", axum::routing::post(proxy_handler))
        .with_state(state);

    let addr = format!("{}:{}", args.listen_host, args.listen_port)
        .parse::<std::net::SocketAddr>()?;
    println!("=== Token Stats Service ===");
    println!("Listening: http://{}", addr);
    println!("Upstream: {}", args.upstream_base_url);
    println!("Stats file: {}", args.db_path);
    println!();
    println!("Endpoints:");
    println!("  POST /v1/chat/completions  -> forward + log token usage");
    println!("  GET  /stats                -> JSON token stats");
    println!();

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    let upstream_url = format!("{}/chat/completions", state.upstream_base_url);
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .to_vec();

    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let model_id = headers
        .get("x-model-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body_bytes = sanitize_tool_names(body_bytes);

    let mut builder = state
        .client
        .request(method.clone(), upstream_url)
        .header("content-type", "application/json")
        .body(body_bytes.clone());

    builder = builder.header("authorization", format!("Bearer {}", state.upstream_api_key));

    let t0 = std::time::Instant::now();
    let resp_result = builder.send().await;
    let upstream_ms = t0.elapsed().as_millis();

    match resp_result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body_bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
            let body_clone = body_bytes.clone();

            let (model_opt, usage_opt, error_opt) =
                match serde_json::from_slice::<serde_json::Value>(&body_clone) {
                    Ok(json) => {
                        let model = json.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let usage: Option<Usage> = json.get("usage").and_then(|v| serde_json::from_value(v.clone()).ok());
                        let error = json.get("error").and_then(|v| v.as_str()).map(|s| s.to_string());
                        (model.or(model_id), usage, error)
                    }
                    Err(_) => (model_id, None, None),
                };

            let (prompt_tokens, completion_tokens, total_tokens) = match usage_opt {
                Some(u) => (u.prompt_tokens, u.completion_tokens, u.total_tokens),
                None => (0, 0, 0),
            };

            let record = StatsRecord {
                ts: chrono::Utc::now().to_rfc3339(),
                session_id,
                model: model_opt,
                prompt_tokens,
                completion_tokens,
                total_tokens,
                status,
                upstream_ms,
                endpoint: "/v1/chat/completions".to_string(),
                error: error_opt,
            };

            if let Err(e) = append_record(&state.stats_path, &record) {
                eprintln!("Failed to write stats: {}", e);
            }

            let restored = restore_tool_names(body_bytes.to_vec());
            let mut response = Response::new(Body::from(restored));
            *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            Ok(response)
        }
        Err(e) => {
            let record = StatsRecord {
                ts: chrono::Utc::now().to_rfc3339(),
                session_id,
                model: model_id,
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                status: 502,
                upstream_ms,
                endpoint: "/v1/chat/completions".to_string(),
                error: Some(e.to_string()),
            };
            let _ = append_record(&state.stats_path, &record);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

async fn stats_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    let since = params.get("since").map(|s| s.as_str());
    let rows = read_stats(&state.stats_path, since);
    let resp = StatsResponse { rows };
    let json = serde_json::to_vec(&resp).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(([(axum::http::header::CONTENT_TYPE, "application/json")], json).into_response())
}

fn append_record(path: &PathBuf, record: &StatsRecord) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{}", line)?;
    file.sync_all()?;
    Ok(())
}

fn sanitize_tool_names(body: Vec<u8>) -> Vec<u8> {
    let mut value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(v) => v,
        Err(_) => return body,
    };

    if let Some(tools) = value.get_mut("tools").and_then(|v| v.as_array_mut()) {
        for tool in tools {
            if let Some(func) = tool.get_mut("function") {
                if let Some(name) = func.get_mut("name").and_then(|v| v.as_str()) {
                    let sanitized = name.replace(':', "__COLON__");
                    if sanitized != name {
                        *func.get_mut("name").unwrap() = serde_json::Value::String(sanitized);
                    }
                }
            }
        }
    }

    if let Ok(json) = serde_json::to_vec(&value) {
        return json;
    }
    body
}

fn restore_tool_names(body: Vec<u8>) -> Vec<u8> {
    let mut value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(v) => v,
        Err(_) => return body,
    };

    if let Some(tools) = value.get_mut("tools").and_then(|v| v.as_array_mut()) {
        for tool in tools {
            if let Some(func) = tool.get_mut("function") {
                if let Some(name) = func.get_mut("name").and_then(|v| v.as_str()) {
                    let restored = name.replace("__COLON__", ":");
                    if restored != name {
                        *func.get_mut("name").unwrap() = serde_json::Value::String(restored);
                    }
                }
            }
        }
    }

    if let Some(choices) = value.get_mut("choices").and_then(|v| v.as_array_mut()) {
        for choice in choices {
            if let Some(message) = choice.get_mut("message") {
                if let Some(tool_calls) = message.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    for tool_call in tool_calls {
                        if let Some(func) = tool_call.get_mut("function") {
                            if let Some(name) = func.get_mut("name").and_then(|v| v.as_str()) {
                                let restored = name.replace("__COLON__", ":");
                                if restored != name {
                                    *func.get_mut("name").unwrap() = serde_json::Value::String(restored);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Ok(json) = serde_json::to_vec(&value) {
        return json;
    }
    body
}

fn read_stats(path: &PathBuf, since: Option<&str>) -> Vec<AggregatedModel> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut map: std::collections::HashMap<Option<String>, (usize, u64, u64, u64, u128)> =
        std::collections::HashMap::new();

    for line in content.lines() {
        if let Ok(rec) = serde_json::from_str::<StatsRecord>(line) {
            if let Some(s) = since {
                if rec.ts.as_str() < s {
                    continue;
                }
            }
            let entry = map.entry(rec.model).or_insert((0, 0, 0, 0, 0));
            entry.0 += 1;
            entry.1 += rec.prompt_tokens;
            entry.2 += rec.completion_tokens;
            entry.3 += rec.total_tokens;
            entry.4 += rec.upstream_ms;
        }
    }

    let mut rows: Vec<AggregatedModel> = map
        .into_iter()
        .map(|(model, (cnt, pt, ct, tt, ms))| AggregatedModel {
            model,
            cnt,
            prompt_tokens: pt,
            completion_tokens: ct,
            total_tokens: tt,
            avg_ms: if cnt > 0 { ms / cnt as u128 } else { 0 },
        })
        .collect();
    rows.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    rows
}
