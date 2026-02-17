use chrono::{Duration, NaiveDateTime};
use rusqlite::Connection;

use crate::db::queries;
use crate::models::Availability;

#[derive(Debug)]
pub enum SchedulingError {
    OutsideBusinessHours { hours: String },
    Conflict,
}

impl std::fmt::Display for SchedulingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulingError::OutsideBusinessHours { hours } => {
                write!(
                    f,
                    "That time is outside our business hours. We're available: {hours}"
                )
            }
            SchedulingError::Conflict => {
                write!(
                    f,
                    "Sorry, that time slot is already booked. Could you pick a different time?"
                )
            }
        }
    }
}

pub fn validate_booking_time(
    conn: &Connection,
    dt: &NaiveDateTime,
    duration_minutes: i32,
    availability: Option<&Availability>,
) -> Result<(), SchedulingError> {
    // Check availability if configured
    if let Some(avail) = availability {
        if !avail.effective_slots().is_empty() {
            if !avail.is_available(dt) {
                return Err(SchedulingError::OutsideBusinessHours {
                    hours: avail.to_human_readable(),
                });
            }
            if !avail.end_time_within_slot(dt, duration_minutes) {
                return Err(SchedulingError::OutsideBusinessHours {
                    hours: avail.to_human_readable(),
                });
            }
        }
    }

    // Check for conflicts with existing bookings
    let day_start = dt.date().and_hms_opt(0, 0, 0).unwrap_or(*dt);
    let day_end = dt.date().and_hms_opt(23, 59, 59).unwrap_or(*dt);

    let bookings = queries::get_bookings_in_range(conn, &day_start, &day_end)
        .map_err(|_| SchedulingError::Conflict)?;

    let proposed_end = *dt + Duration::minutes(duration_minutes as i64);

    for booking in &bookings {
        let booking_end =
            booking.date_time + Duration::minutes(booking.duration_minutes as i64);
        // Overlap: booking starts before proposed ends AND booking ends after proposed starts
        if booking.date_time < proposed_end && booking_end > *dt {
            return Err(SchedulingError::Conflict);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Booking, BookingStatus};

    fn setup_db() -> Connection {
        db::init_db(":memory:").unwrap()
    }

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn make_avail(json: &str) -> Availability {
        Availability::from_json(json).unwrap()
    }

    #[test]
    fn test_valid_time_no_availability() {
        let conn = setup_db();
        let result = validate_booking_time(&conn, &dt("2025-06-16 10:00"), 60, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_outside_business_hours() {
        let conn = setup_db();
        let avail = make_avail(
            r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#,
        );
        // 2025-06-16 is Monday, 20:00 is outside hours
        let result = validate_booking_time(&conn, &dt("2025-06-16 20:00"), 60, Some(&avail));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchedulingError::OutsideBusinessHours { .. }));
    }

    #[test]
    fn test_end_time_exceeds_slot() {
        let conn = setup_db();
        let avail = make_avail(
            r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#,
        );
        // 16:30 + 60min = 17:30, exceeds 17:00 end
        let result = validate_booking_time(&conn, &dt("2025-06-16 16:30"), 60, Some(&avail));
        assert!(result.is_err());
    }

    #[test]
    fn test_conflict_with_existing_booking() {
        let conn = setup_db();
        let now = chrono::Utc::now().naive_utc();

        let booking = Booking {
            id: "existing-1".to_string(),
            customer_phone: "+15551110000".to_string(),
            customer_name: Some("Alice".to_string()),
            date_time: dt("2025-06-16 10:00"),
            duration_minutes: 60,
            status: BookingStatus::Confirmed,
            notes: None,
            created_at: now,
            updated_at: now,
        };
        queries::create_booking(&conn, &booking).unwrap();

        // Proposing 10:30 overlaps with 10:00-11:00
        let result = validate_booking_time(&conn, &dt("2025-06-16 10:30"), 60, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SchedulingError::Conflict));
    }

    #[test]
    fn test_no_conflict_adjacent_booking() {
        let conn = setup_db();
        let now = chrono::Utc::now().naive_utc();

        let booking = Booking {
            id: "existing-2".to_string(),
            customer_phone: "+15551110000".to_string(),
            customer_name: Some("Alice".to_string()),
            date_time: dt("2025-06-16 10:00"),
            duration_minutes: 60,
            status: BookingStatus::Confirmed,
            notes: None,
            created_at: now,
            updated_at: now,
        };
        queries::create_booking(&conn, &booking).unwrap();

        // 11:00 starts exactly when previous ends — no overlap
        let result = validate_booking_time(&conn, &dt("2025-06-16 11:00"), 60, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_valid_time_within_hours_no_conflict() {
        let conn = setup_db();
        let avail = make_avail(
            r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#,
        );
        let result = validate_booking_time(&conn, &dt("2025-06-16 10:00"), 60, Some(&avail));
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_slots_skips_availability_check() {
        let conn = setup_db();
        let avail = make_avail(r#"{"slots":[]}"#);
        // Sunday 20:00 — would fail if slots were checked, but empty slots = no restriction
        let result = validate_booking_time(&conn, &dt("2025-06-15 20:00"), 60, Some(&avail));
        assert!(result.is_ok());
    }
}
