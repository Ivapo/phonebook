use crate::models::{AiPreferences, ConversationMessage, ExtractedIntent, Intent};
use crate::services::ai::{LlmProvider, Message};

const SYSTEM_PROMPT: &str = r#"You are an intent extraction engine for an SMS booking assistant. Analyze the customer's latest message in context of the conversation history.

Return ONLY valid JSON (no markdown, no explanation) with this exact structure:
{
  "intent": "book|reschedule|cancel|confirm|decline|general_question|unknown",
  "customer_name": "extracted name or null",
  "requested_date": "extracted date like 2025-01-15 or null",
  "requested_time": "extracted time like 14:00 or null",
  "duration_minutes": 60,
  "notes": "any special requests or null",
  "message_to_customer": "Your friendly reply to the customer"
}

Intent rules:
- "book": Customer wants to schedule a new appointment
- "reschedule": Customer wants to change an existing appointment
- "cancel": Customer wants to cancel an existing appointment
- "confirm": Customer says yes/ok/confirmed/sounds good to a proposed time
- "decline": Customer says no/that doesn't work to a proposed time
- "general_question": Customer asks about services, hours, pricing, etc.
- "unknown": Can't determine intent

When booking, only suggest times within the business hours shown in the context.
If the customer requests a time outside business hours, politely suggest the nearest available time.

For the message_to_customer:
- Be friendly and professional
- If booking: ask for missing info (name, preferred date/time) or propose a time
- If confirming: acknowledge the booking is confirmed
- If cancelling: confirm what's being cancelled
- Keep messages concise (SMS-friendly, under 160 chars when possible)
"#;

pub async fn extract_intent(
    llm: &dyn LlmProvider,
    history: &[ConversationMessage],
    latest_message: &str,
    business_context: &str,
    ai_preferences: Option<&AiPreferences>,
) -> anyhow::Result<ExtractedIntent> {
    let mut messages: Vec<Message> = history
        .iter()
        .map(|m| Message {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    messages.push(Message {
        role: "user".to_string(),
        content: latest_message.to_string(),
    });

    let personality = ai_preferences
        .map(|p| p.to_prompt())
        .unwrap_or_default();

    let system = format!("{SYSTEM_PROMPT}{personality}\n\nBusiness context:\n{business_context}");

    let response = llm.chat(&system, &messages).await?;

    parse_intent_response(&response)
}

fn parse_intent_response(response: &str) -> anyhow::Result<ExtractedIntent> {
    // Try direct parse first
    if let Ok(intent) = serde_json::from_str::<ExtractedIntent>(response) {
        return Ok(intent);
    }

    // Strip markdown code fences
    let cleaned = response
        .trim()
        .strip_prefix("```json")
        .or_else(|| response.trim().strip_prefix("```"))
        .unwrap_or(response.trim());
    let cleaned = cleaned
        .strip_suffix("```")
        .unwrap_or(cleaned)
        .trim();

    if let Ok(intent) = serde_json::from_str::<ExtractedIntent>(cleaned) {
        return Ok(intent);
    }

    // Try to find JSON object in the response
    if let Some(start) = cleaned.find('{') {
        if let Some(end) = cleaned.rfind('}') {
            let json_str = &cleaned[start..=end];
            if let Ok(intent) = serde_json::from_str::<ExtractedIntent>(json_str) {
                return Ok(intent);
            }
        }
    }

    // Fallback: unknown intent with raw text as reply
    tracing::warn!("failed to parse LLM response as intent JSON, using fallback");
    Ok(ExtractedIntent {
        intent: Intent::Unknown,
        customer_name: None,
        requested_date: None,
        requested_time: None,
        duration_minutes: None,
        notes: None,
        message_to_customer: response.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{"intent":"book","customer_name":"John","requested_date":"2025-01-15","requested_time":"14:00","duration_minutes":60,"notes":null,"message_to_customer":"Great! I have you down for Jan 15 at 2pm."}"#;
        let result = parse_intent_response(json).unwrap();
        assert_eq!(result.intent, Intent::Book);
        assert_eq!(result.customer_name, Some("John".to_string()));
    }

    #[test]
    fn test_parse_markdown_fenced_json() {
        let json = "```json\n{\"intent\":\"confirm\",\"customer_name\":null,\"requested_date\":null,\"requested_time\":null,\"duration_minutes\":null,\"notes\":null,\"message_to_customer\":\"Confirmed!\"}\n```";
        let result = parse_intent_response(json).unwrap();
        assert_eq!(result.intent, Intent::Confirm);
    }

    #[test]
    fn test_parse_fallback() {
        let raw = "I don't understand the format you want";
        let result = parse_intent_response(raw).unwrap();
        assert_eq!(result.intent, Intent::Unknown);
        assert_eq!(result.message_to_customer, raw);
    }
}
