use chrono::{DateTime, NaiveDate, Utc};

pub fn parse_date(date_str: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(date_str)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

pub fn is_in_range(date: Option<&str>, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    if let Some(date_str) = date {
        if let Some(date) = parse_date(date_str) {
            return date >= start && date <= end;
        }
    }
    false
}

pub struct WeekRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

pub fn get_week_ranges(start: NaiveDate, end: NaiveDate) -> Vec<WeekRange> {
    let mut weeks = Vec::new();
    let mut current = start;

    while current <= end {
        let week_end = if current.format("%w").to_string() == "0" {
            current
        } else {
            let days_until_sunday =
                7 - current.format("%w").to_string().parse::<u32>().unwrap_or(0);
            current + chrono::Duration::days(days_until_sunday as i64)
        };

        let week_end = if week_end > end { end } else { week_end };

        weeks.push(WeekRange {
            start: current,
            end: week_end,
        });

        current = week_end + chrono::Duration::days(1);
    }

    weeks
}
