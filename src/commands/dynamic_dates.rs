#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

//! Port of Redash's client-side dynamic-date resolution.
//!
//! Redash resolves no `d_*` tokens server-side: its frontend expands them in
//! `getExecutionValue()` before sending, so the server (and both the local
//! ad-hoc and `--remote` stored-query API paths) only ever sees concrete
//! dates. The stored-query endpoint outright rejects raw `d_*` values. We must
//! therefore resolve tokens here, ahead of either path.
//!
//! The token set and semantics mirror upstream Redash (verified against commit
//! `d32d5ae4`):
//! - single `date`/`datetime*`: `client/app/services/parameters/DateParameter.js`
//!   (`DYNAMIC_DATES` — only `now` and `yesterday`)
//! - `*-range`: `client/app/services/parameters/DateRangeParameter.js`
//!   (`DYNAMIC_DATE_RANGES`)
//!
//! Matching details worth preserving on a Redash upgrade: `last_*` ranges end
//! at the current instant (moment's `untilNow`), not end-of-day; the week
//! starts Sunday (moment.js default `startOf("week")`); formats come from
//! upstream `DATETIME_FORMATS`.

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime};
use serde_json::{Value, json};

const PREFIX: &str = "d_";

fn single_date_format(param_type: &str) -> Option<&'static str> {
    match param_type {
        "date" => Some("%Y-%m-%d"),
        "datetime-local" => Some("%Y-%m-%d %H:%M"),
        "datetime-with-seconds" => Some("%Y-%m-%d %H:%M:%S"),
        _ => None,
    }
}

fn range_format(param_type: &str) -> Option<&'static str> {
    match param_type {
        "date-range" => Some("%Y-%m-%d"),
        "datetime-range" => Some("%Y-%m-%d %H:%M"),
        "datetime-range-with-seconds" => Some("%Y-%m-%d %H:%M:%S"),
        _ => None,
    }
}

#[must_use]
pub fn resolve(value: &Value, param_type: &str) -> Option<Value> {
    let Value::String(raw) = value else {
        return None;
    };
    let token = raw.strip_prefix(PREFIX)?;
    resolve_with_now(token, param_type, Local::now().naive_local())
}

fn resolve_with_now(token: &str, param_type: &str, now: NaiveDateTime) -> Option<Value> {
    if let Some(format) = single_date_format(param_type) {
        return resolve_single(token, now)
            .map(|moment| Value::String(format_moment(moment, format)));
    }
    if let Some(format) = range_format(param_type) {
        let (start, end) = resolve_range(token, now)?;
        return Some(json!({
            "start": format_moment(start, format),
            "end": format_moment(end, format),
        }));
    }
    None
}

fn format_moment(moment: NaiveDateTime, format: &str) -> String {
    moment.format(format).to_string()
}

fn resolve_single(token: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    match token {
        "now" => Some(now),
        "yesterday" => Some(now - Duration::days(1)),
        _ => None,
    }
}

fn resolve_range(token: &str, now: NaiveDateTime) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let today = now.date();

    let range = match token {
        "today" => (start_of_day(today), end_of_day(today)),
        "yesterday" => {
            let day = today - Duration::days(1);
            (start_of_day(day), end_of_day(day))
        }
        "this_week" => (
            start_of_day(start_of_week(today)),
            end_of_day(end_of_week(today)),
        ),
        "this_month" => (
            start_of_day(start_of_month(today)),
            end_of_day(end_of_month(today)),
        ),
        "this_year" => (
            start_of_day(start_of_year(today)),
            end_of_day(end_of_year(today)),
        ),
        "last_week" => {
            let base = today - Duration::days(7);
            (
                start_of_day(start_of_week(base)),
                end_of_day(end_of_week(base)),
            )
        }
        "last_month" => {
            let base = subtract_months(today, 1);
            (
                start_of_day(start_of_month(base)),
                end_of_day(end_of_month(base)),
            )
        }
        "last_year" => {
            let base = subtract_months(today, 12);
            (
                start_of_day(start_of_year(base)),
                end_of_day(end_of_year(base)),
            )
        }
        "last_hour" => (now - Duration::hours(1), now),
        "last_8_hours" => (now - Duration::hours(8), now),
        "last_24_hours" => (now - Duration::hours(24), now),
        "last_7_days" => (start_of_day(today - Duration::days(7)), now),
        "last_14_days" => (start_of_day(today - Duration::days(14)), now),
        "last_30_days" => (start_of_day(today - Duration::days(30)), now),
        "last_60_days" => (start_of_day(today - Duration::days(60)), now),
        "last_90_days" => (start_of_day(today - Duration::days(90)), now),
        "last_12_months" => (start_of_day(subtract_months(today, 12)), now),
        "last_2_years" => (start_of_day(subtract_months(today, 24)), now),
        "last_3_years" => (start_of_day(subtract_months(today, 36)), now),
        "last_10_years" => (start_of_day(subtract_months(today, 120)), now),
        _ => return None,
    };

    Some(range)
}

fn start_of_day(date: NaiveDate) -> NaiveDateTime {
    date.and_hms_opt(0, 0, 0).unwrap()
}

fn end_of_day(date: NaiveDate) -> NaiveDateTime {
    date.and_hms_opt(23, 59, 59).unwrap()
}

fn start_of_week(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_sunday()))
}

fn end_of_week(date: NaiveDate) -> NaiveDate {
    start_of_week(date) + Duration::days(6)
}

fn start_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap()
}

fn end_of_month(date: NaiveDate) -> NaiveDate {
    start_of_month(date + Months::new(1)) - Duration::days(1)
}

fn start_of_year(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap()
}

fn end_of_year(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), 12, 31).unwrap()
}

fn subtract_months(date: NaiveDate, months: u32) -> NaiveDate {
    date - Months::new(months)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(date: &str, time: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn day(date: &str) -> NaiveDateTime {
        at(date, "12:30:45")
    }

    #[test]
    fn single_now_formats_by_type() {
        let now = at("2026-06-25", "14:05:09");
        assert_eq!(
            resolve_with_now("now", "date", now),
            Some(json!("2026-06-25"))
        );
        assert_eq!(
            resolve_with_now("now", "datetime-local", now),
            Some(json!("2026-06-25 14:05"))
        );
        assert_eq!(
            resolve_with_now("now", "datetime-with-seconds", now),
            Some(json!("2026-06-25 14:05:09"))
        );
    }

    #[test]
    fn single_yesterday() {
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("yesterday", "date", now),
            Some(json!("2026-06-24"))
        );
    }

    #[test]
    fn single_unknown_token_is_left_alone() {
        let now = day("2026-06-25");
        assert_eq!(resolve_with_now("last_7_days", "date", now), None);
    }

    #[test]
    fn range_today() {
        let now = at("2026-06-25", "14:05:09");
        assert_eq!(
            resolve_with_now("today", "date-range", now),
            Some(json!({"start": "2026-06-25", "end": "2026-06-25"}))
        );
    }

    #[test]
    fn range_last_7_days_ends_at_now() {
        let now = at("2026-06-25", "14:05:09");
        assert_eq!(
            resolve_with_now("last_7_days", "date-range", now),
            Some(json!({"start": "2026-06-18", "end": "2026-06-25"}))
        );
        assert_eq!(
            resolve_with_now("last_7_days", "datetime-range-with-seconds", now),
            Some(json!({"start": "2026-06-18 00:00:00", "end": "2026-06-25 14:05:09"}))
        );
    }

    #[test]
    fn range_this_week_starts_sunday() {
        // 2026-06-25 is a Thursday; the enclosing week runs Sun 21 .. Sat 27.
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("this_week", "date-range", now),
            Some(json!({"start": "2026-06-21", "end": "2026-06-27"}))
        );
    }

    #[test]
    fn range_last_week() {
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("last_week", "date-range", now),
            Some(json!({"start": "2026-06-14", "end": "2026-06-20"}))
        );
    }

    #[test]
    fn range_this_month() {
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("this_month", "date-range", now),
            Some(json!({"start": "2026-06-01", "end": "2026-06-30"}))
        );
    }

    #[test]
    fn range_last_month_clamps_into_prior_month() {
        let now = day("2026-03-31");
        assert_eq!(
            resolve_with_now("last_month", "date-range", now),
            Some(json!({"start": "2026-02-01", "end": "2026-02-28"}))
        );
    }

    #[test]
    fn range_this_year() {
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("this_year", "date-range", now),
            Some(json!({"start": "2026-01-01", "end": "2026-12-31"}))
        );
    }

    #[test]
    fn range_last_year() {
        let now = day("2026-06-25");
        assert_eq!(
            resolve_with_now("last_year", "date-range", now),
            Some(json!({"start": "2025-01-01", "end": "2025-12-31"}))
        );
    }

    #[test]
    fn range_last_12_months() {
        let now = at("2026-06-25", "14:05:09");
        assert_eq!(
            resolve_with_now("last_12_months", "date-range", now),
            Some(json!({"start": "2025-06-25", "end": "2026-06-25"}))
        );
    }

    #[test]
    fn range_token_on_single_type_is_left_alone() {
        let now = day("2026-06-25");
        assert_eq!(resolve_with_now("last_7_days", "date", now), None);
    }

    #[test]
    fn non_date_type_is_left_alone() {
        let now = day("2026-06-25");
        assert_eq!(resolve_with_now("now", "text", now), None);
    }

    #[test]
    fn resolve_ignores_non_string_and_non_prefixed() {
        assert_eq!(resolve(&json!(5), "date"), None);
        assert_eq!(resolve(&json!("2026-06-25"), "date"), None);
    }
}
