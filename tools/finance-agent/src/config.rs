use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub llm_api_base: Option<String>,
    pub llm_api_key: Option<String>,
    pub llm_model: Option<String>,
    pub default_entity: Option<String>,
    pub database_path: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let project_root = env!("CARGO_MANIFEST_DIR");
        let config_path = Path::new(project_root).join("finance_config.json");

        let mut cfg: Config = if config_path.exists() {
            let content = fs::read_to_string(config_path)?;
            serde_json::from_str(&content)?
        } else {
            Config {
                llm_api_base: None,
                llm_api_key: None,
                llm_model: None,
                default_entity: None,
                database_path: None,
            }
        };

        // Environment variable fallback for LLM settings
        if cfg.llm_api_base.is_none() {
            cfg.llm_api_base = std::env::var("LLM_API_BASE").ok();
        }
        if cfg.llm_api_key.is_none() {
            cfg.llm_api_key = std::env::var("LLM_API_KEY").ok();
        }
        if cfg.llm_model.is_none() {
            cfg.llm_model = std::env::var("LLM_MODEL").ok();
        }

        Ok(cfg)
    }

    pub fn api_base(&self) -> String {
        self.llm_api_base
            .clone()
            .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string())
    }

    pub fn api_key(&self) -> Option<String> {
        self.llm_api_key.clone()
    }

    pub fn model(&self) -> String {
        self.llm_model
            .clone()
            .unwrap_or_else(|| "deepseek-chat".to_string())
    }

    pub fn default_entity(&self) -> String {
        self.default_entity
            .clone()
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn database_path(&self) -> String {
        self.database_path
            .clone()
            .unwrap_or_else(|| "./finance.db".to_string())
    }
}
