pub mod ollama;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, system_prompt: &str, messages: &[Message]) -> anyhow::Result<String>;
}
