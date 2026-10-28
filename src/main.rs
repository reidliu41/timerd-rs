mod cli;
mod client;
mod config;
mod daemon;
mod display;
mod duration;
mod model;
mod paths;
mod protocol;
mod theme;
mod ui;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser};
use clap_complete::generate;

use crate::cli::{Cli, Command, ConfigCommand, ThemeCommand};
use crate::display::DisplaySize;
use crate::duration::{format_millis, parse_duration};
use crate::model::{TimerKind, TimerView};
use crate::paths::AppPaths;
use crate::protocol::{Request, Response};
use crate::theme::{ThemeCatalog, ThemeSession};

fn main() {
    if let Err(error) = run() {
        eprintln!("timer: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if matches!(cli.command, Some(Command::Daemon)) {
        return daemon::run(AppPaths::discover()?);
    }

    match cli.command {
        Some(Command::Stopwatch {
            note,
            detach,
            size,
            theme,
        }) => {
            let size = resolve_display_size(size)?;
            let theme = resolve_theme(theme)?;
            let paths = AppPaths::discover()?;
            create_timer(
                &paths,
                TimerKind::Stopwatch,
                None,
                note,
                detach,
                size,
                theme,
            )
        }
        Some(Command::Countdown {
            duration,
            note,
            detach,
            size,
            theme,
        }) => {
            let duration = parse_duration(&duration)?;
            let size = resolve_display_size(size)?;
            let theme = resolve_theme(theme)?;
            let paths = AppPaths::discover()?;
            create_timer(
                &paths,
                TimerKind::Countdown,
                Some(duration),
                note,
                detach,
                size,
                theme,
            )
        }
        Some(Command::Attach { id, size, theme }) => {
            let size = resolve_display_size(size)?;
            let theme = resolve_theme(theme)?;
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            ui::run_foreground(&paths, id, size, theme)
        }
        Some(Command::List) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            list_timers(&paths)
        }
        Some(Command::Pause { id }) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            print_changed(client::request(&paths, &Request::Pause { id })?)
        }
        Some(Command::Resume { id }) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            print_changed(client::request(&paths, &Request::Resume { id })?)
        }
        Some(Command::Stop { id }) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            print_changed(client::request(&paths, &Request::Stop { id })?)
        }
        Some(Command::Clear) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            match client::expect_response(client::request(&paths, &Request::Clear)?)? {
                Response::Cleared { count } => {
                    println!("cleared {count} finished timer(s)");
                    Ok(())
                }
                _ => bail!("unexpected response from timer daemon"),
            }
        }
        Some(Command::Config { command }) => match command {
            ConfigCommand::Path => {
                println!("{}", config::path()?.display());
                Ok(())
            }
            ConfigCommand::Show => show_config(),
        },
        Some(Command::Theme { command }) => match command {
            ThemeCommand::List => list_themes(),
            ThemeCommand::Preview { name } => ui::preview_theme(ThemeSession::load(&name)?),
            ThemeCommand::Check { file } => {
                let checked = theme::validate_file(&file)?;
                let (width, height) = checked.dimensions();
                println!(
                    "valid theme: {} ({}) · {} fps · {}x{} · {}",
                    checked.id(),
                    checked.name(),
                    checked.fps(),
                    width,
                    height,
                    checked.origin()
                );
                Ok(())
            }
            ThemeCommand::Path => {
                println!("{}", theme::user_theme_dir()?.display());
                Ok(())
            }
        },
        Some(Command::Completions { shell }) => {
            let mut command = Cli::command();
            generate(shell, &mut command, "timer", &mut std::io::stdout());
            Ok(())
        }
        Some(Command::Shutdown) => {
            let paths = AppPaths::discover()?;
            daemon::ensure_running(&paths)?;
            match client::expect_response(client::request(&paths, &Request::Shutdown)?)? {
                Response::Ok { message } => {
                    println!("{message}");
                    Ok(())
                }
                _ => bail!("unexpected response from timer daemon"),
            }
        }
        Some(Command::Daemon) => unreachable!(),
        None => {
            let size = resolve_display_size(cli.size)?;
            let theme = resolve_theme(cli.theme)?;
            if let Some(duration) = cli.duration {
                let duration = parse_duration(&duration)?;
                let paths = AppPaths::discover()?;
                return create_timer(
                    &paths,
                    TimerKind::Countdown,
                    Some(duration),
                    cli.note,
                    cli.detach,
                    size,
                    theme,
                );
            }
            interactive_create(cli.note, cli.detach, size, theme)
        }
    }
}

fn interactive_create(
    note: Option<String>,
    detach: bool,
    size: DisplaySize,
    theme: Option<ThemeSession>,
) -> Result<()> {
    let kind = ui::choose_timer_kind()?;
    let duration = if kind == TimerKind::Countdown {
        let input = ui::prompt("Duration [s/m/h] (e.g. 15s, 5m, 1h30m, 00:15:00): ")?;
        Some(parse_duration(&input)?)
    } else {
        None
    };
    let note = match note {
        Some(note) => Some(note),
        None => {
            let note = ui::prompt("Note (optional): ")?;
            (!note.is_empty()).then_some(note)
        }
    };
    let paths = AppPaths::discover()?;
    create_timer(&paths, kind, duration, note, detach, size, theme)
}

fn create_timer(
    paths: &AppPaths,
    kind: TimerKind,
    duration_millis: Option<u64>,
    note: Option<String>,
    detach: bool,
    size: DisplaySize,
    theme: Option<ThemeSession>,
) -> Result<()> {
    daemon::ensure_running(paths)?;
    let response = client::expect_response(client::request(
        paths,
        &Request::Create {
            kind,
            duration_millis,
            note,
            tty_path: client::current_tty_path(),
        },
    )?)?;

    let Response::Created {
        task,
        terminal_notify,
    } = response
    else {
        bail!("unexpected response from timer daemon");
    };

    if detach {
        let round_up = task.kind == TimerKind::Countdown;
        println!(
            "[{}] {} started · {} · detached",
            task.id,
            task.kind.as_str(),
            format_millis(task.value_millis, round_up)
        );
        if task.kind == TimerKind::Countdown && !terminal_notify {
            eprintln!("warning: the current terminal is unavailable for completion messages");
        }
        return Ok(());
    }

    ui::run_foreground(paths, Some(task.id), size, theme)
}

fn resolve_display_size(override_size: Option<DisplaySize>) -> Result<DisplaySize> {
    match override_size {
        Some(size) => Ok(size),
        None => Ok(config::load()?.display_size),
    }
}

fn resolve_theme(name: Option<String>) -> Result<Option<ThemeSession>> {
    match name.as_deref() {
        None | Some("none") => Ok(None),
        Some(name) => ThemeSession::load(name).map(Some),
    }
}

fn list_themes() -> Result<()> {
    let catalog = ThemeCatalog::load()?;
    if catalog.themes().is_empty() && catalog.invalid().is_empty() {
        println!("no themes installed");
        return Ok(());
    }

    println!(
        "{:<20} {:<24} {:<5} {:<9} SOURCE",
        "NAME", "TITLE", "FPS", "SIZE"
    );
    for theme in catalog.themes() {
        let (width, height) = theme.dimensions();
        println!(
            "{:<20} {:<24} {:<5} {:<9} {}",
            theme.id(),
            theme.name(),
            theme.fps(),
            format!("{width}x{height}"),
            theme.origin()
        );
    }
    for invalid in catalog.invalid() {
        eprintln!(
            "invalid theme `{}` ({}): {}",
            invalid.id,
            invalid.path.display(),
            invalid.error
        );
    }
    Ok(())
}

fn show_config() -> Result<()> {
    let loaded = config::load()?;
    if loaded.exists {
        println!("Config: {}", loaded.path.display());
    } else {
        println!(
            "Config: {} (not found; using defaults)",
            loaded.path.display()
        );
    }
    println!("[display]");
    println!("size = \"{}\"", loaded.display_size);
    Ok(())
}

fn list_timers(paths: &AppPaths) -> Result<()> {
    let response = client::expect_response(client::request(paths, &Request::List)?)?;
    let Response::Tasks { tasks } = response else {
        bail!("unexpected response from timer daemon");
    };
    if tasks.is_empty() {
        println!("no timers");
        return Ok(());
    }

    println!(
        "{:<4} {:<11} {:<10} {:<10} NOTE",
        "ID", "TYPE", "STATUS", "TIME"
    );
    for task in tasks {
        let round_up = task.kind == TimerKind::Countdown && task.status.is_active();
        println!(
            "{:<4} {:<11} {:<10} {:<10} {}",
            task.id,
            task.kind.as_str(),
            task.status.as_str(),
            format_millis(task.value_millis, round_up),
            task.note.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn print_changed(response: Response) -> Result<()> {
    match client::expect_response(response)? {
        Response::Changed { task } => {
            println!("{}", summary(&task));
            Ok(())
        }
        _ => bail!("unexpected response from timer daemon"),
    }
}

fn summary(task: &TimerView) -> String {
    let round_up = task.kind == TimerKind::Countdown && task.status.is_active();
    let note = task
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    format!(
        "[{}] {} · {}{}",
        task.id,
        task.status.as_str(),
        format_millis(task.value_millis, round_up),
        note
    )
}
