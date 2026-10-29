use std::fmt;

use clap::ValueEnum;

use crate::alarm::format_alarm_clock;
use crate::duration::format_millis;
use crate::model::{TimerKind, TimerStatus, TimerView};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum DisplaySize {
    #[default]
    #[value(alias = "s", alias = "1")]
    Small,
    #[value(alias = "m", alias = "2")]
    Medium,
    #[value(alias = "l", alias = "3")]
    Large,
    #[value(alias = "xl", alias = "4")]
    Xlarge,
}

impl DisplaySize {
    pub fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            "xlarge" => Some(Self::Xlarge),
            _ => None,
        }
    }

    pub fn larger(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large | Self::Xlarge => Self::Xlarge,
        }
    }

    pub fn smaller(self) -> Self {
        match self {
            Self::Small | Self::Medium => Self::Small,
            Self::Large => Self::Medium,
            Self::Xlarge => Self::Large,
        }
    }
}

impl fmt::Display for DisplaySize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Xlarge => "xlarge",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderedFrame {
    pub lines: Vec<String>,
    pub effective_size: DisplaySize,
}

pub fn render_timer(
    view: &TimerView,
    requested_size: DisplaySize,
    terminal_width: u16,
    terminal_height: u16,
) -> RenderedFrame {
    let width = usize::from(terminal_width.max(1));
    let height = usize::from(terminal_height.max(1));
    let mut effective_size = requested_size;

    while effective_size != DisplaySize::Small {
        let digits = render_digits(view, effective_size);
        let digit_width = digits
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        if digit_width <= width && digits.len().saturating_add(2) <= height {
            break;
        }
        effective_size = effective_size.smaller();
    }

    if effective_size == DisplaySize::Small {
        return RenderedFrame {
            lines: vec![truncate(&compact_line(view), width)],
            effective_size,
        };
    }

    let mut lines = render_digits(view, effective_size);
    lines.push(metadata_line(view));
    let size_description = if effective_size == requested_size {
        effective_size.to_string()
    } else {
        format!("{effective_size}; requested {requested_size}")
    };
    if view.kind == TimerKind::Alarm {
        lines.push(format!(
            "d detach | x cancel | +/- size ({size_description})"
        ));
    } else {
        lines.push(format!(
            "Space pause | d detach | x stop | +/- size ({size_description})"
        ));
    }

    RenderedFrame {
        lines: lines
            .into_iter()
            .map(|line| center(&truncate(&line, width), width))
            .collect(),
        effective_size,
    }
}

pub fn compact_line(view: &TimerView) -> String {
    let symbol = match view.status {
        TimerStatus::Running if view.kind == TimerKind::Stopwatch => "⏱",
        TimerStatus::Running if view.kind == TimerKind::Alarm => "⏰",
        TimerStatus::Running => "⏳",
        TimerStatus::Paused => "⏸",
        TimerStatus::Done => "✓",
        TimerStatus::Stopped | TimerStatus::Cancelled => "×",
    };
    let round_up = view.kind.counts_down() && view.status.is_active();
    let note = view
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    let paused = if view.status == TimerStatus::Paused {
        " · paused"
    } else {
        ""
    };
    if view.kind == TimerKind::Alarm {
        let target = view
            .target_at_millis
            .map(format_alarm_clock)
            .unwrap_or_else(|| "unknown time".to_owned());
        format!(
            "{symbol} {}  [#{}]  at {target}{note}",
            format_millis(view.value_millis, round_up),
            view.id
        )
    } else {
        format!(
            "{symbol} {}{note}{paused}  [#{}]",
            format_millis(view.value_millis, round_up),
            view.id
        )
    }
}

fn render_digits(view: &TimerView, size: DisplaySize) -> Vec<String> {
    let round_up = view.kind.counts_down() && view.status.is_active();
    let time = format_millis(view.value_millis, round_up).to_string();
    let (font, scale) = match size {
        DisplaySize::Medium => (Font::Medium, 1),
        DisplaySize::Large => (Font::Large, 1),
        DisplaySize::Xlarge => (Font::Large, 2),
        DisplaySize::Small => return vec![compact_line(view)],
    };
    render_text(&time, font, scale)
}

fn metadata_line(view: &TimerView) -> String {
    let note = view
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    if view.kind == TimerKind::Alarm {
        let target = view
            .target_at_millis
            .map(format_alarm_clock)
            .unwrap_or_else(|| "unknown time".to_owned());
        format!(
            "alarm #{} · at {target} · {}{note}",
            view.id,
            view.status.as_str()
        )
    } else {
        format!(
            "{} #{} · {}{note}",
            view.kind.as_str(),
            view.id,
            view.status.as_str()
        )
    }
}

#[derive(Clone, Copy)]
enum Font {
    Medium,
    Large,
}

fn render_text(text: &str, font: Font, scale: usize) -> Vec<String> {
    let glyphs: Vec<Vec<&str>> = text
        .chars()
        .map(|character| pattern(font, character).split('/').collect())
        .collect();
    let height = glyphs.first().map(Vec::len).unwrap_or_default();
    let separator = " ".repeat(scale);
    let mut lines = Vec::with_capacity(height.saturating_mul(scale));

    for row in 0..height {
        let mut rendered = String::new();
        for (index, glyph) in glyphs.iter().enumerate() {
            if index > 0 {
                rendered.push_str(&separator);
            }
            for pixel in glyph[row].chars() {
                let cell = if pixel == '#' { '█' } else { ' ' };
                for _ in 0..scale {
                    rendered.push(cell);
                }
            }
        }
        for _ in 0..scale {
            lines.push(rendered.clone());
        }
    }
    lines
}

fn pattern(font: Font, character: char) -> &'static str {
    match (font, character) {
        (Font::Medium, '0') => "###/# #/# #/# #/###",
        (Font::Medium, '1') => " # /## / # / # /###",
        (Font::Medium, '2') => "###/  #/###/#  /###",
        (Font::Medium, '3') => "###/  #/###/  #/###",
        (Font::Medium, '4') => "# #/# #/###/  #/  #",
        (Font::Medium, '5') => "###/#  /###/  #/###",
        (Font::Medium, '6') => "###/#  /###/# #/###",
        (Font::Medium, '7') => "###/  #/  #/ # / # ",
        (Font::Medium, '8') => "###/# #/###/# #/###",
        (Font::Medium, '9') => "###/# #/###/  #/###",
        (Font::Medium, ':') => " /#/ /#/ ",
        (Font::Large, '0') => " ### /#   #/#   #/#   #/#   #/#   #/ ### ",
        (Font::Large, '1') => "  #  / ##  /# #  /  #  /  #  /  #  /#####",
        (Font::Large, '2') => " ### /#   #/    #/   # /  #  / #   /#####",
        (Font::Large, '3') => "#### /    #/    #/ ### /    #/    #/#### ",
        (Font::Large, '4') => "#   #/#   #/#   #/#####/    #/    #/    #",
        (Font::Large, '5') => "#####/#    /#    /#### /    #/    #/#### ",
        (Font::Large, '6') => " ### /#    /#    /#### /#   #/#   #/ ### ",
        (Font::Large, '7') => "#####/    #/   # /  #  / #   / #   / #   ",
        (Font::Large, '8') => " ### /#   #/#   #/ ### /#   #/#   #/ ### ",
        (Font::Large, '9') => " ### /#   #/#   #/ ####/    #/    #/ ### ",
        (Font::Large, ':') => " /#/ / /#/ / ",
        _ => unreachable!("timer display only renders digits and colons"),
    }
}

fn truncate(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut truncated: String = value.chars().take(width - 1).collect();
    truncated.push('…');
    truncated
}

fn center(value: &str, width: usize) -> String {
    let length = value.chars().count();
    format!("{}{}", " ".repeat(width.saturating_sub(length) / 2), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> TimerView {
        TimerView {
            id: 11,
            kind: TimerKind::Stopwatch,
            status: TimerStatus::Running,
            note: None,
            value_millis: 12_000,
            target_at_millis: None,
        }
    }

    #[test]
    fn size_aliases_and_steps_are_stable() {
        assert_eq!(DisplaySize::Small.larger(), DisplaySize::Medium);
        assert_eq!(DisplaySize::Medium.larger(), DisplaySize::Large);
        assert_eq!(DisplaySize::Large.larger(), DisplaySize::Xlarge);
        assert_eq!(DisplaySize::Xlarge.larger(), DisplaySize::Xlarge);
        assert_eq!(DisplaySize::Xlarge.smaller(), DisplaySize::Large);
    }

    #[test]
    fn renders_four_fixed_sizes() {
        let small = render_timer(&view(), DisplaySize::Small, 120, 40);
        let medium = render_timer(&view(), DisplaySize::Medium, 120, 40);
        let large = render_timer(&view(), DisplaySize::Large, 120, 40);
        let xlarge = render_timer(&view(), DisplaySize::Xlarge, 120, 40);

        assert_eq!(small.lines.len(), 1);
        assert_eq!(medium.lines.len(), 7);
        assert_eq!(large.lines.len(), 9);
        assert_eq!(xlarge.lines.len(), 16);
    }

    #[test]
    fn all_font_glyphs_have_consistent_dimensions() {
        for (font, expected_width, expected_height) in [(Font::Medium, 3, 5), (Font::Large, 5, 7)] {
            for character in "0123456789".chars() {
                let rows: Vec<_> = pattern(font, character).split('/').collect();
                assert_eq!(rows.len(), expected_height, "glyph {character}");
                assert!(
                    rows.iter().all(|row| row.chars().count() == expected_width),
                    "glyph {character}"
                );
            }
            let colon: Vec<_> = pattern(font, ':').split('/').collect();
            assert_eq!(colon.len(), expected_height);
            assert!(colon.iter().all(|row| row.chars().count() == 1));
        }
    }

    #[test]
    fn falls_back_when_the_terminal_is_too_small() {
        let frame = render_timer(&view(), DisplaySize::Xlarge, 45, 12);
        assert_eq!(frame.effective_size, DisplaySize::Large);
        assert!(frame.lines.iter().all(|line| line.chars().count() <= 45));

        let tiny = render_timer(&view(), DisplaySize::Large, 20, 4);
        assert_eq!(tiny.effective_size, DisplaySize::Small);
        assert_eq!(tiny.lines.len(), 1);
    }

    #[test]
    fn alarm_has_target_metadata_and_no_pause_control() {
        let alarm = TimerView {
            id: 12,
            kind: TimerKind::Alarm,
            status: TimerStatus::Running,
            note: Some("breakfast".to_owned()),
            value_millis: 24 * 60 * 1_000,
            target_at_millis: Some(1_798_000_000_000),
        };
        let small = render_timer(&alarm, DisplaySize::Small, 120, 40);
        assert!(small.lines[0].contains("⏰"));
        assert!(small.lines[0].contains("at "));
        assert!(small.lines[0].contains("breakfast"));

        let medium = render_timer(&alarm, DisplaySize::Medium, 120, 40);
        assert!(medium.lines.iter().any(|line| line.contains("alarm #12")));
        assert!(medium.lines.iter().any(|line| line.contains("x cancel")));
        assert!(!medium.lines.iter().any(|line| line.contains("pause")));
    }
}
