use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};
use crossterm::{execute, queue};

use crate::alarm::{
    format_alarm_clock, format_alarm_target, format_created_at, parse_alarm_target,
};
use crate::client;
use crate::duration::{format_millis, parse_duration};
use crate::model::{TimerKind, TimerView, now_millis};
use crate::paths::AppPaths;
use crate::protocol::{Request, Response};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const MINIMUM_TABLE_WIDTH: usize = 68;

pub fn run(paths: &AppPaths) -> Result<Option<u64>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("watch requires an interactive terminal");
    }

    let mut terminal = WatchTerminal::new()?;
    let mut tasks = active_tasks(paths)?;
    let mut selected = 0_usize;
    let mut message = String::new();
    let mut last_refresh = Instant::now();
    let mut dirty = true;

    loop {
        selected = selected.min(tasks.len().saturating_sub(1));
        if dirty {
            draw(&tasks, selected, &message)?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                        message.clear();
                        dirty = true;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected + 1 < tasks.len() {
                            selected += 1;
                        }
                        message.clear();
                        dirty = true;
                    }
                    KeyCode::Enter => {
                        if let Some(task) = tasks.get(selected) {
                            let id = task.id;
                            drop(terminal);
                            return Ok(Some(id));
                        }
                    }
                    KeyCode::Char('x') => {
                        if let Some(task) = tasks.get(selected) {
                            message = change_message(client::request(
                                paths,
                                &Request::Stop { id: Some(task.id) },
                            ));
                            tasks = refresh_preserving_selection(paths, &tasks, &mut selected)?;
                            dirty = true;
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(task) = tasks.get(selected).cloned() {
                            message = snooze(paths, &task, &mut terminal);
                            tasks = refresh_preserving_selection(paths, &tasks, &mut selected)?;
                            dirty = true;
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(task) = tasks.get(selected).cloned() {
                            message = edit(paths, &task, &mut terminal);
                            tasks = refresh_preserving_selection(paths, &tasks, &mut selected)?;
                            dirty = true;
                        }
                    }
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }

        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            tasks = refresh_preserving_selection(paths, &tasks, &mut selected)?;
            last_refresh = Instant::now();
            dirty = true;
        }
    }
}

fn active_tasks(paths: &AppPaths) -> Result<Vec<TimerView>> {
    let response = client::expect_response(client::request(paths, &Request::List)?)?;
    let Response::Tasks { mut tasks } = response else {
        bail!("unexpected response from timer daemon");
    };
    tasks.retain(|task| task.status.is_active());
    tasks.sort_by_key(|task| task.id);
    Ok(tasks)
}

fn refresh_preserving_selection(
    paths: &AppPaths,
    previous: &[TimerView],
    selected: &mut usize,
) -> Result<Vec<TimerView>> {
    let selected_id = previous.get(*selected).map(|task| task.id);
    let tasks = active_tasks(paths)?;
    if let Some(id) = selected_id {
        if let Some(index) = tasks.iter().position(|task| task.id == id) {
            *selected = index;
        } else {
            *selected = (*selected).min(tasks.len().saturating_sub(1));
        }
    } else {
        *selected = 0;
    }
    Ok(tasks)
}

fn snooze(paths: &AppPaths, task: &TimerView, terminal: &mut WatchTerminal) -> String {
    if task.kind == TimerKind::Stopwatch {
        return "A stopwatch cannot be snoozed".to_owned();
    }

    if let Err(error) = terminal.prompt_mode() {
        return format!("Failed to open snooze prompt: {error:#}");
    }
    let result = prompt("Snooze by [s/m/h] (e.g. 5m, empty cancels): ").and_then(|value| {
        if value.is_empty() {
            return Ok(None);
        }
        let delay_millis = parse_duration(&value)?;
        let response = client::expect_response(client::request(
            paths,
            &Request::Snooze {
                id: task.id,
                delay_millis,
            },
        )?)?;
        match response {
            Response::Changed { task } => Ok(Some(format!(
                "Timer #{} snoozed by {}",
                task.id,
                format_millis(delay_millis, false)
            ))),
            _ => bail!("unexpected response from timer daemon"),
        }
    });
    let resume = terminal.watch_mode();
    match (result, resume) {
        (_, Err(error)) => format!("Failed to restore watch: {error:#}"),
        (Ok(Some(message)), Ok(())) => message,
        (Ok(None), Ok(())) => "Snooze cancelled".to_owned(),
        (Err(error), Ok(())) => format!("Snooze failed: {error:#}"),
    }
}

fn edit(paths: &AppPaths, task: &TimerView, terminal: &mut WatchTerminal) -> String {
    if let Err(error) = terminal.prompt_mode() {
        return format!("Failed to open edit prompt: {error:#}");
    }
    let result = edit_inner(paths, task);
    let resume = terminal.watch_mode();
    match (result, resume) {
        (_, Err(error)) => format!("Failed to restore watch: {error:#}"),
        (Ok(message), Ok(())) => message,
        (Err(error), Ok(())) => format!("Edit failed: {error:#}"),
    }
}

fn edit_inner(paths: &AppPaths, task: &TimerView) -> Result<String> {
    let current_note = task.note.as_deref().unwrap_or("-");
    let note_input = prompt(&format!(
        "Note [{current_note}] (Enter keeps it, - clears it): "
    ))?;
    let note = match note_input.as_str() {
        "" => task.note.clone(),
        "-" => None,
        _ => Some(note_input),
    };

    let schedule_millis = match task.kind {
        TimerKind::Stopwatch => None,
        TimerKind::Countdown => {
            let input = prompt(&format!(
                "Remaining duration [{}] (Enter keeps it): ",
                format_millis(task.value_millis, true)
            ))?;
            if input.is_empty() {
                None
            } else {
                Some(parse_duration(&input)?)
            }
        }
        TimerKind::Alarm => {
            let current = task
                .target_at_millis
                .map(format_alarm_target)
                .unwrap_or_else(|| "unknown".to_owned());
            let input = prompt(&format!("Target [{current}] (Enter keeps it): "))?;
            if input.is_empty() {
                None
            } else {
                Some(parse_alarm_target(&input)?)
            }
        }
    };

    match client::expect_response(client::request(
        paths,
        &Request::Edit {
            id: task.id,
            note,
            schedule_millis,
        },
    )?)? {
        Response::Changed { task } => Ok(format!("Timer #{} updated", task.id)),
        _ => bail!("unexpected response from timer daemon"),
    }
}

fn prompt(label: &str) -> Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{label}")?;
    stdout.flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_owned())
}

fn change_message(result: Result<Response>) -> String {
    match result.and_then(client::expect_response) {
        Ok(Response::Changed { task }) => format!("Timer #{} cancelled", task.id),
        Ok(_) => "Unexpected response from timer daemon".to_owned(),
        Err(error) => format!("Cancel failed: {error:#}"),
    }
}

fn draw(tasks: &[TimerView], selected: usize, message: &str) -> Result<()> {
    let (terminal_width, terminal_height) = terminal::size().unwrap_or((80, 24));
    let lines = render(tasks, selected, terminal_width, terminal_height, message);
    let mut stdout = io::stdout();
    queue!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
    for (index, line) in lines.iter().enumerate() {
        write!(stdout, "{line}")?;
        if index + 1 < lines.len() {
            write!(stdout, "\r\n")?;
        }
    }
    stdout.flush()?;
    Ok(())
}

fn render(
    tasks: &[TimerView],
    selected: usize,
    terminal_width: u16,
    terminal_height: u16,
    message: &str,
) -> Vec<String> {
    let width = usize::from(terminal_width.max(1));
    let height = usize::from(terminal_height.max(1));
    let mut lines = vec!["Active timers".to_owned()];

    if width < MINIMUM_TABLE_WIDTH {
        lines.push(format!(
            "Terminal too narrow ({width}); resize to at least {MINIMUM_TABLE_WIDTH} columns."
        ));
    } else if tasks.is_empty() {
        lines.push(String::new());
        lines.push("No active timers. Waiting...".to_owned());
    } else {
        let note_width = width.saturating_sub(MINIMUM_TABLE_WIDTH).saturating_add(1);
        lines.push(format_row(
            " ", "ID", "TYPE", "STATUS", "TIME", "TARGET", "NOTE", "CREATED", note_width,
        ));
        let reserved_rows = if message.is_empty() { 4 } else { 5 };
        let available_rows = height.saturating_sub(reserved_rows).max(1);
        let start = selected.saturating_sub(available_rows.saturating_sub(1));
        for (index, task) in tasks.iter().enumerate().skip(start).take(available_rows) {
            let target = match task.kind {
                TimerKind::Alarm => task
                    .target_at_millis
                    .map(format_alarm_clock)
                    .unwrap_or_else(|| "-".to_owned()),
                TimerKind::Countdown => {
                    format_alarm_clock(now_millis().saturating_add(task.value_millis))
                }
                TimerKind::Stopwatch => "-".to_owned(),
            };
            lines.push(format_row(
                if index == selected { ">" } else { " " },
                &task.id.to_string(),
                task.kind.as_str(),
                task.status.as_str(),
                &format_millis(task.value_millis, task.kind.counts_down()).to_string(),
                &target,
                task.note.as_deref().unwrap_or("-"),
                &format_created_at(task.created_at_millis),
                note_width,
            ));
        }
    }

    lines.push(String::new());
    lines.push("Up/Down select | Enter attach | x cancel | s snooze | e edit | q exit".to_owned());
    if !message.is_empty() {
        lines.push(message.to_owned());
    }
    lines.truncate(height);
    lines
}

#[allow(clippy::too_many_arguments)]
fn format_row(
    marker: &str,
    id: &str,
    kind: &str,
    status: &str,
    time: &str,
    target: &str,
    note: &str,
    created: &str,
    note_width: usize,
) -> String {
    format!(
        "{marker} {} {} {} {} {} {} {}",
        fit_cell(id, 4),
        fit_cell(kind, 10),
        fit_cell(status, 8),
        fit_cell(time, 10),
        fit_cell(target, 8),
        fit_cell(note, note_width),
        fit_cell(created, 19),
    )
}

fn fit_cell(value: &str, width: usize) -> String {
    let mut output = String::new();
    let mut used = 0_usize;
    let mut truncated = false;
    for character in value.chars() {
        let character_width = display_width(character);
        if used.saturating_add(character_width) > width {
            truncated = true;
            break;
        }
        output.push(character);
        used += character_width;
    }
    if truncated && width > 0 {
        while used.saturating_add(1) > width {
            if let Some(character) = output.pop() {
                used = used.saturating_sub(display_width(character));
            } else {
                break;
            }
        }
        output.push('…');
        used += 1;
    }
    output.push_str(&" ".repeat(width.saturating_sub(used)));
    output
}

fn display_width(character: char) -> usize {
    if character.is_ascii() { 1 } else { 2 }
}

struct WatchTerminal {
    raw: bool,
    alternate: bool,
}

impl WatchTerminal {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All)
        ) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        Ok(Self {
            raw: true,
            alternate: true,
        })
    }

    fn prompt_mode(&mut self) -> Result<()> {
        if self.raw {
            disable_raw_mode()?;
            self.raw = false;
        }
        if self.alternate {
            execute!(io::stdout(), Show, LeaveAlternateScreen)?;
            self.alternate = false;
        }
        Ok(())
    }

    fn watch_mode(&mut self) -> Result<()> {
        if !self.raw {
            enable_raw_mode()?;
            self.raw = true;
        }
        if !self.alternate {
            if let Err(error) = execute!(
                io::stdout(),
                EnterAlternateScreen,
                Hide,
                Clear(ClearType::All)
            ) {
                let _ = disable_raw_mode();
                self.raw = false;
                return Err(error.into());
            }
            self.alternate = true;
        }
        Ok(())
    }
}

impl Drop for WatchTerminal {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        } else {
            let _ = execute!(io::stdout(), Show);
        }
        if self.raw {
            let _ = disable_raw_mode();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::TimerStatus;

    use super::*;

    fn task(id: u64, kind: TimerKind, note: Option<&str>) -> TimerView {
        TimerView {
            id,
            kind,
            status: TimerStatus::Running,
            note: note.map(str::to_owned),
            value_millis: 252_000,
            target_at_millis: (kind == TimerKind::Alarm).then_some(1_798_000_000_000),
            bell: false,
            created_at_millis: 1_798_000_000_000,
        }
    }

    #[test]
    fn table_has_all_columns_and_creation_time_is_last() {
        let lines = render(
            &[
                task(12, TimerKind::Alarm, Some("breakfast")),
                task(13, TimerKind::Countdown, Some("coffee")),
                task(14, TimerKind::Stopwatch, Some("coding")),
            ],
            1,
            120,
            24,
            "",
        );
        assert!(lines[1].contains("ID"));
        assert!(lines[1].contains("TARGET"));
        assert!(lines[1].ends_with("CREATED            "));
        assert!(lines[3].starts_with("> 13"));
        assert!(lines[3].contains("countdown"));
        assert!(lines[3].contains("coffee"));
        assert!(lines[3].ends_with(&format_created_at(1_798_000_000_000)));
    }

    #[test]
    fn empty_and_narrow_views_explain_their_state() {
        let empty = render(&[], 0, 80, 24, "");
        assert!(empty.iter().any(|line| line.contains("No active timers")));

        let narrow = render(&[task(1, TimerKind::Stopwatch, None)], 0, 40, 24, "");
        assert!(narrow.iter().any(|line| line.contains("too narrow")));
    }

    #[test]
    fn long_notes_are_truncated() {
        let row = format_row(
            ">",
            "1",
            "countdown",
            "running",
            "00:01:00",
            "08:00",
            "a very long note",
            "2026-10-29 08:00:00",
            8,
        );
        assert!(row.contains("a very …"));
        assert!(row.ends_with("2026-10-29 08:00:00"));
    }

    #[test]
    fn chinese_notes_use_terminal_cell_width() {
        let row = format_row(
            ">",
            "12",
            "alarm",
            "running",
            "00:24:37",
            "08:00",
            "吃早饭提醒",
            "2026-10-29 08:00:00",
            8,
        );
        assert!(row.contains("吃早饭…"));
        assert!(row.ends_with("2026-10-29 08:00:00"));
    }
}
