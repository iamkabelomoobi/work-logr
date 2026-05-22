use super::model::TimesheetEntry;
use crate::utils::dates::WeekRange;

pub fn split_entries_by_week(
    entries: Vec<TimesheetEntry>,
    weeks: &[WeekRange],
) -> Vec<Vec<TimesheetEntry>> {
    weeks
        .iter()
        .map(|week| {
            entries
                .iter()
                .filter(|entry| {
                    if let Ok(entry_date) = chrono::DateTime::parse_from_rfc3339(&entry.date) {
                        let entry_naive = entry_date.naive_utc().date();
                        entry_naive >= week.start && entry_naive <= week.end
                    } else {
                        false
                    }
                })
                .cloned()
                .collect()
        })
        .collect()
}
