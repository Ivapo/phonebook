use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;

use super::{LlmProvider, Message};

pub struct OllamaProvider {
    url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(url: String, model: String) -> Self {
        Self {
            url,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn chat(&self, system_prompt: &str, messages: &[Message]) -> anyhow::Result<String> {
        let mut ollama_messages = vec![json!({
            "role": "system",
            "content": system_prompt,
        })];

        for msg in messages {
            ollama_messages.push(json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }

        let body = json!({
            "model": self.model,
            "messages": ollama_messages,
            "stream": false,
        });

        let resp = self
            .client
            .post(format!("{}/api/chat", self.url))
            .json(&body)
            .send()
            .await
            .context("failed to call Ollama API")?;

        let data: serde_json::Value = resp
            .json()
            .await
            .context("failed to parse Ollama response")?;

        data["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("missing content in Ollama response"))
    }
}
