use chrono::{NaiveDateTime, Utc};
use rusqlite::{params, Connection};

use crate::models::{
    Booking, BookingStatus, Conversation, ConversationMessage, ConversationState, InboxEvent,
    InboxThread, PendingBooking, User,
};

// ── Conversations ──

pub fn get_conversation(conn: &Connection, phone: &str) -> anyhow::Result<Option<Conversation>> {
    let now = Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut stmt = conn.prepare(
        "SELECT phone, messages, state, last_activity, expires_at FROM conversations WHERE phone = ?1 AND expires_at > ?2",
    )?;

    let result = stmt.query_row(params![phone, now], |row| {
        let messages_json: String = row.get(1)?;
        let state_str: String = row.get(2)?;
        let last_activity_str: String = row.get(3)?;
        let expires_at_str: String = row.get(4)?;

        Ok((
            row.get::<_, String>(0)?,
            messages_json,
            state_str,
            last_activity_str,
            expires_at_str,
        ))
    });

    match result {
        Ok((phone, messages_json, state_str, last_activity_str, expires_at_str)) => {
            let data: serde_json::Value =
                serde_json::from_str(&messages_json).unwrap_or(serde_json::json!({}));

            let (messages, pending_booking): (Vec<ConversationMessage>, Option<PendingBooking>) =
                if data.is_array() {
                    // Legacy format: just an array of messages
                    let msgs = serde_json::from_value(data).unwrap_or_default();
                    (msgs, None)
                } else {
                    let msgs = data
                        .get("messages")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    let pending = data
                        .get("pending_booking")
                        .and_then(|v| serde_json::from_value(v.clone()).ok());
                    (msgs, pending)
                };

            let last_activity =
                NaiveDateTime::parse_from_str(&last_activity_str, "%Y-%m-%d %H:%M:%S")
                    .unwrap_or_else(|_| Utc::now().naive_utc());
            let expires_at =
                NaiveDateTime::parse_from_str(&expires_at_str, "%Y-%m-%d %H:%M:%S")
                    .unwrap_or_else(|_| Utc::now().naive_utc());

            Ok(Some(Conversation {
                phone,
                messages,
                state: ConversationState::parse(&state_str),
                pending_booking,
                last_activity,
                expires_at,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save_conversation(conn: &Connection, conv: &Conversation) -> anyhow::Result<()> {
    let data = serde_json::json!({
        "messages": conv.messages,
        "pending_booking": conv.pending_booking,
    });
    let messages_json = serde_json::to_string(&data)?;
    let state_str = conv.state.as_str();
    let last_activity = conv.last_activity.format("%Y-%m-%d %H:%M:%S").to_string();
    let expires_at = conv.expires_at.format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO conversations (phone, messages, state, last_activity, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(phone) DO UPDATE SET
           messages = excluded.messages,
           state = excluded.state,
           last_activity = excluded.last_activity,
           expires_at = excluded.expires_at",
        params![conv.phone, messages_json, state_str, last_activity, expires_at],
    )?;
    Ok(())
}

pub fn expire_old_conversations(conn: &Connection) -> anyhow::Result<usize> {
    let now = Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();
    let count = conn.execute("DELETE FROM conversations WHERE expires_at <= ?1", params![now])?;
    Ok(count)
}

// ── Bookings ──

pub fn create_booking(conn: &Connection, booking: &Booking) -> anyhow::Result<()> {
    let date_time = booking.date_time.format("%Y-%m-%d %H:%M:%S").to_string();
    let created_at = booking.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
    let updated_at = booking.updated_at.format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO bookings (id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            booking.id,
            booking.customer_phone,
            booking.customer_name,
            date_time,
            booking.duration_minutes,
            booking.status.as_str(),
            booking.notes,
            created_at,
            updated_at,
        ],
    )?;
    Ok(())
}

pub fn get_bookings_for_phone(conn: &Connection, phone: &str) -> anyhow::Result<Vec<Booking>> {
    let mut stmt = conn.prepare(
        "SELECT id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at
         FROM bookings WHERE customer_phone = ?1 AND status != 'cancelled' ORDER BY date_time ASC",
    )?;

    let rows = stmt.query_map(params![phone], |row| {
        Ok(parse_booking_row(row))
    })?;

    let mut bookings = vec![];
    for row in rows {
        bookings.push(row??);
    }
    Ok(bookings)
}

pub fn get_bookings_in_range(
    conn: &Connection,
    start: &NaiveDateTime,
    end: &NaiveDateTime,
) -> anyhow::Result<Vec<Booking>> {
    let start_str = start.format("%Y-%m-%d %H:%M:%S").to_string();
    let end_str = end.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(
        "SELECT id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at
         FROM bookings WHERE date_time >= ?1 AND date_time <= ?2 AND status != 'cancelled' ORDER BY date_time ASC",
    )?;

    let rows = stmt.query_map(params![start_str, end_str], |row| {
        Ok(parse_booking_row(row))
    })?;

    let mut bookings = vec![];
    for row in rows {
        bookings.push(row??);
    }
    Ok(bookings)
}

pub fn update_booking_status(
    conn: &Connection,
    id: &str,
    status: &BookingStatus,
) -> anyhow::Result<bool> {
    let now = Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let count = conn.execute(
        "UPDATE bookings SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), now, id],
    )?;
    Ok(count > 0)
}

pub fn get_all_bookings(
    conn: &Connection,
    status_filter: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<Booking>> {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match status_filter {
        Some(status) => (
            "SELECT id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at \
             FROM bookings WHERE status = ?1 ORDER BY date_time DESC LIMIT ?2"
                .to_string(),
            vec![
                Box::new(status.to_string()) as Box<dyn rusqlite::types::ToSql>,
                Box::new(limit),
            ],
        ),
        None => (
            "SELECT id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at \
             FROM bookings ORDER BY date_time DESC LIMIT ?1"
                .to_string(),
            vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| Ok(parse_booking_row(row)))?;

    let mut bookings = vec![];
    for row in rows {
        bookings.push(row??);
    }
    Ok(bookings)
}

pub fn get_booking_by_id(conn: &Connection, id: &str) -> anyhow::Result<Option<Booking>> {
    let result = conn.query_row(
        "SELECT id, customer_phone, customer_name, date_time, duration_minutes, status, notes, created_at, updated_at \
         FROM bookings WHERE id = ?1",
        params![id],
        |row| Ok(parse_booking_row(row)),
    );

    match result {
        Ok(booking) => Ok(Some(booking?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn get_dashboard_stats(conn: &Connection) -> anyhow::Result<DashboardStats> {
    let now = Utc::now().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string();
    let window = current_hour_window();

    let messages_this_hour: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(message_count), 0) FROM rate_limits WHERE window_start = ?1",
            params![window],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let blocked_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM blocked_numbers", [], |row| row.get(0))
        .unwrap_or(0);

    let upcoming_bookings_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM bookings WHERE date_time > ?1 AND status = 'confirmed'",
            params![now],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(DashboardStats {
        messages_this_hour,
        blocked_count,
        upcoming_bookings_count,
    })
}

pub struct DashboardStats {
    pub messages_this_hour: i64,
    pub blocked_count: i64,
    pub upcoming_bookings_count: i64,
}

fn parse_booking_row(row: &rusqlite::Row) -> anyhow::Result<Booking> {
    let id: String = row.get(0)?;
    let customer_phone: String = row.get(1)?;
    let customer_name: Option<String> = row.get(2)?;
    let date_time_str: String = row.get(3)?;
    let duration_minutes: i32 = row.get(4)?;
    let status_str: String = row.get(5)?;
    let notes: Option<String> = row.get(6)?;
    let created_at_str: String = row.get(7)?;
    let updated_at_str: String = row.get(8)?;

    let date_time = NaiveDateTime::parse_from_str(&date_time_str, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let created_at = NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|_| Utc::now().naive_utc());
    let updated_at = NaiveDateTime::parse_from_str(&updated_at_str, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|_| Utc::now().naive_utc());

    Ok(Booking {
        id,
        customer_phone,
        customer_name,
        date_time,
        duration_minutes,
        status: BookingStatus::parse(&status_str),
        notes,
        created_at,
        updated_at,
    })
}

// ── Blocked Numbers ──

pub fn is_blocked(conn: &Connection, phone: &str) -> anyhow::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM blocked_numbers WHERE phone = ?1",
        params![phone],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn block_number(
    conn: &Connection,
    phone: &str,
    reason: Option<&str>,
    is_auto: bool,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO blocked_numbers (phone, reason, is_auto) VALUES (?1, ?2, ?3)
         ON CONFLICT(phone) DO UPDATE SET reason = excluded.reason, is_auto = excluded.is_auto",
        params![phone, reason, is_auto as i32],
    )?;
    Ok(())
}

pub fn unblock_number(conn: &Connection, phone: &str) -> anyhow::Result<bool> {
    let count = conn.execute(
        "DELETE FROM blocked_numbers WHERE phone = ?1",
        params![phone],
    )?;
    Ok(count > 0)
}

pub fn list_blocked(conn: &Connection) -> anyhow::Result<Vec<(String, Option<String>, bool)>> {
    let mut stmt =
        conn.prepare("SELECT phone, reason, is_auto FROM blocked_numbers ORDER BY created_at DESC")?;
    let rows = stmt.query_map([], |row| {
        let phone: String = row.get(0)?;
        let reason: Option<String> = row.get(1)?;
        let is_auto: bool = row.get::<_, i32>(2)? != 0;
        Ok((phone, reason, is_auto))
    })?;

    let mut blocked = vec![];
    for row in rows {
        blocked.push(row?);
    }
    Ok(blocked)
}

// ── Rate Limits ──

pub fn increment_message_count(conn: &Connection, phone: &str) -> anyhow::Result<i64> {
    let window = current_hour_window();

    conn.execute(
        "INSERT INTO rate_limits (phone_number, message_count, window_start)
         VALUES (?1, 1, ?2)
         ON CONFLICT(phone_number, window_start) DO UPDATE SET message_count = message_count + 1",
        params![phone, window],
    )?;

    let count: i64 = conn.query_row(
        "SELECT message_count FROM rate_limits WHERE phone_number = ?1 AND window_start = ?2",
        params![phone, window],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn check_rate_limit(conn: &Connection, phone: &str, max: i64) -> anyhow::Result<bool> {
    let window = current_hour_window();
    let count: i64 = conn
        .query_row(
            "SELECT message_count FROM rate_limits WHERE phone_number = ?1 AND window_start = ?2",
            params![phone, window],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count <= max)
}

pub fn get_global_message_count(conn: &Connection) -> anyhow::Result<i64> {
    let window = current_hour_window();
    let count: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(message_count), 0) FROM rate_limits WHERE window_start = ?1",
            params![window],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count)
}

pub fn cleanup_old_windows(conn: &Connection) -> anyhow::Result<()> {
    let cutoff = (Utc::now() - chrono::Duration::hours(2))
        .format("%Y-%m-%d %H:00:00")
        .to_string();
    conn.execute(
        "DELETE FROM rate_limits WHERE window_start < ?1",
        params![cutoff],
    )?;
    Ok(())
}

fn current_hour_window() -> String {
    Utc::now().format("%Y-%m-%d %H:00:00").to_string()
}

fn current_month() -> String {
    Utc::now().format("%Y-%m").to_string()
}

// ── Monthly Activity ──

pub struct MonthlyActivity {
    pub month: String,
    pub messages_received: i64,
    pub messages_sent: i64,
    pub bookings_created: i64,
    pub bookings_cancelled: i64,
    pub bookings_rescheduled: i64,
}

pub fn increment_monthly_received(conn: &Connection) -> anyhow::Result<()> {
    let month = current_month();
    conn.execute(
        "INSERT INTO monthly_activity (month, messages_received) VALUES (?1, 1)
         ON CONFLICT(month) DO UPDATE SET messages_received = messages_received + 1",
        params![month],
    )?;
    Ok(())
}

pub fn increment_monthly_sent(conn: &Connection) -> anyhow::Result<()> {
    let month = current_month();
    conn.execute(
        "INSERT INTO monthly_activity (month, messages_sent) VALUES (?1, 1)
         ON CONFLICT(month) DO UPDATE SET messages_sent = messages_sent + 1",
        params![month],
    )?;
    Ok(())
}

pub fn increment_monthly_bookings(conn: &Connection) -> anyhow::Result<()> {
    let month = current_month();
    conn.execute(
        "INSERT INTO monthly_activity (month, bookings_created) VALUES (?1, 1)
         ON CONFLICT(month) DO UPDATE SET bookings_created = bookings_created + 1",
        params![month],
    )?;
    Ok(())
}

pub fn increment_monthly_cancelled(conn: &Connection) -> anyhow::Result<()> {
    let month = current_month();
    conn.execute(
        "INSERT INTO monthly_activity (month, bookings_cancelled) VALUES (?1, 1)
         ON CONFLICT(month) DO UPDATE SET bookings_cancelled = bookings_cancelled + 1",
        params![month],
    )?;
    Ok(())
}

pub fn increment_monthly_rescheduled(conn: &Connection) -> anyhow::Result<()> {
    let month = current_month();
    conn.execute(
        "INSERT INTO monthly_activity (month, bookings_rescheduled) VALUES (?1, 1)
         ON CONFLICT(month) DO UPDATE SET bookings_rescheduled = bookings_rescheduled + 1",
        params![month],
    )?;
    Ok(())
}

pub fn get_recent_monthly_activity(conn: &Connection, months: usize) -> anyhow::Result<Vec<MonthlyActivity>> {
    let now = Utc::now();
    let mut result = Vec::with_capacity(months);

    for i in 0..months {
        let date = now - chrono::Months::new(i as u32);
        let month = date.format("%Y-%m").to_string();

        let activity = conn.query_row(
            "SELECT month, messages_received, messages_sent, bookings_created, bookings_cancelled, bookings_rescheduled
             FROM monthly_activity WHERE month = ?1",
            params![month],
            |row| {
                Ok(MonthlyActivity {
                    month: row.get(0)?,
                    messages_received: row.get(1)?,
                    messages_sent: row.get(2)?,
                    bookings_created: row.get(3)?,
                    bookings_cancelled: row.get(4)?,
                    bookings_rescheduled: row.get(5)?,
                })
            },
        );

        result.push(match activity {
            Ok(a) => a,
            Err(rusqlite::Error::QueryReturnedNoRows) => MonthlyActivity {
                month,
                messages_received: 0,
                messages_sent: 0,
                bookings_created: 0,
                bookings_cancelled: 0,
                bookings_rescheduled: 0,
            },
            Err(e) => return Err(e.into()),
        });
    }

    // Return oldest first
    result.reverse();
    Ok(result)
}

// ── Users ──

pub fn get_user(conn: &Connection, id: &str) -> anyhow::Result<Option<User>> {
    let result = conn.query_row(
        "SELECT id, business_name, owner_name, owner_phone, twilio_account_sid, twilio_auth_token, twilio_phone_number, availability, timezone, ai_preferences
         FROM users WHERE id = ?1",
        params![id],
        |row| {
            Ok(User {
                id: row.get(0)?,
                business_name: row.get(1)?,
                owner_name: row.get(2)?,
                owner_phone: row.get(3)?,
                twilio_account_sid: row.get(4)?,
                twilio_auth_token: row.get(5)?,
                twilio_phone_number: row.get(6)?,
                availability: row.get(7)?,
                timezone: row.get(8)?,
                ai_preferences: row.get(9)?,
            })
        },
    );

    match result {
        Ok(user) => Ok(Some(user)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save_user(conn: &Connection, user: &User) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO users (id, business_name, owner_name, owner_phone, twilio_account_sid, twilio_auth_token, twilio_phone_number, availability, timezone, ai_preferences)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
           business_name = excluded.business_name,
           owner_name = excluded.owner_name,
           owner_phone = excluded.owner_phone,
           twilio_account_sid = excluded.twilio_account_sid,
           twilio_auth_token = excluded.twilio_auth_token,
           twilio_phone_number = excluded.twilio_phone_number,
           availability = excluded.availability,
           timezone = excluded.timezone,
           ai_preferences = excluded.ai_preferences,
           updated_at = datetime('now')",
        params![
            user.id,
            user.business_name,
            user.owner_name,
            user.owner_phone,
            user.twilio_account_sid,
            user.twilio_auth_token,
            user.twilio_phone_number,
            user.availability,
            user.timezone,
            user.ai_preferences,
        ],
    )?;
    Ok(())
}

// ── Inbox Events ──

pub fn insert_inbox_event(
    conn: &Connection,
    phone: &str,
    kind: &str,
    content: &str,
) -> anyhow::Result<i64> {
    conn.execute(
        "INSERT INTO inbox_events (phone, kind, content) VALUES (?1, ?2, ?3)",
        params![phone, kind, content],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_inbox_threads(conn: &Connection) -> anyhow::Result<Vec<InboxThread>> {
    let mut stmt = conn.prepare(
        "SELECT e.phone, e.content, e.kind, e.created_at,
                (SELECT COUNT(*) FROM inbox_events e2 WHERE e2.phone = e.phone AND e2.is_read = 0) as unread_count
         FROM inbox_events e
         INNER JOIN (
             SELECT phone, MAX(id) as max_id FROM inbox_events GROUP BY phone
         ) latest ON e.id = latest.max_id
         ORDER BY e.created_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(InboxThread {
            phone: row.get(0)?,
            last_message: row.get(1)?,
            last_kind: row.get(2)?,
            last_activity: row.get(3)?,
            unread_count: row.get(4)?,
        })
    })?;

    let mut threads = vec![];
    for row in rows {
        threads.push(row?);
    }
    Ok(threads)
}

pub fn get_thread_events(
    conn: &Connection,
    phone: &str,
    limit: i64,
) -> anyhow::Result<Vec<InboxEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, phone, kind, content, is_read, created_at
         FROM inbox_events WHERE phone = ?1
         ORDER BY id ASC LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![phone, limit], |row| {
        Ok(InboxEvent {
            id: row.get(0)?,
            phone: row.get(1)?,
            kind: row.get(2)?,
            content: row.get(3)?,
            is_read: row.get::<_, i32>(4)? != 0,
            created_at: row.get(5)?,
        })
    })?;

    let mut events = vec![];
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

pub fn get_inbox_events_since(conn: &Connection, since_id: i64) -> anyhow::Result<Vec<InboxEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, phone, kind, content, is_read, created_at
         FROM inbox_events WHERE id > ?1
         ORDER BY id ASC",
    )?;

    let rows = stmt.query_map(params![since_id], |row| {
        Ok(InboxEvent {
            id: row.get(0)?,
            phone: row.get(1)?,
            kind: row.get(2)?,
            content: row.get(3)?,
            is_read: row.get::<_, i32>(4)? != 0,
            created_at: row.get(5)?,
        })
    })?;

    let mut events = vec![];
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

pub fn mark_thread_read(conn: &Connection, phone: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE inbox_events SET is_read = 1 WHERE phone = ?1 AND is_read = 0",
        params![phone],
    )?;
    Ok(())
}

// ── Contacts ──

pub struct ContactSummary {
    pub phone: String,
    pub name: Option<String>,
    pub total_bookings: i64,
    pub last_booking: Option<String>,
    pub first_seen: String,
}

pub fn get_contacts(conn: &Connection, limit: i64) -> anyhow::Result<Vec<ContactSummary>> {
    let mut stmt = conn.prepare(
        "SELECT
            ie.phone,
            b_agg.customer_name,
            COALESCE(b_agg.total_bookings, 0),
            b_agg.last_booking,
            MIN(ie.created_at) as first_seen
         FROM inbox_events ie
         LEFT JOIN (
             SELECT customer_phone,
                    MAX(customer_name) as customer_name,
                    COUNT(*) as total_bookings,
                    MAX(date_time) as last_booking
             FROM bookings
             WHERE status != 'cancelled'
             GROUP BY customer_phone
         ) b_agg ON ie.phone = b_agg.customer_phone
         GROUP BY ie.phone
         ORDER BY MAX(ie.created_at) DESC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map(params![limit], |row| {
        Ok(ContactSummary {
            phone: row.get(0)?,
            name: row.get(1)?,
            total_bookings: row.get(2)?,
            last_booking: row.get(3)?,
            first_seen: row.get(4)?,
        })
    })?;

    let mut contacts = vec![];
    for row in rows {
        contacts.push(row?);
    }
    Ok(contacts)
}
