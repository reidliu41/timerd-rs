use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use crossterm::{execute, queue};

use crate::client;
use crate::duration::format_millis;
use crate::model::{TimerKind, TimerStatus, TimerView};
use crate::paths::AppPaths;
use crate::protocol::{Request, Response};

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

pub fn run_foreground(paths: &AppPaths, id: Option<u64>) -> Result<()> {
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

    let guard = RawTerminal::new(true)?;
    let mut last_render = String::new();
    let mut prefix = false;

    loop {
        let rendered = render_timer(&view);
        if rendered != last_render {
            draw_line(&rendered)?;
            last_render = rendered;
        }

        if view.status.is_final() {
            let _ = control.shutdown(std::net::Shutdown::Both);
            drop(guard);
            println!("{}", final_line(&view));
            return Ok(());
        }

        while let Ok(message) = receiver.try_recv() {
            view = snapshot_from_response(message?)?;
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
                    drop(guard);
                    println!("{}", detached_line(&view));
                    return Ok(());
                }

                match key.code {
                    KeyCode::Char(' ') => {
                        send_change(paths, Request::Toggle { id: view.id })?;
                    }
                    KeyCode::Char('x') => {
                        send_change(paths, Request::Stop { id: Some(view.id) })?;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        send_change(paths, Request::Stop { id: Some(view.id) })?;
                    }
                    _ => {}
                }
            }
        }

        match receiver.try_recv() {
            Ok(message) => view = snapshot_from_response(message?)?,
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = control.shutdown(std::net::Shutdown::Both);
                drop(guard);
                bail!("lost connection to timer daemon; the timer may still be running");
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
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

fn render_timer(view: &TimerView) -> String {
    let symbol = match view.status {
        TimerStatus::Running if view.kind == TimerKind::Stopwatch => "⏱",
        TimerStatus::Running => "⏳",
        TimerStatus::Paused => "⏸",
        TimerStatus::Done => "✓",
        TimerStatus::Stopped | TimerStatus::Cancelled => "×",
    };
    let round_up = view.kind == TimerKind::Countdown && view.status.is_active();
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
    format!(
        "{symbol} {}{note}{paused}  [#{}]",
        format_millis(view.value_millis, round_up),
        view.id
    )
}

fn draw_line(line: &str) -> Result<()> {
    let mut stdout = io::stdout();
    queue!(stdout, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
    write!(stdout, "{line}")?;
    stdout.flush()?;
    Ok(())
}

fn final_line(view: &TimerView) -> String {
    let note = view
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    match view.status {
        TimerStatus::Done => format!("✓ Timer #{} finished{note}", view.id),
        TimerStatus::Stopped => format!(
            "■ Timer #{} stopped at {}{note}",
            view.id,
            format_millis(view.value_millis, false)
        ),
        TimerStatus::Cancelled => format!("× Timer #{} cancelled{note}", view.id),
        _ => render_timer(view),
    }
}

fn detached_line(view: &TimerView) -> String {
    let round_up = view.kind == TimerKind::Countdown;
    format!(
        "[{}] detached · {} {}",
        view.id,
        format_millis(view.value_millis, round_up),
        view.status.as_str()
    )
}

fn is_control_b(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL)
}

struct RawTerminal {
    clear_on_drop: bool,
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
