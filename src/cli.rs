use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Debug, Parser)]
#[command(
    name = "timer",
    version,
    about = "A small foreground/background terminal timer",
    args_conflicts_with_subcommands = true
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
    },

    /// Start a countdown
    #[command(visible_alias = "cd")]
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
    },

    /// Attach to the latest detached timer
    #[command(visible_alias = "a")]
    Attach {
        /// Timer ID; defaults to the latest detached timer
        id: Option<u64>,
    },

    /// List all timers
    #[command(visible_alias = "ls")]
    List,

    /// Pause a running timer
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

    /// Stop a stopwatch or cancel a countdown
    #[command(visible_alias = "x")]
    Stop {
        /// Timer ID; defaults to the latest active timer
        id: Option<u64>,
    },

    /// Remove completed, stopped, and cancelled timers
    Clear,

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_countdown() {
        let cli = Cli::try_parse_from(["timer", "15m", "-d", "-n", "tea"]).unwrap();
        assert_eq!(cli.duration.as_deref(), Some("15m"));
        assert!(cli.detach);
        assert_eq!(cli.note.as_deref(), Some("tea"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_readable_commands_and_aliases() {
        let full = Cli::try_parse_from(["timer", "attach", "3"]).unwrap();
        assert!(matches!(
            full.command,
            Some(Command::Attach { id: Some(3) })
        ));

        let short = Cli::try_parse_from(["timer", "a"]).unwrap();
        assert!(matches!(short.command, Some(Command::Attach { id: None })));

        let countdown = Cli::try_parse_from(["timer", "cd", "5m", "-d", "-n", "break"]).unwrap();
        assert!(matches!(
            countdown.command,
            Some(Command::Countdown {
                duration,
                note: Some(note),
                detach: true,
            }) if duration == "5m" && note == "break"
        ));

        let completions = Cli::try_parse_from(["timer", "completions", "fish"]).unwrap();
        assert!(matches!(
            completions.command,
            Some(Command::Completions { shell: Shell::Fish })
        ));
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
}
