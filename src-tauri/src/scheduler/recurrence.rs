#![allow(dead_code)]

use chrono::{Datelike, Local, NaiveDate};

/// Supported recurrence rule values
const RECURRENCE_RULES: &[&str] = &[
    "daily", "weekly", "biweekly", "monthly", "yearly", "weekdays", "weekends",
];

/// Calculate the next occurrence date from a recurrence rule and current due date.
///
/// If `current_due` is `None`, uses today as the base date.
pub fn calculate_next_due(current_due: &Option<String>, rule: &str) -> Option<String> {
    let base_date = if let Some(d) = current_due {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?
    } else {
        Local::now().date_naive()
    };

    let next = match rule.to_lowercase().as_str() {
        "daily" => base_date + chrono::Duration::days(1),
        "weekly" => base_date + chrono::Duration::weeks(1),
        "biweekly" => base_date + chrono::Duration::weeks(2),
        "monthly" => {
            let month = base_date.month();
            let year = base_date.year();
            if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, base_date.day())
                    .unwrap_or_else(|| NaiveDate::from_ymd_opt(year + 1, 1, 28).unwrap())
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, base_date.day())
                    .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, month + 1, 28).unwrap())
            }
        }
        "yearly" => {
            NaiveDate::from_ymd_opt(base_date.year() + 1, base_date.month(), base_date.day())
                .unwrap_or_else(|| {
                    NaiveDate::from_ymd_opt(base_date.year() + 1, base_date.month(), 28).unwrap()
                })
        }
        "weekdays" => {
            let mut d = base_date + chrono::Duration::days(1);
            while d
                .format("%u")
                .to_string()
                .parse::<u8>()
                .map_or(true, |dow| dow > 5)
            {
                d += chrono::Duration::days(1);
            }
            d
        }
        "weekends" => {
            let mut d = base_date + chrono::Duration::days(1);
            while d
                .format("%u")
                .to_string()
                .parse::<u8>()
                .map_or(true, |dow| dow < 6)
            {
                d += chrono::Duration::days(1);
            }
            d
        }
        _ => return None,
    };

    Some(next.format("%Y-%m-%d").to_string())
}

/// Validate whether a recurrence rule string is supported
pub fn validate_rule(rule: &str) -> bool {
    RECURRENCE_RULES.contains(&rule.to_lowercase().as_str())
}

/// Return all supported recurrence rules (for UI dropdown population)
pub fn get_supported_rules() -> Vec<&'static str> {
    RECURRENCE_RULES.to_vec()
}
