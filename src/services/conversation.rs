use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::db::queries;
use crate::models::{
    Booking, BookingStatus, Conversation, ConversationMessage, ConversationState, Intent,
    PendingBooking,
};
use crate::services::ai::intent::extract_intent;
use crate::state::AppState;

pub async fn process_message(
    state: &Arc<AppState>,
    from_phone: &str,
    message: &str,
) -> anyhow::Result<String> {
    // Load or create conversation
    let mut conv = {
        let db = state.db.lock().unwrap();
        queries::get_conversation(&db, from_phone)?
    }
    .unwrap_or_else(|| new_conversation(from_phone));

    // Append user message
    conv.messages.push(ConversationMessage {
        role: "user".to_string(),
        content: message.to_string(),
    });

    // Build business context
    let business_context = format!(
        "Business phone: {}. Owner phone: {}.",
        state.config.twilio_phone_number, state.config.owner_phone,
    );

    // Extract intent via LLM
    let extracted = extract_intent(
        state.llm.as_ref(),
        &conv.messages,
        message,
        &business_context,
    )
    .await?;

    tracing::info!(
        phone = from_phone,
        intent = ?extracted.intent,
        state = conv.state.as_str(),
        "processing message"
    );

    // State machine transition
    let reply = match (&conv.state, &extracted.intent) {
        // New booking request
        (_, Intent::Book) => {
            let has_enough_info = extracted.customer_name.is_some()
                && extracted.requested_date.is_some()
                && extracted.requested_time.is_some();

            if has_enough_info {
                // We have enough info to propose a time
                conv.pending_booking = Some(PendingBooking {
                    customer_name: extracted.customer_name,
                    date_time: make_datetime_string(
                        extracted.requested_date.as_deref(),
                        extracted.requested_time.as_deref(),
                    ),
                    duration_minutes: extracted.duration_minutes,
                    notes: extracted.notes,
                });
                conv.state = ConversationState::Confirming;
            } else {
                // Need more info
                conv.pending_booking = Some(PendingBooking {
                    customer_name: extracted.customer_name,
                    date_time: make_datetime_string(
                        extracted.requested_date.as_deref(),
                        extracted.requested_time.as_deref(),
                    ),
                    duration_minutes: extracted.duration_minutes,
                    notes: extracted.notes,
                });
                conv.state = ConversationState::CollectingInfo;
            }
            extracted.message_to_customer.clone()
        }

        // Collecting info — LLM continues asking questions until it has enough
        (ConversationState::CollectingInfo, _) => {
            // Update pending booking with any new info
            if let Some(ref mut pending) = conv.pending_booking {
                if extracted.customer_name.is_some() {
                    pending.customer_name = extracted.customer_name.clone();
                }
                if extracted.requested_date.is_some() || extracted.requested_time.is_some() {
                    pending.date_time = make_datetime_string(
                        extracted
                            .requested_date
                            .as_deref()
                            .or(pending.date_time.as_deref()),
                        extracted.requested_time.as_deref(),
                    );
                }
                if extracted.duration_minutes.is_some() {
                    pending.duration_minutes = extracted.duration_minutes;
                }
                if extracted.notes.is_some() {
                    pending.notes = extracted.notes.clone();
                }
            }

            // Check if we now have enough info to confirm
            let has_enough = conv
                .pending_booking
                .as_ref()
                .map(|p| p.customer_name.is_some() && p.date_time.is_some())
                .unwrap_or(false);

            if has_enough
                && (extracted.intent == Intent::Confirm
                    || extracted.requested_date.is_some()
                    || extracted.requested_time.is_some())
            {
                conv.state = ConversationState::Confirming;
            }

            extracted.message_to_customer.clone()
        }

        // Customer confirms a proposed booking
        (ConversationState::Confirming, Intent::Confirm) => {
            if let Some(ref pending) = conv.pending_booking {
                let booking = create_booking_from_pending(from_phone, pending);
                let reply = extracted.message_to_customer.clone();

                // Save booking to DB
                {
                    let db = state.db.lock().unwrap();
                    queries::create_booking(&db, &booking)?;
                }

                // Notify owner
                let owner_msg = format!(
                    "New booking: {} for {} at {}",
                    pending.customer_name.as_deref().unwrap_or("Unknown"),
                    pending
                        .date_time
                        .as_deref()
                        .unwrap_or("TBD"),
                    from_phone,
                );
                notify_owner(state, &owner_msg).await;

                // Reset conversation
                conv.state = ConversationState::Idle;
                conv.pending_booking = None;

                reply
            } else {
                conv.state = ConversationState::Idle;
                "I'm sorry, something went wrong. Could you start over?".to_string()
            }
        }

        // Customer declines a proposed time
        (ConversationState::Confirming, Intent::Decline) => {
            conv.state = ConversationState::CollectingInfo;
            extracted.message_to_customer.clone()
        }

        // Cancel request
        (_, Intent::Cancel) => {
            let owner_notification = {
                let db = state.db.lock().unwrap();
                let bookings = queries::get_bookings_for_phone(&db, from_phone)?;
                if let Some(next_booking) = bookings.into_iter().next() {
                    queries::update_booking_status(
                        &db,
                        &next_booking.id,
                        &BookingStatus::Cancelled,
                    )?;
                    Some(format!(
                        "Cancelled: {} for {} ({}) at {}",
                        next_booking.customer_name.as_deref().unwrap_or("Unknown"),
                        next_booking.date_time.format("%Y-%m-%d %H:%M"),
                        from_phone,
                        next_booking.id,
                    ))
                } else {
                    None
                }
            };

            if let Some(msg) = &owner_notification {
                notify_owner(state, msg).await;
            }

            conv.state = ConversationState::Idle;
            conv.pending_booking = None;
            if owner_notification.is_some() {
                extracted.message_to_customer.clone()
            } else {
                "I don't see any upcoming bookings to cancel. Would you like to book an appointment instead?".to_string()
            }
        }

        // Reschedule request
        (_, Intent::Reschedule) => {
            let bookings = {
                let db = state.db.lock().unwrap();
                queries::get_bookings_for_phone(&db, from_phone)?
            };

            if let Some(next_booking) = bookings.into_iter().next() {
                // Cancel old booking
                {
                    let db = state.db.lock().unwrap();
                    queries::update_booking_status(
                        &db,
                        &next_booking.id,
                        &BookingStatus::Cancelled,
                    )?;
                }

                // Start new booking flow with existing info
                conv.pending_booking = Some(PendingBooking {
                    customer_name: next_booking.customer_name.or(extracted.customer_name),
                    date_time: make_datetime_string(
                        extracted.requested_date.as_deref(),
                        extracted.requested_time.as_deref(),
                    ),
                    duration_minutes: extracted
                        .duration_minutes
                        .or(Some(next_booking.duration_minutes)),
                    notes: extracted.notes.or(next_booking.notes),
                });

                let has_time = extracted.requested_date.is_some()
                    && extracted.requested_time.is_some();
                conv.state = if has_time {
                    ConversationState::Confirming
                } else {
                    ConversationState::CollectingInfo
                };
            } else {
                conv.state = ConversationState::Idle;
                conv.pending_booking = None;
            }
            extracted.message_to_customer.clone()
        }

        // General question or unknown — LLM handles it, no state change
        (_, Intent::GeneralQuestion | Intent::Unknown) => {
            extracted.message_to_customer.clone()
        }

        // Confirm/Decline outside of Confirming state — treat as general
        (_, Intent::Confirm | Intent::Decline) => {
            extracted.message_to_customer.clone()
        }
    };

    // Append assistant reply
    conv.messages.push(ConversationMessage {
        role: "assistant".to_string(),
        content: reply.clone(),
    });

    // Update timestamps
    let now = Utc::now().naive_utc();
    conv.last_activity = now;
    conv.expires_at = now + Duration::minutes(30);

    // Save conversation
    {
        let db = state.db.lock().unwrap();
        queries::save_conversation(&db, &conv)?;
    }

    Ok(reply)
}

fn new_conversation(phone: &str) -> Conversation {
    let now = Utc::now().naive_utc();
    Conversation {
        phone: phone.to_string(),
        messages: vec![],
        state: ConversationState::Idle,
        pending_booking: None,
        last_activity: now,
        expires_at: now + Duration::minutes(30),
    }
}

fn make_datetime_string(date: Option<&str>, time: Option<&str>) -> Option<String> {
    match (date, time) {
        (Some(d), Some(t)) => Some(format!("{d} {t}")),
        (Some(d), None) => Some(d.to_string()),
        (None, Some(t)) => Some(t.to_string()),
        (None, None) => None,
    }
}

fn create_booking_from_pending(phone: &str, pending: &PendingBooking) -> Booking {
    let now = Utc::now().naive_utc();
    let date_time = pending
        .date_time
        .as_deref()
        .and_then(|dt| {
            chrono::NaiveDateTime::parse_from_str(dt, "%Y-%m-%d %H:%M")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(dt, "%Y-%m-%d %H:%M:%S"))
                .ok()
        })
        .unwrap_or(now);

    Booking {
        id: uuid::Uuid::new_v4().to_string(),
        customer_phone: phone.to_string(),
        customer_name: pending.customer_name.clone(),
        date_time,
        duration_minutes: pending.duration_minutes.unwrap_or(60),
        status: BookingStatus::Confirmed,
        notes: pending.notes.clone(),
        created_at: now,
        updated_at: now,
    }
}

async fn notify_owner(state: &Arc<AppState>, message: &str) {
    if state.config.owner_phone.is_empty() {
        tracing::warn!("owner_phone not configured, skipping notification");
        return;
    }

    if let Err(e) = state
        .messaging
        .send_message(&state.config.owner_phone, message)
        .await
    {
        tracing::error!(error = %e, "failed to notify owner");
    }
}
