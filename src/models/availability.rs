use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSlot {
    pub day: String,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakSlot {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayOverride {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Availability {
    pub slots: Vec<TimeSlot>,
    #[serde(default)]
    pub overrides: HashMap<String, DayOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_size: Option<u32>,
    #[serde(default)]
    pub breaks: Vec<BreakSlot>,
}

const DAY_ORDER: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

impl Availability {
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        let availability: Availability = serde_json::from_str(s)?;
        for slot in &availability.slots {
            parse_weekday(&slot.day)?;
            parse_time(&slot.start)?;
            parse_time(&slot.end)?;
        }
        for (date_key, ovr) in &availability.overrides {
            if chrono::NaiveDate::parse_from_str(date_key, "%Y-%m-%d").is_err() {
                return Err(anyhow::anyhow!("invalid override date: {date_key}"));
            }
            if let Some(ref start) = ovr.start {
                parse_time(start)?;
            }
            if let Some(ref end) = ovr.end {
                parse_time(end)?;
            }
        }
        if let Some(ref d) = availability.day_from {
            parse_weekday(d)?;
        }
        if let Some(ref d) = availability.day_to {
            parse_weekday(d)?;
        }
        if let Some(ref t) = availability.time_from {
            parse_time(t)?;
        }
        if let Some(ref t) = availability.time_to {
            parse_time(t)?;
        }
        for brk in &availability.breaks {
            parse_time(&brk.start)?;
            parse_time(&brk.end)?;
        }
        Ok(availability)
    }

    /// Returns effective slots — generated from day/time range if new fields are present,
    /// otherwise falls back to legacy `slots`.
    pub fn effective_slots(&self) -> Vec<TimeSlot> {
        if let (Some(ref day_from), Some(ref day_to), Some(ref time_from), Some(ref time_to)) =
            (&self.day_from, &self.day_to, &self.time_from, &self.time_to)
        {
            let from_idx = day_index(day_from);
            let to_idx = day_index(day_to);
            if let (Some(fi), Some(ti)) = (from_idx, to_idx) {
                let mut days = Vec::new();
                if fi <= ti {
                    // Normal range: mon..fri
                    for day in &DAY_ORDER[fi..=ti] {
                        days.push((*day).to_string());
                    }
                } else {
                    // Wrap-around: fri..mon
                    for day in &DAY_ORDER[fi..] {
                        days.push((*day).to_string());
                    }
                    for day in &DAY_ORDER[..=ti] {
                        days.push((*day).to_string());
                    }
                }
                return days
                    .into_iter()
                    .map(|day| TimeSlot {
                        day,
                        start: time_from.clone(),
                        end: time_to.clone(),
                    })
                    .collect();
            }
        }
        self.slots.clone()
    }

    /// Check if a given HH:MM time falls within any break.
    pub fn is_during_break(&self, time: &str) -> bool {
        self.breaks
            .iter()
            .any(|b| time >= b.start.as_str() && time < b.end.as_str())
    }

    /// Check if a time range [start, end) overlaps any break.
    pub fn overlaps_break(&self, start: &str, end: &str) -> bool {
        self.breaks
            .iter()
            .any(|b| start < b.end.as_str() && end > b.start.as_str())
    }

    pub fn is_available(&self, dt: &chrono::NaiveDateTime) -> bool {
        let date_key = dt.format("%Y-%m-%d").to_string();

        if let Some(ovr) = self.overrides.get(&date_key) {
            if !ovr.available {
                return false;
            }
            if let (Some(ref start), Some(ref end)) = (&ovr.start, &ovr.end) {
                let time = dt.format("%H:%M").to_string();
                return time >= *start && time < *end && !self.is_during_break(&time);
            }
        }

        let weekday = dt.format("%a").to_string().to_lowercase();
        let time = dt.format("%H:%M").to_string();

        let slots = self.effective_slots();
        let in_slot = slots.iter().any(|slot| {
            slot.day.to_lowercase() == weekday && time >= slot.start && time < slot.end
        });

        in_slot && !self.is_during_break(&time)
    }

    pub fn end_time_within_slot(&self, dt: &chrono::NaiveDateTime, duration_minutes: i32) -> bool {
        let end_dt = *dt + chrono::Duration::minutes(duration_minutes as i64);
        let date_key = dt.format("%Y-%m-%d").to_string();

        if let Some(ovr) = self.overrides.get(&date_key) {
            if !ovr.available {
                return false;
            }
            if let (Some(ref start), Some(ref end)) = (&ovr.start, &ovr.end) {
                let start_time = dt.format("%H:%M").to_string();
                let end_time = end_dt.format("%H:%M").to_string();
                return start_time >= *start
                    && end_time <= *end
                    && !self.overlaps_break(&start_time, &end_time);
            }
        }

        let weekday = dt.format("%a").to_string().to_lowercase();
        let start_time = dt.format("%H:%M").to_string();
        let end_time = end_dt.format("%H:%M").to_string();

        let slots = self.effective_slots();
        let in_slot = slots.iter().any(|slot| {
            slot.day.to_lowercase() == weekday
                && start_time >= slot.start
                && end_time <= slot.end
        });

        in_slot && !self.overlaps_break(&start_time, &end_time)
    }

    pub fn to_human_readable(&self) -> String {
        let slots = self.effective_slots();
        if slots.is_empty() {
            return String::new();
        }

        let mut sorted_slots = slots;
        sorted_slots.sort_by(|a, b| {
            let a_idx = DAY_ORDER
                .iter()
                .position(|d| *d == a.day.to_lowercase())
                .unwrap_or(7);
            let b_idx = DAY_ORDER
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

fn day_index(day: &str) -> Option<usize> {
    DAY_ORDER
        .iter()
        .position(|d| *d == day.to_lowercase())
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

    #[test]
    fn test_override_blocked_day() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}],"overrides":{"2025-06-16":{"available":false}}}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-16 is a Monday but blocked by override
        assert!(!avail.is_available(&dt("2025-06-16 10:00")));
    }

    #[test]
    fn test_override_custom_hours() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}],"overrides":{"2025-06-16":{"available":true,"start":"11:00","end":"15:00"}}}"#;
        let avail = Availability::from_json(json).unwrap();
        // 2025-06-16 is a Monday with custom hours 11-15
        assert!(!avail.is_available(&dt("2025-06-16 10:00")));
        assert!(avail.is_available(&dt("2025-06-16 11:00")));
        assert!(avail.is_available(&dt("2025-06-16 14:00")));
        assert!(!avail.is_available(&dt("2025-06-16 15:00")));
    }

    #[test]
    fn test_override_end_time_blocked() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}],"overrides":{"2025-06-16":{"available":false}}}"#;
        let avail = Availability::from_json(json).unwrap();
        assert!(!avail.end_time_within_slot(&dt("2025-06-16 10:00"), 60));
    }

    #[test]
    fn test_override_end_time_custom() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}],"overrides":{"2025-06-16":{"available":true,"start":"11:00","end":"15:00"}}}"#;
        let avail = Availability::from_json(json).unwrap();
        assert!(avail.end_time_within_slot(&dt("2025-06-16 11:00"), 60));
        assert!(!avail.end_time_within_slot(&dt("2025-06-16 14:30"), 60));
    }

    #[test]
    fn test_backward_compat_no_overrides() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        assert!(avail.overrides.is_empty());
        assert!(avail.is_available(&dt("2025-06-16 10:00")));
    }

    #[test]
    fn test_invalid_override_date() {
        let json = r#"{"slots":[],"overrides":{"not-a-date":{"available":false}}}"#;
        assert!(Availability::from_json(json).is_err());
    }

    #[test]
    fn test_override_available_no_custom_hours_uses_default() {
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"}],"overrides":{"2025-06-16":{"available":true}}}"#;
        let avail = Availability::from_json(json).unwrap();
        // Override says available but no custom hours — should use default Mon slots
        assert!(avail.is_available(&dt("2025-06-16 10:00")));
        assert!(!avail.is_available(&dt("2025-06-16 08:00")));
    }

    // ── New-style fields tests ──

    #[test]
    fn test_new_style_effective_slots() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"fri","time_from":"09:00","time_to":"17:00"}"#;
        let avail = Availability::from_json(json).unwrap();
        let slots = avail.effective_slots();
        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].day, "mon");
        assert_eq!(slots[4].day, "fri");
        assert_eq!(slots[0].start, "09:00");
        assert_eq!(slots[0].end, "17:00");
    }

    #[test]
    fn test_new_style_is_available() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"fri","time_from":"09:00","time_to":"17:00"}"#;
        let avail = Availability::from_json(json).unwrap();
        // Monday 10:00 — available
        assert!(avail.is_available(&dt("2025-06-16 10:00")));
        // Saturday — not available
        assert!(!avail.is_available(&dt("2025-06-21 10:00")));
        // Monday 18:00 — outside hours
        assert!(!avail.is_available(&dt("2025-06-16 18:00")));
    }

    #[test]
    fn test_new_style_human_readable() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"wed","time_from":"10:00","time_to":"16:00"}"#;
        let avail = Availability::from_json(json).unwrap();
        assert_eq!(
            avail.to_human_readable(),
            "Mon: 10:00-16:00, Tue: 10:00-16:00, Wed: 10:00-16:00"
        );
    }

    #[test]
    fn test_wrap_around_day_range() {
        let json = r#"{"slots":[],"day_from":"fri","day_to":"mon","time_from":"09:00","time_to":"17:00"}"#;
        let avail = Availability::from_json(json).unwrap();
        let slots = avail.effective_slots();
        assert_eq!(slots.len(), 4); // fri, sat, sun, mon
        assert_eq!(slots[0].day, "fri");
        assert_eq!(slots[1].day, "sat");
        assert_eq!(slots[2].day, "sun");
        assert_eq!(slots[3].day, "mon");
    }

    #[test]
    fn test_breaks_during_availability() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"fri","time_from":"09:00","time_to":"17:00","breaks":[{"start":"12:00","end":"13:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // Monday 12:30 — during break
        assert!(!avail.is_available(&dt("2025-06-16 12:30")));
        // Monday 11:00 — before break
        assert!(avail.is_available(&dt("2025-06-16 11:00")));
        // Monday 13:00 — after break
        assert!(avail.is_available(&dt("2025-06-16 13:00")));
    }

    #[test]
    fn test_break_overlaps_booking() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"fri","time_from":"09:00","time_to":"17:00","breaks":[{"start":"12:00","end":"13:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        // 11:30 + 60min = 12:30 — overlaps break
        assert!(!avail.end_time_within_slot(&dt("2025-06-16 11:30"), 60));
        // 10:00 + 60min = 11:00 — doesn't overlap break
        assert!(avail.end_time_within_slot(&dt("2025-06-16 10:00"), 60));
        // 13:00 + 60min = 14:00 — doesn't overlap break
        assert!(avail.end_time_within_slot(&dt("2025-06-16 13:00"), 60));
    }

    #[test]
    fn test_breaks_with_override_custom_hours() {
        let json = r#"{"slots":[],"day_from":"mon","day_to":"fri","time_from":"09:00","time_to":"17:00","breaks":[{"start":"12:00","end":"13:00"}],"overrides":{"2025-06-16":{"available":true,"start":"10:00","end":"15:00"}}}"#;
        let avail = Availability::from_json(json).unwrap();
        // Override custom hours, but break still applies
        assert!(!avail.is_available(&dt("2025-06-16 12:30")));
        assert!(avail.is_available(&dt("2025-06-16 10:30")));
    }

    #[test]
    fn test_backward_compat_new_fields_missing() {
        // Old-style JSON without any new fields — should still work
        let json = r#"{"slots":[{"day":"mon","start":"09:00","end":"17:00"},{"day":"wed","start":"10:00","end":"14:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        let slots = avail.effective_slots();
        assert_eq!(slots.len(), 2);
        assert!(avail.is_available(&dt("2025-06-16 10:00")));
        assert!(!avail.is_available(&dt("2025-06-17 10:00"))); // Tuesday
        assert!(avail.is_available(&dt("2025-06-18 10:00"))); // Wednesday
    }

    #[test]
    fn test_is_during_break() {
        let json = r#"{"slots":[],"breaks":[{"start":"12:00","end":"13:00"},{"start":"15:00","end":"15:30"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        assert!(avail.is_during_break("12:00"));
        assert!(avail.is_during_break("12:30"));
        assert!(!avail.is_during_break("13:00"));
        assert!(avail.is_during_break("15:15"));
        assert!(!avail.is_during_break("15:30"));
    }

    #[test]
    fn test_overlaps_break() {
        let json = r#"{"slots":[],"breaks":[{"start":"12:00","end":"13:00"}]}"#;
        let avail = Availability::from_json(json).unwrap();
        assert!(avail.overlaps_break("11:30", "12:30"));
        assert!(avail.overlaps_break("12:30", "13:30"));
        assert!(!avail.overlaps_break("10:00", "12:00"));
        assert!(!avail.overlaps_break("13:00", "14:00"));
    }

    #[test]
    fn test_validate_new_fields() {
        // Invalid day_from
        let json = r#"{"slots":[],"day_from":"xyz"}"#;
        assert!(Availability::from_json(json).is_err());

        // Invalid time_from
        let json = r#"{"slots":[],"time_from":"25:00"}"#;
        assert!(Availability::from_json(json).is_err());

        // Invalid break time
        let json = r#"{"slots":[],"breaks":[{"start":"12:00","end":"99:00"}]}"#;
        assert!(Availability::from_json(json).is_err());
    }
}
