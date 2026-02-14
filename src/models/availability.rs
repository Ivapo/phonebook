use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSlot {
    pub day: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Availability {
    pub slots: Vec<TimeSlot>,
}

impl Availability {
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        let availability: Availability = serde_json::from_str(s)?;
        for slot in &availability.slots {
            parse_weekday(&slot.day)?;
            parse_time(&slot.start)?;
            parse_time(&slot.end)?;
        }
        Ok(availability)
    }

    pub fn is_available(&self, dt: &chrono::NaiveDateTime) -> bool {
        let weekday = dt.format("%a").to_string().to_lowercase();
        let time = dt.format("%H:%M").to_string();

        self.slots.iter().any(|slot| {
            slot.day.to_lowercase() == weekday && time >= slot.start && time < slot.end
        })
    }

    pub fn end_time_within_slot(&self, dt: &chrono::NaiveDateTime, duration_minutes: i32) -> bool {
        let end_dt = *dt + chrono::Duration::minutes(duration_minutes as i64);
        let weekday = dt.format("%a").to_string().to_lowercase();
        let start_time = dt.format("%H:%M").to_string();
        let end_time = end_dt.format("%H:%M").to_string();

        self.slots.iter().any(|slot| {
            slot.day.to_lowercase() == weekday
                && start_time >= slot.start
                && end_time <= slot.end
        })
    }

    pub fn to_human_readable(&self) -> String {
        if self.slots.is_empty() {
            return String::new();
        }

        let day_order = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

        let mut sorted_slots = self.slots.clone();
        sorted_slots.sort_by(|a, b| {
            let a_idx = day_order
                .iter()
                .position(|d| *d == a.day.to_lowercase())
                .unwrap_or(7);
            let b_idx = day_order
                .iter()
                .position(|d| *d == b.day.to_lowercase())
                .unwrap_or(7);
            a_idx.cmp(&b_idx)
        });

        sorted_slots
            .iter()
            .map(|s| {
                let day = capitalize(&s.day);
                format!("{day}: {}-{}", s.start, s.end)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + &c.as_str().to_lowercase(),
    }
}

fn parse_weekday(s: &str) -> anyhow::Result<()> {
    match s.to_lowercase().as_str() {
        "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun" => Ok(()),
        _ => Err(anyhow::anyhow!("invalid weekday: {s}")),
    }
}

fn parse_time(s: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("invalid time format: {s}"));
    }
    let hour: u32 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid hour in: {s}"))?;
    let minute: u32 = parts[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid minute in: {s}"))?;
    if hour > 23 || minute > 59 {
        return Err(anyhow::anyhow!("time out of range: {s}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"},{"day":"tue","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        assert_eq!(avail.slots.len(), 2);
        assert_eq!(avail.slots[0].day, "mon");
    }

    #[test]
    fn test_parse_invalid_json() {
        assert!(Availability::from_json("not json").is_err());
    }

    #[test]
    fn test_parse_invalid_day() {
        let json = r#"{"slots":[{"day":"xyz","start":"09:00","end":"17:00"}]}"#;
        assert!(Availability::from_json(json).is_err());
    }

    #[test]
    fn test_parse_invalid_time() {
        let json = r#"{"slots":[{"day":"mon","start":"25:00","end":"17:00"}]}"#;
        assert!(Availability::from_json(json).is_err());
    }

    #[test]
    fn test_is_available_within_hours() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-16 is a Monday
        assert!(avail.is_available(&dt("2025-06-16 10:00")));
        assert!(avail.is_available(&dt("2025-06-16 09:00")));
        assert!(avail.is_available(&dt("2025-06-16 16:59")));
    }

    #[test]
    fn test_is_available_outside_hours() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-16 is a Monday
        assert!(!avail.is_available(&dt("2025-06-16 08:00")));
        assert!(!avail.is_available(&dt("2025-06-16 17:00")));
        assert!(!avail.is_available(&dt("2025-06-16 20:00")));
    }

    #[test]
    fn test_is_available_wrong_day() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-17 is a Tuesday
        assert!(!avail.is_available(&dt("2025-06-17 10:00")));
    }

    #[test]
    fn test_end_time_within_slot() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-16 is a Monday
        assert!(avail.end_time_within_slot(&dt("2025-06-16 09:00"), 60));
        assert!(avail.end_time_within_slot(&dt("2025-06-16 16:00"), 60));
        assert!(!avail.end_time_within_slot(&dt("2025-06-16 16:30"), 60));
    }

    #[test]
    fn test_to_human_readable() {
        let json = r#"{"slots":[{"day":"fri","start":"10:00","end":"16:00"},{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        let readable = avail.to_human_readable();
        assert_eq!(readable, "Mon: 09:00-17:00, Fri: 10:00-16:00");
    }

    #[test]
    fn test_to_human_readable_empty() {
        let json = r#"{"slots":[]}"#;
        let avail = Availability::from_json(json).unwrap();
        assert_eq!(avail.to_human_readable(), "");
    }
}
