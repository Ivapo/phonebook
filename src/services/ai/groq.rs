use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;

use super::{LlmProvider, Message};

pub struct GroqProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GroqProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for GroqProvider {
    async fn chat(&self, system_prompt: &str, messages: &[Message]) -> anyhow::Result<String> {
        let mut chat_messages = vec![json!({
            "role": "system",
            "content": system_prompt,
        })];

        for msg in messages {
            chat_messages.push(json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }

        let body = json!({
            "model": self.model,
            "messages": chat_messages,
            "temperature": 0.7,
        });

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("failed to call Groq API")?;

        let status = resp.status();
        let data: serde_json::Value = resp
            .json()
            .await
            .context("failed to parse Groq response")?;

        if !status.is_success() {
            anyhow::bail!("Groq API error ({}): {}", status, data);
        }

        data["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("missing content in Groq response"))
    }
}
