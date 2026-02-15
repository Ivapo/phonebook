use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiPreferences {
    #[serde(default)]
    pub identity: Identity,
    #[serde(default = "default_tone")]
    pub tone: String,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub returning_customers: ReturningCustomers,
    #[serde(default)]
    pub boundaries: Boundaries,
    #[serde(default)]
    pub custom_instructions: String,
}

fn default_tone() -> String {
    "professional".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    #[serde(default = "default_true")]
    pub disclose_ai: bool,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub act_as_business: bool,
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            disclose_ai: true,
            agent_name: String::new(),
            act_as_business: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default = "default_true")]
    pub can_book: bool,
    #[serde(default = "default_true")]
    pub can_cancel: bool,
    #[serde(default = "default_true")]
    pub can_reschedule: bool,
    #[serde(default = "default_true")]
    pub can_answer_questions: bool,
    #[serde(default)]
    pub can_send_reminders: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            can_book: true,
            can_cancel: true,
            can_reschedule: true,
            can_answer_questions: true,
            can_send_reminders: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturningCustomers {
    #[serde(default = "default_true")]
    pub greet_by_name: bool,
    #[serde(default)]
    pub remember_preferences: bool,
}

impl Default for ReturningCustomers {
    fn default() -> Self {
        Self {
            greet_by_name: true,
            remember_preferences: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Boundaries {
    #[serde(default)]
    pub booking_only: bool,
    #[serde(default = "default_true")]
    pub share_pricing: bool,
    #[serde(default)]
    pub pricing_info: String,
}

impl Default for Boundaries {
    fn default() -> Self {
        Self {
            booking_only: false,
            share_pricing: true,
            pricing_info: String::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

impl Default for AiPreferences {
    fn default() -> Self {
        Self {
            identity: Identity::default(),
            tone: default_tone(),
            capabilities: Capabilities::default(),
            returning_customers: ReturningCustomers::default(),
            boundaries: Boundaries::default(),
            custom_instructions: String::new(),
        }
    }
}

impl AiPreferences {
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    pub fn to_prompt(&self) -> String {
        let mut lines = Vec::new();

        // Identity
        if !self.identity.agent_name.is_empty() {
            lines.push(format!("Your name is {}.", self.identity.agent_name));
        }
        if !self.identity.disclose_ai {
            lines.push(
                "Never reveal that you are an AI. Act as a human representative of the business."
                    .to_string(),
            );
        }
        if self.identity.act_as_business {
            lines.push(
                "Speak in first person as the business owner (use \"I\", \"my\", \"we\")."
                    .to_string(),
            );
        }

        // Tone
        match self.tone.as_str() {
            "friendly" => lines.push(
                "Use a warm, friendly tone. Be personable and approachable.".to_string(),
            ),
            "casual" => lines.push(
                "Use a casual, relaxed tone. Keep it conversational, like texting a friend."
                    .to_string(),
            ),
            // "professional" is the default — no extra instruction needed
            _ => {}
        }

        // Capabilities — only emit for disabled ones
        let mut disabled = Vec::new();
        if !self.capabilities.can_book {
            disabled.push("booking new appointments");
        }
        if !self.capabilities.can_cancel {
            disabled.push("cancelling appointments");
        }
        if !self.capabilities.can_reschedule {
            disabled.push("rescheduling appointments");
        }
        if !self.capabilities.can_answer_questions {
            disabled.push("answering general questions");
        }
        if !disabled.is_empty() {
            lines.push(format!(
                "You are NOT able to help with: {}. Politely let the customer know and suggest they contact the business directly.",
                disabled.join(", ")
            ));
        }

        // Returning customers
        if self.returning_customers.greet_by_name {
            lines.push(
                "If you know the customer's name from previous messages, greet them by name."
                    .to_string(),
            );
        }
        if self.returning_customers.remember_preferences {
            lines.push(
                "Remember and reference the customer's previous preferences when relevant."
                    .to_string(),
            );
        }

        // Boundaries
        if self.boundaries.booking_only {
            lines.push(
                "Only discuss topics related to booking appointments. Politely redirect any other topics."
                    .to_string(),
            );
        }
        if self.boundaries.share_pricing && !self.boundaries.pricing_info.is_empty() {
            lines.push(format!(
                "You may share the following pricing information: {}",
                self.boundaries.pricing_info
            ));
        }

        // Custom instructions
        if !self.custom_instructions.is_empty() {
            lines.push(self.custom_instructions.clone());
        }

        if lines.is_empty() {
            return String::new();
        }

        format!("\nPersonality and behavior:\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_produces_minimal_prompt() {
        let prefs = AiPreferences::default();
        let prompt = prefs.to_prompt();
        // Default greet_by_name is true, so we get one line
        assert!(prompt.contains("greet them by name"));
        // No tone line for professional (default)
        assert!(!prompt.contains("casual"));
        assert!(!prompt.contains("friendly"));
    }

    #[test]
    fn test_from_json_partial() {
        let json = r#"{"tone":"casual"}"#;
        let prefs = AiPreferences::from_json(json).unwrap();
        assert_eq!(prefs.tone, "casual");
        assert!(prefs.identity.disclose_ai); // default
        assert!(prefs.capabilities.can_book); // default
    }

    #[test]
    fn test_from_json_full() {
        let json = r#"{
            "identity": {"disclose_ai": false, "agent_name": "Sophie", "act_as_business": true},
            "tone": "friendly",
            "capabilities": {"can_book": true, "can_cancel": false, "can_reschedule": true, "can_answer_questions": true, "can_send_reminders": false},
            "returning_customers": {"greet_by_name": true, "remember_preferences": true},
            "boundaries": {"booking_only": false, "share_pricing": true, "pricing_info": "Haircut $35, Color $80"},
            "custom_instructions": "Always end with a smiley face"
        }"#;
        let prefs = AiPreferences::from_json(json).unwrap();
        let prompt = prefs.to_prompt();
        assert!(prompt.contains("Your name is Sophie."));
        assert!(prompt.contains("Never reveal that you are an AI"));
        assert!(prompt.contains("first person as the business owner"));
        assert!(prompt.contains("warm, friendly tone"));
        assert!(prompt.contains("NOT able to help with: cancelling appointments"));
        assert!(prompt.contains("remember_preferences") || prompt.contains("previous preferences"));
        assert!(prompt.contains("Haircut $35, Color $80"));
        assert!(prompt.contains("Always end with a smiley face"));
    }

    #[test]
    fn test_null_json_uses_defaults() {
        let prefs = AiPreferences::default();
        assert_eq!(prefs.tone, "professional");
        assert!(prefs.identity.disclose_ai);
        assert!(prefs.capabilities.can_book);
        assert!(!prefs.boundaries.booking_only);
    }

    #[test]
    fn test_disabled_capabilities_prompt() {
        let json = r#"{"capabilities":{"can_book":false,"can_cancel":false,"can_reschedule":false,"can_answer_questions":false}}"#;
        let prefs = AiPreferences::from_json(json).unwrap();
        let prompt = prefs.to_prompt();
        assert!(prompt.contains("booking new appointments"));
        assert!(prompt.contains("cancelling appointments"));
        assert!(prompt.contains("rescheduling appointments"));
        assert!(prompt.contains("answering general questions"));
    }

    #[test]
    fn test_pricing_not_shown_when_share_disabled() {
        let json = r#"{"boundaries":{"share_pricing":false,"pricing_info":"Haircut $35"}}"#;
        let prefs = AiPreferences::from_json(json).unwrap();
        let prompt = prefs.to_prompt();
        assert!(!prompt.contains("Haircut $35"));
    }

    #[test]
    fn test_pricing_not_shown_when_empty() {
        let json = r#"{"boundaries":{"share_pricing":true,"pricing_info":""}}"#;
        let prefs = AiPreferences::from_json(json).unwrap();
        let prompt = prefs.to_prompt();
        assert!(!prompt.contains("pricing information"));
    }
}
