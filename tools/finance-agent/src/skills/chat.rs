use anyhow::Result;

pub async fn chat_message(text: &str) -> Result<String> {
    Ok(format!("Echo: {}", text))
}
