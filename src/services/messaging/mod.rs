pub mod twilio;

use async_trait::async_trait;

#[async_trait]
pub trait MessagingProvider: Send + Sync {
    async fn send_message(&self, to: &str, body: &str) -> anyhow::Result<()>;
}
