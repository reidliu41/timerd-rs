use std::fmt;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DurationError {
    #[error("duration cannot be empty")]
    Empty,
    #[error("invalid duration `{0}` (try 15, 15s, 5m, 1h30m, or 00:15:00)")]
    Invalid(String),
    #[error("duration must be greater than zero")]
    Zero,
    #[error("duration is too large")]
    Overflow,
}

pub fn parse_duration(input: &str) -> Result<u64, DurationError> {
    let input = input.trim().to_ascii_lowercase();
    if input.is_empty() {
        return Err(DurationError::Empty);
    }

    let seconds = if input.contains(':') {
        parse_colon_duration(&input)?
    } else if input.bytes().all(|byte| byte.is_ascii_digit()) {
        input.parse::<u64>().map_err(|_| DurationError::Overflow)?
    } else {
        parse_unit_duration(&input)?
    };

    if seconds == 0 {
        return Err(DurationError::Zero);
    }

    seconds.checked_mul(1_000).ok_or(DurationError::Overflow)
}

fn parse_colon_duration(input: &str) -> Result<u64, DurationError> {
    let parts: Vec<&str> = input.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(DurationError::Invalid(input.to_owned()));
    }

    let values: Vec<u64> = parts
        .iter()
        .map(|part| part.parse::<u64>().map_err(|_| DurationError::Overflow))
        .collect::<Result<_, _>>()?;

    let (hours, minutes, seconds) = match values.as_slice() {
        [minutes, seconds] => (0, *minutes, *seconds),
        [hours, minutes, seconds] => (*hours, *minutes, *seconds),
        _ => unreachable!(),
    };

    if minutes >= 60 || seconds >= 60 {
        return Err(DurationError::Invalid(input.to_owned()));
    }

    hours
        .checked_mul(3_600)
        .and_then(|value| minutes.checked_mul(60).and_then(|m| value.checked_add(m)))
        .and_then(|value| value.checked_add(seconds))
        .ok_or(DurationError::Overflow)
}

fn parse_unit_duration(input: &str) -> Result<u64, DurationError> {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut seconds = 0_u64;
    let mut seen = [false; 3];

    while index < bytes.len() {
        let number_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if number_start == index || index >= bytes.len() {
            return Err(DurationError::Invalid(input.to_owned()));
        }

        let number = input[number_start..index]
            .parse::<u64>()
            .map_err(|_| DurationError::Overflow)?;
        let unit = bytes[index];
        index += 1;

        let (slot, multiplier) = match unit {
            b'h' => (0, 3_600_u64),
            b'm' => (1, 60_u64),
            b's' => (2, 1_u64),
            _ => return Err(DurationError::Invalid(input.to_owned())),
        };
        if seen[slot] {
            return Err(DurationError::Invalid(input.to_owned()));
        }
        seen[slot] = true;

        seconds = seconds
            .checked_add(
                number
                    .checked_mul(multiplier)
                    .ok_or(DurationError::Overflow)?,
            )
            .ok_or(DurationError::Overflow)?;
    }

    Ok(seconds)
}

pub fn format_millis(millis: u64, round_up: bool) -> FormattedDuration {
    let seconds = if round_up {
        millis.saturating_add(999) / 1_000
    } else {
        millis / 1_000
    };
    FormattedDuration(seconds)
}

pub struct FormattedDuration(u64);

impl fmt::Display for FormattedDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hours = self.0 / 3_600;
        let minutes = (self.0 % 3_600) / 60;
        let seconds = self.0 % 60;
        write!(formatter, "{hours:02}:{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_duration_forms() {
        assert_eq!(parse_duration("15").unwrap(), 15_000);
        assert_eq!(parse_duration("15s").unwrap(), 15_000);
        assert_eq!(parse_duration("5m").unwrap(), 300_000);
        assert_eq!(parse_duration("2h").unwrap(), 7_200_000);
        assert_eq!(parse_duration("1h30m").unwrap(), 5_400_000);
        assert_eq!(parse_duration("00:15:00").unwrap(), 900_000);
        assert_eq!(parse_duration("15:00").unwrap(), 900_000);
    }

    #[test]
    fn rejects_invalid_durations() {
        assert!(matches!(parse_duration(""), Err(DurationError::Empty)));
        assert!(matches!(parse_duration("0"), Err(DurationError::Zero)));
        assert!(matches!(
            parse_duration("1h2h"),
            Err(DurationError::Invalid(_))
        ));
        assert!(matches!(
            parse_duration("00:60:00"),
            Err(DurationError::Invalid(_))
        ));
        assert!(matches!(
            parse_duration("tomorrow"),
            Err(DurationError::Invalid(_))
        ));
    }

    #[test]
    fn formats_with_unbounded_hours() {
        assert_eq!(format_millis(5_400_000, false).to_string(), "01:30:00");
        assert_eq!(format_millis(450_603_000, false).to_string(), "125:10:03");
        assert_eq!(format_millis(14_001, true).to_string(), "00:00:15");
    }
}
