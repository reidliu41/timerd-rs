use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

use crate::display::DisplaySize;

const BELL_NOTE: &str = r#"NOTE:
  Bell is disabled by default. Use -b/--bell to send the standard terminal
  BEL character (0x07) when a countdown or alarm completes. Over SSH, the
  local terminal decides whether to play a sound, flash, show an indicator,
  or ignore it. Test support with: printf '\a'

  In iTerm2, disable "Silence bell" to hear a sound. Use --no-bell to
  explicitly keep the bell disabled."#;

#[derive(Debug, Parser)]
#[command(
    name = "timer",
    version,
    about = "A small foreground/background terminal timer",
    args_conflicts_with_subcommands = true,
    after_help = BELL_NOTE
)]
pub struct Cli {
    /// Countdown duration: 15, 15s, 5m, 1h30m, or 00:15:00
    #[arg(value_name = "DURATION")]
    pub duration: Option<String>,

    /// A short note shown with the timer
    #[arg(short = 'n', long, value_name = "TEXT")]
    pub note: Option<String>,

    /// Start detached from the terminal
    #[arg(short = 'd', long)]
    pub detach: bool,

    /// Ring the terminal bell when the countdown finishes
    #[arg(short = 'b', long, conflicts_with = "no_bell")]
    pub bell: bool,

    /// Explicitly disable the terminal bell
    #[arg(long = "no-bell", conflicts_with = "bell")]
    pub no_bell: bool,

    /// Foreground size: small (s/1), medium (m/2), large (l/3), or xlarge (xl/4)
    #[arg(
        short = 's',
        long,
        value_enum,
        value_name = "SIZE",
        conflicts_with = "detach"
    )]
    pub size: Option<DisplaySize>,

    /// Animated foreground theme; run `timer theme list` to see names
    #[arg(
        short = 't',
        long,
        value_name = "THEME",
        value_parser = crate::theme::ThemeNameParser,
        hide_possible_values = true,
        conflicts_with = "detach"
    )]
    pub theme: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a stopwatch that counts up from zero
    #[command(visible_alias = "sw")]
    Stopwatch {
        /// A short note shown with the timer
        #[arg(short = 'n', long, value_name = "TEXT")]
        note: Option<String>,

        /// Start detached from the terminal
        #[arg(short = 'd', long)]
        detach: bool,

        /// Foreground size: small (s/1), medium (m/2), large (l/3), or xlarge (xl/4)
        #[arg(
            short = 's',
            long,
            value_enum,
            value_name = "SIZE",
            conflicts_with = "detach"
        )]
        size: Option<DisplaySize>,

        /// Animated foreground theme; run `timer theme list` to see names
        #[arg(
            short = 't',
            long,
            value_name = "THEME",
            value_parser = crate::theme::ThemeNameParser,
            hide_possible_values = true,
            conflicts_with = "detach"
        )]
        theme: Option<String>,
    },

    /// Start a countdown
    #[command(visible_alias = "cd", after_help = BELL_NOTE)]
    Countdown {
        /// Countdown duration
        #[arg(value_name = "DURATION")]
        duration: String,

        /// A short note shown with the timer
        #[arg(short = 'n', long, value_name = "TEXT")]
        note: Option<String>,

        /// Start detached from the terminal
        #[arg(short = 'd', long)]
        detach: bool,

        /// Ring the terminal bell when the countdown finishes
        #[arg(short = 'b', long, conflicts_with = "no_bell")]
        bell: bool,

        /// Explicitly disable the terminal bell
        #[arg(long = "no-bell", conflicts_with = "bell")]
        no_bell: bool,

        /// Foreground size: small (s/1), medium (m/2), large (l/3), or xlarge (xl/4)
        #[arg(
            short = 's',
            long,
            value_enum,
            value_name = "SIZE",
            conflicts_with = "detach"
        )]
        size: Option<DisplaySize>,

        /// Animated foreground theme; run `timer theme list` to see names
        #[arg(
            short = 't',
            long,
            value_name = "THEME",
            value_parser = crate::theme::ThemeNameParser,
            hide_possible_values = true,
            conflicts_with = "detach"
        )]
        theme: Option<String>,
    },

    /// Schedule a one-time alarm at a local clock time
    #[command(name = "at", visible_alias = "alarm", after_help = BELL_NOTE)]
    Alarm {
        /// Local time: 08:00, 08:00:30, or 2030-10-30 08:00
        #[arg(value_name = "TIME")]
        time: String,

        /// A short description shown with the alarm
        #[arg(short = 'n', long, value_name = "TEXT")]
        note: Option<String>,

        /// Start detached from the terminal
        #[arg(short = 'd', long)]
        detach: bool,

        /// Ring the terminal bell when the alarm fires
        #[arg(short = 'b', long, conflicts_with = "no_bell")]
        bell: bool,

        /// Explicitly disable the terminal bell
        #[arg(long = "no-bell", conflicts_with = "bell")]
        no_bell: bool,

        /// Foreground size: small (s/1), medium (m/2), large (l/3), or xlarge (xl/4)
        #[arg(
            short = 's',
            long,
            value_enum,
            value_name = "SIZE",
            conflicts_with = "detach"
        )]
        size: Option<DisplaySize>,

        /// Animated foreground theme; run `timer theme list` to see names
        #[arg(
            short = 't',
            long,
            value_name = "THEME",
            value_parser = crate::theme::ThemeNameParser,
            hide_possible_values = true,
            conflicts_with = "detach"
        )]
        theme: Option<String>,
    },

    /// Attach to the latest detached timer
    #[command(visible_alias = "a")]
    Attach {
        /// Timer ID; defaults to the latest detached timer
        id: Option<u64>,

        /// Foreground size: small (s/1), medium (m/2), large (l/3), or xlarge (xl/4)
        #[arg(short = 's', long, value_enum, value_name = "SIZE")]
        size: Option<DisplaySize>,

        /// Animated foreground theme; run `timer theme list` to see names
        #[arg(
            short = 't',
            long,
            value_name = "THEME",
            value_parser = crate::theme::ThemeNameParser,
            hide_possible_values = true
        )]
        theme: Option<String>,
    },

    /// List all timers
    #[command(visible_alias = "ls")]
    List,

    /// Pause a running stopwatch or countdown
    #[command(visible_alias = "p")]
    Pause {
        /// Timer ID; defaults to the latest running timer
        id: Option<u64>,
    },

    /// Resume a paused timer
    #[command(visible_alias = "r")]
    Resume {
        /// Timer ID; defaults to the latest paused timer
        id: Option<u64>,
    },

    /// Stop a stopwatch or cancel a countdown/alarm
    #[command(visible_alias = "x")]
    Stop {
        /// Timer ID; defaults to the latest active timer
        id: Option<u64>,
    },

    /// Remove completed, stopped, and cancelled timers
    Clear,

    /// Inspect display configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// List, preview, and validate animated themes
    #[command(visible_alias = "themes")]
    Theme {
        #[command(subcommand)]
        command: ThemeCommand,
    },

    /// Generate shell completion code
    #[command(
        visible_alias = "completion",
        after_help = r#"Examples:
  # Fish: enable now
  timer completions fish | source

  # Fish: install permanently
  mkdir -p ~/.config/fish/completions
  timer completions fish > ~/.config/fish/completions/timer.fish

  # Bash: enable now
  source <(timer completions bash)

  # Zsh: enable now
  source <(timer completions zsh)

Run `timer completions <SHELL> > FILE` to save generated completion code."#
    )]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Stop the background timer service
    Shutdown,

    #[command(name = "__daemon", hide = true)]
    Daemon,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the configuration file path
    Path,

    /// Show the active configuration
    Show,
}

#[derive(Debug, Subcommand)]
pub enum ThemeCommand {
    /// List built-in and user-installed themes
    List,

    /// Preview a theme; press q or Esc to exit
    Preview {
        /// Theme name shown by `timer theme list`
        #[arg(
            value_parser = crate::theme::ThemeNameParser,
            hide_possible_values = true
        )]
        name: String,
    },

    /// Validate a theme file without installing it
    Check {
        /// Path to a theme TOML file
        #[arg(value_hint = clap::ValueHint::FilePath)]
        file: PathBuf,
    },

    /// Print the directory scanned for user themes
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_countdown() {
        let cli = Cli::try_parse_from(["timer", "15m", "-d", "-b", "-n", "tea"]).unwrap();
        assert_eq!(cli.duration.as_deref(), Some("15m"));
        assert!(cli.detach);
        assert!(cli.bell);
        assert!(!cli.no_bell);
        assert_eq!(cli.note.as_deref(), Some("tea"));
        assert_eq!(cli.size, None);
        assert_eq!(cli.theme, None);
        assert!(cli.command.is_none());

        let sized = Cli::try_parse_from(["timer", "15m", "-s", "xl", "-t", "sleepy-cat"]).unwrap();
        assert_eq!(sized.size, Some(DisplaySize::Xlarge));
        assert_eq!(sized.theme.as_deref(), Some("sleepy-cat"));

        let custom = Cli::try_parse_from(["timer", "15m", "-t", "my-own-theme"]).unwrap();
        assert_eq!(custom.theme.as_deref(), Some("my-own-theme"));
    }

    #[test]
    fn parses_readable_commands_and_aliases() {
        let full = Cli::try_parse_from(["timer", "attach", "3"]).unwrap();
        assert!(matches!(
            full.command,
            Some(Command::Attach {
                id: Some(3),
                size: None,
                theme: None
            })
        ));

        let short = Cli::try_parse_from(["timer", "a"]).unwrap();
        assert!(matches!(
            short.command,
            Some(Command::Attach {
                id: None,
                size: None,
                theme: None
            })
        ));

        let countdown =
            Cli::try_parse_from(["timer", "cd", "5m", "-n", "break", "-s", "3"]).unwrap();
        assert!(matches!(
            countdown.command,
            Some(Command::Countdown {
                duration,
                note: Some(note),
                detach: false,
                bell: false,
                no_bell: false,
                size: Some(DisplaySize::Large),
                theme: None,
            }) if duration == "5m" && note == "break"
        ));

        let alarm =
            Cli::try_parse_from(["timer", "at", "08:00", "-n", "breakfast", "-d", "-b"]).unwrap();
        assert!(matches!(
            alarm.command,
            Some(Command::Alarm {
                time,
                note: Some(note),
                detach: true,
                bell: true,
                no_bell: false,
                size: None,
                theme: None,
            }) if time == "08:00" && note == "breakfast"
        ));

        let alarm_alias = Cli::try_parse_from(["timer", "alarm", "08:00:30"]).unwrap();
        assert!(matches!(alarm_alias.command, Some(Command::Alarm { .. })));

        let no_bell = Cli::try_parse_from(["timer", "at", "08:00", "--no-bell"]).unwrap();
        assert!(matches!(
            no_bell.command,
            Some(Command::Alarm {
                bell: false,
                no_bell: true,
                ..
            })
        ));

        let completions = Cli::try_parse_from(["timer", "completions", "fish"]).unwrap();
        assert!(matches!(
            completions.command,
            Some(Command::Completions { shell: Shell::Fish })
        ));
    }

    #[test]
    fn parses_sizes_and_config_commands() {
        let stopwatch = Cli::try_parse_from(["timer", "sw", "-s", "m"]).unwrap();
        assert!(matches!(
            stopwatch.command,
            Some(Command::Stopwatch {
                size: Some(DisplaySize::Medium),
                theme: None,
                ..
            })
        ));

        let attach = Cli::try_parse_from([
            "timer",
            "a",
            "11",
            "--size",
            "xlarge",
            "--theme",
            "sleepy-cat",
        ])
        .unwrap();
        assert!(matches!(
            attach.command,
            Some(Command::Attach {
                id: Some(11),
                size: Some(DisplaySize::Xlarge),
                theme: Some(theme),
            }) if theme == "sleepy-cat"
        ));

        let theme = Cli::try_parse_from(["timer", "theme", "preview", "sleepy-cat"]).unwrap();
        assert!(matches!(
            theme.command,
            Some(Command::Theme {
                command: ThemeCommand::Preview { name }
            }) if name == "sleepy-cat"
        ));

        let check = Cli::try_parse_from(["timer", "themes", "check", "cat.toml"]).unwrap();
        assert!(matches!(
            check.command,
            Some(Command::Theme {
                command: ThemeCommand::Check { file }
            }) if file == std::path::Path::new("cat.toml")
        ));

        let config = Cli::try_parse_from(["timer", "config", "show"]).unwrap();
        assert!(matches!(
            config.command,
            Some(Command::Config {
                command: ConfigCommand::Show
            })
        ));

        assert!(Cli::try_parse_from(["timer", "sw", "--detach", "--size", "large"]).is_err());
        assert!(Cli::try_parse_from(["timer", "sw", "--detach", "--theme", "sleepy-cat"]).is_err());
        assert!(Cli::try_parse_from(["timer", "sw", "--bell"]).is_err());
        assert!(Cli::try_parse_from(["timer", "15m", "--bell", "--no-bell"]).is_err());
    }

    #[test]
    fn completion_help_explains_how_to_enable_it() {
        let error = Cli::try_parse_from(["timer", "completions", "--help"]).unwrap_err();
        let help = error.to_string();

        assert!(help.contains("timer completions fish | source"));
        assert!(help.contains("timer completions fish > ~/.config/fish/completions/timer.fish"));
        assert!(help.contains("source <(timer completions bash)"));
        assert!(help.contains("source <(timer completions zsh)"));
    }

    #[test]
    fn bell_help_explains_terminal_support_and_usage() {
        for arguments in [
            vec!["timer", "--help"],
            vec!["timer", "countdown", "--help"],
            vec!["timer", "at", "--help"],
        ] {
            let help = Cli::try_parse_from(arguments).unwrap_err().to_string();
            assert!(help.contains("NOTE:"));
            assert!(help.contains("-b/--bell"));
            assert!(help.contains("printf '\\a'"));
            assert!(help.contains("Silence bell"));
            assert!(help.contains("--no-bell"));
        }
    }
}
