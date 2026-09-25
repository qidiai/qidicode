use anyhow::Result;

#[allow(dead_code)]
pub async fn chat_message(text: &str) -> Result<String> {
    Ok(format!("[Finance Agent] {}", text))
}
