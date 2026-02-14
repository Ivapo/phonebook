use chrono::Duration;

use crate::models::Booking;

pub fn generate_ics(booking: &Booking, business_name: &str) -> String {
    let dtstart = booking.date_time.format("%Y%m%dT%H%M%S").to_string();
    let dtend = (booking.date_time + Duration::minutes(booking.duration_minutes as i64))
        .format("%Y%m%dT%H%M%S")
        .to_string();
    let dtstamp = booking.created_at.format("%Y%m%dT%H%M%S").to_string();
    let uid = format!("{}@phonebook", booking.id);

    let summary = format!(
        "Appointment with {}",
        business_name
    );
    let description = booking
        .notes
        .as_deref()
        .unwrap_or("No additional notes");

    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//Phonebook//Booking Agent//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:{dtstamp}\r\n\
         DTSTART:{dtstart}\r\n\
         DTEND:{dtend}\r\n\
         SUMMARY:{summary}\r\n\
         DESCRIPTION:{description}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use crate::models::{Booking, BookingStatus};

    #[test]
    fn test_generate_ics() {
        let booking = Booking {
            id: "test-123".to_string(),
            customer_phone: "+1234567890".to_string(),
            customer_name: Some("Alice".to_string()),
            date_time: NaiveDateTime::parse_from_str("2025-03-15 14:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            duration_minutes: 60,
            status: BookingStatus::Confirmed,
            notes: Some("Haircut".to_string()),
            created_at: NaiveDateTime::parse_from_str("2025-03-10 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            updated_at: NaiveDateTime::parse_from_str("2025-03-10 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        };

        let ics = generate_ics(&booking, "Bob's Barbershop");
        assert!(ics.contains("BEGIN:VCALENDAR"));
        assert!(ics.contains("BEGIN:VEVENT"));
        assert!(ics.contains("DTSTART:20250315T140000"));
        assert!(ics.contains("DTEND:20250315T150000"));
        assert!(ics.contains("SUMMARY:Appointment with Bob's Barbershop"));
        assert!(ics.contains("DESCRIPTION:Haircut"));
        assert!(ics.contains("UID:test-123@phonebook"));
        assert!(ics.contains("END:VEVENT"));
        assert!(ics.contains("END:VCALENDAR"));
    }

    #[test]
    fn test_generate_ics_no_notes() {
        let booking = Booking {
            id: "test-456".to_string(),
            customer_phone: "+1234567890".to_string(),
            customer_name: None,
            date_time: NaiveDateTime::parse_from_str("2025-04-01 09:30:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            duration_minutes: 30,
            status: BookingStatus::Confirmed,
            notes: None,
            created_at: NaiveDateTime::parse_from_str("2025-03-25 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            updated_at: NaiveDateTime::parse_from_str("2025-03-25 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        };

        let ics = generate_ics(&booking, "Test Biz");
        assert!(ics.contains("DTSTART:20250401T093000"));
        assert!(ics.contains("DTEND:20250401T100000"));
        assert!(ics.contains("DESCRIPTION:No additional notes"));
    }
}
