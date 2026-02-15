use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::db::queries;
use crate::models::{
    AiPreferences, Availability, Booking, BookingStatus, Conversation, ConversationMessage,
    ConversationState, Intent, PendingBooking,
};
use crate::services::ai::intent::extract_intent;
use crate::services::inbox::record_inbox_event;
use crate::services::scheduling::validate_booking_time;
use crate::state::{AppState, DevNotification, DevNotificationKind};

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

    // Load user settings
    let user = {
        let db = state.db.lock().unwrap();
        queries::get_user(&db, "default").ok().flatten()
    };

    let availability = user
        .as_ref()
        .and_then(|u| u.availability.as_deref())
        .and_then(|s| Availability::from_json(s).ok());

    let ai_preferences = user
        .as_ref()
        .and_then(|u| u.ai_preferences.as_deref())
        .and_then(|s| AiPreferences::from_json(s).ok());

    // Append user message
    conv.messages.push(ConversationMessage {
        role: "user".to_string(),
        content: message.to_string(),
    });

    // Forward customer message to owner via dev notification queue
    if let Ok(mut notifications) = state.dev_notifications.lock() {
        notifications.push(DevNotification {
            phone: Some(from_phone.to_string()),
            kind: DevNotificationKind::CustomerMessage,
            content: message.to_string(),
        });
    }
    record_inbox_event(state, from_phone, "customer_message", message);

    // Build business context
    let mut business_context = format!(
        "Business phone: {}. Owner phone: {}.",
        state.config.twilio_phone_number, state.config.owner_phone,
    );
    if let Some(ref avail) = availability {
        let hours = avail.to_human_readable();
        if !hours.is_empty() {
            business_context.push_str(&format!(" Business hours: {hours}."));
        }
    }

    // Extract intent via LLM
    let extracted = extract_intent(
        state.llm.as_ref(),
        &conv.messages,
        message,
        &business_context,
        ai_preferences.as_ref(),
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
                let pending = PendingBooking {
                    customer_name: extracted.customer_name,
                    date_time: make_datetime_string(
                        extracted.requested_date.as_deref(),
                        extracted.requested_time.as_deref(),
                    ),
                    duration_minutes: extracted.duration_minutes,
                    notes: extracted.notes,
                };

                // Validate proposed time
                if let Some(ref dt_str) = pending.date_time {
                    if let Some(validation_err) = try_validate_time(
                        state,
                        dt_str,
                        pending.duration_minutes.unwrap_or(60),
                        availability.as_ref(),
                    ) {
                        conv.pending_booking = Some(pending);
                        conv.state = ConversationState::CollectingInfo;
                        return finish_conversation(state, &mut conv, &validation_err).await;
                    }
                }

                conv.pending_booking = Some(pending);
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
                // Validate before transitioning to Confirming
                let should_confirm =
                    if let Some(ref dt_str) = conv.pending_booking.as_ref().and_then(|p| p.date_time.clone()) {
                        let dur = conv.pending_booking.as_ref().and_then(|p| p.duration_minutes).unwrap_or(60);
                        if let Some(validation_err) = try_validate_time(state, dt_str, dur, availability.as_ref()) {
                            conv.state = ConversationState::CollectingInfo;
                            return finish_conversation(state, &mut conv, &validation_err).await;
                        }
                        true
                    } else {
                        true
                    };

                if should_confirm {
                    conv.state = ConversationState::Confirming;
                }
            }

            extracted.message_to_customer.clone()
        }

        // Customer confirms a proposed booking
        (ConversationState::Confirming, Intent::Confirm) => {
            if let Some(ref pending) = conv.pending_booking {
                // Final validation before creating booking
                if let Some(ref dt_str) = pending.date_time {
                    let dur = pending.duration_minutes.unwrap_or(60);
                    if let Some(validation_err) = try_validate_time(state, dt_str, dur, availability.as_ref()) {
                        conv.state = ConversationState::CollectingInfo;
                        return finish_conversation(state, &mut conv, &validation_err).await;
                    }
                }

                let booking = create_booking_from_pending(from_phone, pending);
                let ics_link = format!(
                    "/calendar/{}.ics",
                    booking.id
                );
                let reply = format!(
                    "{}\n\nAdd to calendar: {}",
                    extracted.message_to_customer,
                    ics_link,
                );

                // Save booking to DB
                {
                    let db = state.db.lock().unwrap();
                    queries::create_booking(&db, &booking)?;
                    let _ = queries::increment_monthly_bookings(&db);
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
                notify_owner(state, &owner_msg, Some(from_phone)).await;

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
                    let _ = queries::increment_monthly_cancelled(&db);
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
                notify_owner(state, msg, Some(from_phone)).await;
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
                // Cancel old booking for rescheduling
                {
                    let db = state.db.lock().unwrap();
                    queries::update_booking_status(
                        &db,
                        &next_booking.id,
                        &BookingStatus::Cancelled,
                    )?;
                    let _ = queries::increment_monthly_rescheduled(&db);
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

                if has_time {
                    if let Some(ref dt_str) = conv.pending_booking.as_ref().and_then(|p| p.date_time.clone()) {
                        let dur = conv.pending_booking.as_ref().and_then(|p| p.duration_minutes).unwrap_or(60);
                        if let Some(validation_err) = try_validate_time(state, dt_str, dur, availability.as_ref()) {
                            conv.state = ConversationState::CollectingInfo;
                            return finish_conversation(state, &mut conv, &validation_err).await;
                        }
                    }
                    conv.state = ConversationState::Confirming;
                } else {
                    conv.state = ConversationState::CollectingInfo;
                }
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

    // Forward AI reply to owner via dev notification queue
    if let Ok(mut notifications) = state.dev_notifications.lock() {
        notifications.push(DevNotification {
            phone: Some(from_phone.to_string()),
            kind: DevNotificationKind::AiReply,
            content: reply.clone(),
        });
    }
    record_inbox_event(state, from_phone, "ai_reply", &reply);

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

pub fn inject_owner_reply(state: &Arc<AppState>, to_phone: &str, message: &str) -> anyhow::Result<()> {
    let db = state.db.lock().unwrap();
    let mut conv = queries::get_conversation(&db, to_phone)?
        .unwrap_or_else(|| new_conversation(to_phone));
    conv.messages.push(ConversationMessage {
        role: "assistant".to_string(),
        content: message.to_string(),
    });
    let now = Utc::now().naive_utc();
    conv.last_activity = now;
    conv.expires_at = now + Duration::minutes(30);
    queries::save_conversation(&db, &conv)?;
    Ok(())
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

fn try_validate_time(
    state: &Arc<AppState>,
    dt_str: &str,
    duration_minutes: i32,
    availability: Option<&Availability>,
) -> Option<String> {
    let dt = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S"))
        .ok()?;

    let db = state.db.lock().unwrap();
    match validate_booking_time(&db, &dt, duration_minutes, availability) {
        Ok(()) => None,
        Err(e) => Some(e.to_string()),
    }
}

async fn finish_conversation(
    state: &Arc<AppState>,
    conv: &mut Conversation,
    reply: &str,
) -> anyhow::Result<String> {
    conv.messages.push(ConversationMessage {
        role: "assistant".to_string(),
        content: reply.to_string(),
    });
    // Forward AI reply to owner via dev notification queue
    if let Ok(mut notifications) = state.dev_notifications.lock() {
        notifications.push(DevNotification {
            phone: Some(conv.phone.clone()),
            kind: DevNotificationKind::AiReply,
            content: reply.to_string(),
        });
    }
    record_inbox_event(state, &conv.phone, "ai_reply", reply);
    let now = Utc::now().naive_utc();
    conv.last_activity = now;
    conv.expires_at = now + Duration::minutes(30);
    {
        let db = state.db.lock().unwrap();
        queries::save_conversation(&db, conv)?;
    }
    Ok(reply.to_string())
}

async fn notify_owner(state: &Arc<AppState>, message: &str, phone: Option<&str>) {
    // Always push to dev notification queue
    if let Ok(mut notifications) = state.dev_notifications.lock() {
        notifications.push(DevNotification {
            phone: phone.map(|p| p.to_string()),
            kind: DevNotificationKind::System,
            content: message.to_string(),
        });
    }
    if let Some(p) = phone {
        record_inbox_event(state, p, "system", message);
    }

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
    } else {
        let db = state.db.lock().unwrap();
        let _ = queries::increment_monthly_sent(&db);
    }
}
