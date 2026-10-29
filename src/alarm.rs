use std::fmt::Display;

use anyhow::{Result, bail};
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Timelike};

const EXPECTED_FORMATS: &str = "HH:MM, HH:MM:SS, YYYY-MM-DD HH:MM, or YYYY-MM-DD HH:MM:SS";

pub fn parse_alarm_target(value: &str) -> Result<u64> {
    let target = parse_in_timezone(value, Local::now())?;
    u64::try_from(target.timestamp_millis())
        .map_err(|_| anyhow::anyhow!("alarm time is outside the supported range"))
}

pub fn format_alarm_target(target_millis: u64) -> String {
    format_target(target_millis, true)
}

pub fn format_alarm_clock(target_millis: u64) -> String {
    format_target(target_millis, false)
}

pub fn format_created_at(timestamp_millis: u64) -> String {
    if timestamp_millis == 0 {
        return "-".to_owned();
    }
    let Ok(timestamp) = i64::try_from(timestamp_millis) else {
        return "unknown".to_owned();
    };
    Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|timestamp| timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn format_target(target_millis: u64, include_date: bool) -> String {
    let Ok(timestamp) = i64::try_from(target_millis) else {
        return "unknown time".to_owned();
    };
    let Some(target) = Local.timestamp_millis_opt(timestamp).single() else {
        return "unknown time".to_owned();
    };
    match (include_date, target.second()) {
        (true, 0) => target.format("%Y-%m-%d %H:%M").to_string(),
        (true, _) => target.format("%Y-%m-%d %H:%M:%S").to_string(),
        (false, 0) => target.format("%H:%M").to_string(),
        (false, _) => target.format("%H:%M:%S").to_string(),
    }
}

fn parse_in_timezone<Tz>(value: &str, now: DateTime<Tz>) -> Result<DateTime<Tz>>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let value = value.trim();
    if value.is_empty() {
        bail!("alarm time is empty; expected {EXPECTED_FORMATS}");
    }

    if let Some(time) = parse_time(value) {
        let mut date = now.date_naive();
        let mut local = date.and_time(time);
        if local <= now.naive_local() {
            date = date
                .succ_opt()
                .ok_or_else(|| anyhow::anyhow!("alarm date is outside the supported range"))?;
            local = date.and_time(time);
        }
        return resolve_local(now.timezone(), local, now.timestamp_millis());
    }

    if let Some(local) = parse_date_time(value) {
        let target = resolve_local(now.timezone(), local, now.timestamp_millis())?;
        if target.timestamp_millis() <= now.timestamp_millis() {
            bail!("alarm time `{value}` has already passed");
        }
        return Ok(target);
    }

    bail!("invalid alarm time `{value}`; expected {EXPECTED_FORMATS}")
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    ["%H:%M:%S", "%H:%M"]
        .into_iter()
        .find_map(|format| NaiveTime::parse_from_str(value, format).ok())
}

fn parse_date_time(value: &str) -> Option<NaiveDateTime> {
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
}

fn resolve_local<Tz>(timezone: Tz, local: NaiveDateTime, now_millis: i64) -> Result<DateTime<Tz>>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    match timezone.from_local_datetime(&local) {
        LocalResult::Single(target) => Ok(target),
        LocalResult::Ambiguous(first, second) => [first, second]
            .into_iter()
            .filter(|target| target.timestamp_millis() > now_millis)
            .min_by_key(DateTime::timestamp_millis)
            .ok_or_else(|| anyhow::anyhow!("alarm time `{local}` has already passed")),
        LocalResult::None => bail!(
            "alarm time `{local}` does not exist in the current timezone (daylight-saving transition)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use super::*;

    fn local_time(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .unwrap()
    }

    #[test]
    fn clock_time_uses_today_when_still_in_the_future() {
        let now = local_time(2026, 10, 28, 7, 30, 0);
        let target = parse_in_timezone("8:00", now).unwrap();
        assert_eq!(target, local_time(2026, 10, 28, 8, 0, 0));
    }

    #[test]
    fn clock_time_rolls_to_tomorrow_after_it_passes() {
        let now = local_time(2026, 10, 28, 8, 0, 1);
        let target = parse_in_timezone("08:00", now).unwrap();
        assert_eq!(target, local_time(2026, 10, 29, 8, 0, 0));
    }

    #[test]
    fn accepts_seconds_and_an_explicit_future_date() {
        let now = local_time(2026, 10, 28, 7, 30, 0);
        assert_eq!(
            parse_in_timezone("08:00:15", now).unwrap(),
            local_time(2026, 10, 28, 8, 0, 15)
        );
        assert_eq!(
            parse_in_timezone("2026-10-30 18:45", now).unwrap(),
            local_time(2026, 10, 30, 18, 45, 0)
        );
    }

    #[test]
    fn rejects_invalid_and_past_explicit_times() {
        let now = local_time(2026, 10, 28, 7, 30, 0);
        assert!(parse_in_timezone("breakfast", now).is_err());
        assert!(parse_in_timezone("2026-10-27 08:00", now).is_err());
    }
}
