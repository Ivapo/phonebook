use anyhow::Context;
use async_trait::async_trait;

use super::MessagingProvider;

pub struct TwilioSmsProvider {
    account_sid: String,
    auth_token: String,
    from_number: String,
    client: reqwest::Client,
}

impl TwilioSmsProvider {
    pub fn new(account_sid: String, auth_token: String, from_number: String) -> Self {
        Self {
            account_sid,
            auth_token,
            from_number,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl MessagingProvider for TwilioSmsProvider {
    async fn send_message(&self, to: &str, body: &str) -> anyhow::Result<()> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.account_sid
        );

        self.client
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&[("To", to), ("From", &self.from_number), ("Body", body)])
            .send()
            .await
            .context("failed to send Twilio SMS")?
            .error_for_status()
            .context("Twilio API returned error")?;

        Ok(())
    }
}
