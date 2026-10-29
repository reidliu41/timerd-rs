use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType, disable_raw_mode, enable_raw_mode};
use crossterm::{execute, queue};

use crate::alarm::{format_alarm_clock, format_alarm_target};
use crate::client;
use crate::display::{DisplaySize, compact_line, render_timer};
use crate::duration::format_millis;
use crate::model::{TimerKind, TimerStatus, TimerView};
use crate::paths::AppPaths;
use crate::protocol::{Request, Response};
use crate::theme::ThemeSession;

pub fn choose_timer_kind() -> Result<TimerKind> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive selection requires a terminal; use `timer stopwatch` or `timer 15m`");
    }

    println!("Select timer type:");
    let mut selected = 0_usize;
    let mut first_render = true;
    let guard = RawTerminal::new(false)?;

    loop {
        render_selector(selected, !first_render)?;
        first_render = false;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1).min(1),
                KeyCode::Char('1') => {
                    selected = 0;
                    break;
                }
                KeyCode::Char('2') => {
                    selected = 1;
                    break;
                }
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    drop(guard);
                    println!();
                    bail!("cancelled");
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    drop(guard);
                    println!();
                    bail!("cancelled");
                }
                _ => {}
            }
        }
    }

    if !first_render {
        render_selector(selected, true)?;
    }
    drop(guard);
    println!();
    Ok(if selected == 0 {
        TimerKind::Stopwatch
    } else {
        TimerKind::Countdown
    })
}

fn render_selector(selected: usize, move_up: bool) -> Result<()> {
    let mut stdout = io::stdout();
    if move_up {
        queue!(stdout, MoveUp(1))?;
    }
    queue!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
    write!(
        stdout,
        "{} 1. Stopwatch    Count up from zero\r\n",
        if selected == 0 { ">" } else { " " }
    )?;
    queue!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
    write!(
        stdout,
        "{} 2. Countdown    Count down to zero",
        if selected == 1 { ">" } else { " " }
    )?;
    stdout.flush()?;
    Ok(())
}

pub fn prompt(label: &str) -> Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{label}")?;
    stdout.flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_owned())
}

pub fn run_foreground(
    paths: &AppPaths,
    id: Option<u64>,
    mut display_size: DisplaySize,
    mut theme: Option<ThemeSession>,
) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("foreground display requires a terminal; start with --detach instead");
    }

    let stream = client::attach(paths, id)?;
    let control = stream.try_clone()?;
    let receiver = response_reader(stream);
    let first = receiver
        .recv_timeout(Duration::from_secs(2))
        .context("timer daemon did not respond to attach request")??;
    let mut view = snapshot_from_response(first)?;

    let guard = RawTerminal::new(false)?;
    let mut drawer = FrameDrawer::default();
    let mut last_render = Vec::new();
    let mut prefix = false;
    let mut final_started = None;

    loop {
        let (terminal_width, terminal_height) = terminal::size().unwrap_or((80, 24));
        let lines = render_with_theme(
            &view,
            display_size,
            terminal_width,
            terminal_height,
            theme.as_mut(),
        );
        if lines != last_render {
            drawer.draw(&lines)?;
            last_render = lines;
        }

        if view.status.is_final() {
            let started = final_started.get_or_insert_with(Instant::now);
            let hold = theme
                .as_ref()
                .map(|theme| theme.completion_hold(view.status))
                .unwrap_or_default();
            if started.elapsed() < hold {
                if event::poll(Duration::from_millis(50))? {
                    let _ = event::read()?;
                }
                continue;
            }
            let _ = control.shutdown(std::net::Shutdown::Both);
            drawer.clear()?;
            drop(guard);
            println!("{}", final_line(&view));
            return Ok(());
        }

        while let Ok(message) = receiver.try_recv() {
            view = snapshot_from_response(message?)?;
        }

        // The daemon closes an attach stream after sending the final snapshot.
        // Render that snapshot (including any theme completion frames) before
        // treating a closed stream as an unexpected disconnect below.
        if view.status.is_final() {
            continue;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if is_control_b(key) {
                    prefix = true;
                    continue;
                }

                let detach = key.code == KeyCode::Char('d')
                    || (prefix && matches!(key.code, KeyCode::Char('D')));
                prefix = false;

                if detach {
                    let _ = control.shutdown(std::net::Shutdown::Both);
                    drawer.clear()?;
                    drop(guard);
                    println!("{}", detached_line(&view));
                    return Ok(());
                }

                match key.code {
                    KeyCode::Char(' ') => {
                        if view.kind != TimerKind::Alarm {
                            send_change(paths, Request::Toggle { id: view.id })?;
                        }
                    }
                    KeyCode::Char('x') => {
                        send_change(paths, Request::Stop { id: Some(view.id) })?;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        send_change(paths, Request::Stop { id: Some(view.id) })?;
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        display_size = display_size.larger();
                    }
                    KeyCode::Char('-') | KeyCode::Char('_') => {
                        display_size = display_size.smaller();
                    }
                    KeyCode::Char('1') => display_size = DisplaySize::Small,
                    KeyCode::Char('2') => display_size = DisplaySize::Medium,
                    KeyCode::Char('3') => display_size = DisplaySize::Large,
                    KeyCode::Char('4') => display_size = DisplaySize::Xlarge,
                    KeyCode::Char(']') => {
                        if let Some(theme) = &mut theme {
                            theme.next();
                        }
                    }
                    KeyCode::Char('[') => {
                        if let Some(theme) = &mut theme {
                            theme.previous();
                        }
                    }
                    _ => {}
                }
            }
        }

        match receiver.try_recv() {
            Ok(message) => view = snapshot_from_response(message?)?,
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = control.shutdown(std::net::Shutdown::Both);
                drawer.clear()?;
                drop(guard);
                bail!("lost connection to timer daemon; the timer may still be running");
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

pub fn preview_theme(mut theme: ThemeSession) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("theme preview requires a terminal");
    }

    let guard = RawTerminal::new(false)?;
    let mut drawer = FrameDrawer::default();
    let mut last_render = Vec::new();

    loop {
        let (terminal_width, terminal_height) = terminal::size().unwrap_or((80, 24));
        let width = usize::from(terminal_width.max(1));
        let mut lines = theme.render(TimerStatus::Running, width);
        let label = format!(
            "Theme: {}  |  [ previous  ] next  |  q exit",
            theme.current_name()
        );
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(center_line(&label, width));
        lines.truncate(usize::from(terminal_height.max(1)));
        if lines != last_render {
            drawer.draw(&lines)?;
            last_render = lines;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char(']') => theme.next(),
                    KeyCode::Char('[') => theme.previous(),
                    _ => {}
                }
            }
        }
    }

    drawer.clear()?;
    drop(guard);
    Ok(())
}

fn render_with_theme(
    view: &TimerView,
    display_size: DisplaySize,
    terminal_width: u16,
    terminal_height: u16,
    theme: Option<&mut ThemeSession>,
) -> Vec<String> {
    let mut rendered = render_timer(view, display_size, terminal_width, terminal_height);
    let Some(theme) = theme else {
        return rendered.lines;
    };
    let art = theme.render(view.status, usize::from(terminal_width.max(1)));
    if art.is_empty() {
        return rendered.lines;
    }

    let extra_height = art.len().saturating_add(1);
    let total_height = usize::from(terminal_height.max(1));
    if rendered.lines.len().saturating_add(extra_height) > total_height {
        let timer_height = total_height.saturating_sub(extra_height).max(1);
        rendered = render_timer(
            view,
            display_size,
            terminal_width,
            u16::try_from(timer_height).unwrap_or(u16::MAX),
        );
    }
    if rendered.lines.len().saturating_add(extra_height) <= total_height {
        rendered.lines.push(String::new());
        rendered.lines.extend(art);
    }
    rendered.lines
}

fn center_line(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length >= width {
        return value.chars().take(width).collect();
    }
    format!("{}{}", " ".repeat((width - length) / 2), value)
}

fn response_reader(stream: UnixStream) -> Receiver<Result<Response>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let response = line
                .context("failed to read timer update")
                .and_then(|line| {
                    serde_json::from_str(&line).context("invalid timer update from daemon")
                });
            if sender.send(response).is_err() {
                break;
            }
        }
    });
    receiver
}

fn snapshot_from_response(response: Response) -> Result<TimerView> {
    match response {
        Response::Snapshot { task } => Ok(task),
        Response::Error { message } => bail!(message),
        _ => Err(anyhow!("unexpected response while attaching to timer")),
    }
}

fn send_change(paths: &AppPaths, request: Request) -> Result<()> {
    match client::request(paths, &request)? {
        Response::Changed { .. } => Ok(()),
        Response::Error { message } => bail!(message),
        _ => bail!("unexpected response from timer daemon"),
    }
}

fn final_line(view: &TimerView) -> String {
    let note = view
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    match (view.kind, view.status) {
        (TimerKind::Alarm, TimerStatus::Done) => {
            let target = view
                .target_at_millis
                .map(format_alarm_clock)
                .unwrap_or_else(|| "scheduled time".to_owned());
            format!("⏰ Alarm #{} · {target}{note}", view.id)
        }
        (_, TimerStatus::Done) => format!("✓ Timer #{} finished{note}", view.id),
        (_, TimerStatus::Stopped) => format!(
            "■ Timer #{} stopped at {}{note}",
            view.id,
            format_millis(view.value_millis, false)
        ),
        (_, TimerStatus::Cancelled) => format!("× Timer #{} cancelled{note}", view.id),
        _ => compact_line(view),
    }
}

fn detached_line(view: &TimerView) -> String {
    if view.kind == TimerKind::Alarm {
        let target = view
            .target_at_millis
            .map(format_alarm_target)
            .unwrap_or_else(|| "unknown time".to_owned());
        let note = view
            .note
            .as_deref()
            .map(|note| format!(" · {note}"))
            .unwrap_or_default();
        format!(
            "[{}] alarm scheduled · {target} · {} remaining · detached{note}",
            view.id,
            format_millis(view.value_millis, true)
        )
    } else {
        format!(
            "[{}] detached · {} {}",
            view.id,
            format_millis(view.value_millis, view.kind.counts_down()),
            view.status.as_str()
        )
    }
}

fn is_control_b(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL)
}

struct RawTerminal {
    clear_on_drop: bool,
}

#[derive(Default)]
struct FrameDrawer {
    lines: usize,
}

impl FrameDrawer {
    fn draw(&mut self, lines: &[String]) -> Result<()> {
        self.clear()?;
        let mut stdout = io::stdout();
        for (index, line) in lines.iter().enumerate() {
            queue!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            write!(stdout, "{line}")?;
            if index + 1 < lines.len() {
                write!(stdout, "\r\n")?;
            }
        }
        stdout.flush()?;
        self.lines = lines.len();
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        if self.lines == 0 {
            return Ok(());
        }

        let mut stdout = io::stdout();
        queue!(stdout, MoveToColumn(0))?;
        if self.lines > 1 {
            queue!(stdout, MoveUp((self.lines - 1) as u16))?;
        }
        for index in 0..self.lines {
            queue!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
            if index + 1 < self.lines {
                write!(stdout, "\r\n")?;
            }
        }
        if self.lines > 1 {
            queue!(stdout, MoveUp((self.lines - 1) as u16))?;
        }
        queue!(stdout, MoveToColumn(0))?;
        stdout.flush()?;
        self.lines = 0;
        Ok(())
    }
}

impl Drop for FrameDrawer {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

impl RawTerminal {
    fn new(clear_on_drop: bool) -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), Hide)?;
        Ok(Self { clear_on_drop })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if self.clear_on_drop {
            let _ = execute!(
                io::stdout(),
                MoveToColumn(0),
                Clear(ClearType::CurrentLine),
                Show
            );
        } else {
            let _ = execute!(io::stdout(), Show);
        }
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_theme_keeps_the_original_rendering() {
        let view = TimerView {
            id: 11,
            kind: TimerKind::Stopwatch,
            status: TimerStatus::Running,
            note: None,
            value_millis: 12_000,
            target_at_millis: None,
        };
        let original = render_timer(&view, DisplaySize::Medium, 120, 40).lines;
        let optional = render_with_theme(&view, DisplaySize::Medium, 120, 40, None);
        assert_eq!(optional, original);
    }
}
