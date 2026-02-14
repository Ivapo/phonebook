use std::sync::Arc;

use crate::db::queries;
use crate::models::InboxEvent;
use crate::state::AppState;

pub fn record_inbox_event(state: &Arc<AppState>, phone: &str, kind: &str, content: &str) {
    let event_id = {
        let db = state.db.lock().unwrap();
        queries::insert_inbox_event(&db, phone, kind, content)
    };

    match event_id {
        Ok(id) => {
            let event = InboxEvent {
                id,
                phone: phone.to_string(),
                kind: kind.to_string(),
                content: content.to_string(),
                is_read: false,
                created_at: chrono::Utc::now()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            };
            // Broadcast to SSE subscribers; ignore if no receivers
            let _ = state.inbox_tx.send(event);
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to record inbox event");
        }
    }
}
