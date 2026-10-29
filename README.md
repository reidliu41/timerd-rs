# timerd-rs

[![crates.io](https://img.shields.io/crates/v/timerd-rs.svg)](https://crates.io/crates/timerd-rs)

`timerd-rs` is a small Linux terminal timer. Its name combines **timer** with
the Unix convention of using a trailing **d** for a daemon. It installs the
simple command `timer` and supports a stopwatch, a countdown, and one-time
wall-clock alarms. A per-user background process owns timer state, so the
foreground display can detach and attach again without stopping the timer.

## Install

Install [`timerd-rs` from crates.io](https://crates.io/crates/timerd-rs):

```bash
cargo install timerd-rs
```

Or install it from a local checkout:

```bash
cargo install --path .
```

The Cargo package is named `timerd-rs`; the installed executable is named
`timer`.

## Start a timer

Run without arguments for an interactive choice between a stopwatch and a
countdown:

```bash
timer
```

Or start one directly:

```bash
timer stopwatch
timer sw

timer 15
timer 15s
timer 5m
timer 1h30m
timer 00:15:00
timer countdown 15m
timer cd 15m
```

A bare number is interpreted as seconds. Two-part clock input is `MM:SS`, and
three-part clock input is `HH:MM:SS`.

Add a note or start detached:

```bash
timer 25m --note "stretch break"
timer 25m -n "stretch break" -d
timer stopwatch -n "coding session" -d
```

## Schedule an alarm

Use `at` to schedule a one-time reminder at a local clock time. `alarm` is a
readable alias for the same command:

```bash
timer at 08:00 -n "eat breakfast"
timer at 08:00:30 -n "join the call" -d
timer at "2030-10-30 18:45" -n "leave for the airport" -d
timer alarm 20:00 -n "take medicine"
```

Accepted inputs are `HH:MM`, `HH:MM:SS`, `YYYY-MM-DD HH:MM`, and
`YYYY-MM-DD HH:MM:SS`. A clock time such as `08:00` means the next occurrence:
today when it is still ahead, otherwise tomorrow. An explicit date must be in
the future. All inputs use the machine's local timezone.

In the foreground, the large value is the remaining time while the target and
description stay fixed:

```text
                 00:24:37
       alarm #12 · at 08:00 · running · eat breakfast
            d detach | x cancel | +/- size (medium)
```

Alarms are tied to an absolute time and therefore cannot be paused or resumed.
They can be detached, attached, listed, or cancelled like other timers. This
release schedules one occurrence only; it does not create a daily repeating
alarm.

Choose one of four fixed foreground display sizes:

```bash
timer sw --size small
timer sw -s m
timer 15m -s large
timer a -s xl
```

The accepted names are `small`, `medium`, `large`, and `xlarge`. Their short
values are `s`, `m`, `l`, and `xl`; `1`, `2`, `3`, and `4` are also accepted.
`small` keeps the original one-line display. Larger sizes use built-in block
digits and automatically fall back when the terminal is too narrow or short.
Because a detached timer has no foreground display, `--size` and `--detach`
cannot be used together; choose a size when attaching instead.

## Animated themes

Themes are optional. Without `-t`/`--theme`, the foreground display remains
the same plain timer as before. Enable the bundled `sleepy-cat` theme when
starting or attaching to a timer:

```bash
timer 25m -s m -t sleepy-cat
timer sw --theme sleepy-cat
timer a --theme sleepy-cat
```

Discover, preview, and validate themes with:

```bash
timer theme list
timer theme preview sleepy-cat
timer theme check ./my-cat.toml
timer theme path
```

During a themed foreground session, press `]` for the next discovered theme
or `[` for the previous one. The cycle includes `none`, which hides the art
without stopping the timer. These keys only become active when the session was
started or attached with a theme option.

User themes are loaded automatically from
`$XDG_CONFIG_HOME/timerd-rs/themes`, or `~/.config/timerd-rs/themes` when
`XDG_CONFIG_HOME` is not set. The filename is the theme name, so a file named
`my-cat.toml` is selected with `timer 5m -t my-cat`. A theme needs at least one
running frame; paused and done frames are optional:

```toml
version = 1
name = "My Cat"
fps = 4
align = "center"

[[running]]
art = """
   /\___/\
  (  o o  )
  (   ^   )
"""

[[running]]
art = """
   /\___/\
  (  - -  )
  (   ^   )
"""

[[paused]]
art = """
   /\___/\
  (  z z  )
  (   ^   )
"""

[[done]]
art = """
   \  |  /
    \(^)/
     / \
"""
```

Each `art` opening and closing `"""` must be on its own line. Theme art is
printable ASCII data only: terminal control characters are rejected, and frame
counts and dimensions are bounded. To contribute a bundled theme, add one TOML
file to the repository's [`themes`](./themes) directory; the build script discovers
and embeds it automatically, with no Rust registration table to edit.

## Foreground controls

While the foreground display is active:

| Key | Action |
| --- | --- |
| `Space` | Pause or resume |
| `d` | Detach while keeping the timer running |
| `Ctrl-B`, then `D` | tmux-style alias for detach |
| `x` | Stop the stopwatch or cancel the countdown |
| `Ctrl-C` | Same as `x` |
| `+` or `=` | Increase the display size |
| `-` | Decrease the display size |
| `1`–`4` | Select `small` through `xlarge` directly |
| `[` / `]` | Previous / next theme, including `none` (themed sessions only) |

Attach again with the complete command or its short alias:

```bash
timer attach
timer a
timer attach 3
timer a 3
```

Without an ID, attach selects the most recently used detached timer. Only one
foreground client may control a timer at a time.

## Configuration

The built-in default display size is `small`. To change it for future
foreground displays, create `$XDG_CONFIG_HOME/timerd-rs/config.toml`, or
`~/.config/timerd-rs/config.toml` when `XDG_CONFIG_HOME` is not set:

```toml
[display]
size = "large"
```

Configuration accepts the full values `small`, `medium`, `large`, and
`xlarge`. A command-line `--size` value takes precedence over the file, while
the `+`, `-`, and number keys affect only the current foreground session.

Inspect the path and active value with:

```bash
timer config path
timer config show
```

A missing file is valid and uses `small`; an invalid setting reports its file
and parse error. The configuration is read by each new foreground invocation,
so the daemon does not need to restart.

## Manage timers

Every frequent command has a readable name and a short alias:

```bash
timer list             # alias: timer ls
timer pause [ID]       # alias: timer p [ID]; stopwatch/countdown only
timer resume [ID]      # alias: timer r [ID]
timer stop [ID]        # alias: timer x [ID]; also cancels an alarm
timer clear
```

When an ID is omitted, the daemon selects the most recently used compatible
timer. `clear` removes finished, stopped, and cancelled records.

## Shell completions

Generate completion code with `timer completions SHELL` (the singular
`completion` spelling is also accepted). For Fish:

```fish
mkdir -p ~/.config/fish/completions
timer completions fish > ~/.config/fish/completions/timer.fish
```

For Bash:

```bash
mkdir -p ~/.local/share/bash-completion/completions
timer completions bash > ~/.local/share/bash-completion/completions/timer
```

For Zsh, generate `_timer` into a directory included in `fpath`:

```zsh
mkdir -p ~/.zfunc
timer completions zsh > ~/.zfunc/_timer
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

Elvish and PowerShell are also supported. Generated completions cover commands,
aliases, options, shell names, and bundled theme names. User-installed theme
names remain free-form. Regenerate the completion file after an upgrade so
newly added options and bundled themes are included.

## Countdown and alarm completion

For a detached countdown, the daemon keeps a write handle to the terminal that
created it. If that terminal or SSH session is still open, completion appears
there as ordinary terminal output:

```text
[timer #3] ✓ countdown finished · stretch break
```

This does not depend on a desktop notification service, iTerm2 escape codes,
tmux, or screen. If the original terminal has closed, the write is allowed to
fail and the timer still becomes `done`. Its result remains visible in
`timer list`.

A detached alarm uses the same terminal-session mechanism and includes its
target clock and description:

```text
[timer #12] ⏰ 08:00 · eat breakfast
```

An asynchronous completion line can visually interrupt text currently being
typed at the shell prompt. The typed input remains owned by the shell, but a
prompt redraw may occasionally be necessary.

## Files and process model

`timer` is a single crate and a single executable. On first use, that executable
starts itself in a hidden daemon mode. The CLI and foreground renderer talk to
it over a private Unix socket.

Runtime files follow the XDG directory conventions:

```text
$XDG_RUNTIME_DIR/timer-rs/timer.sock
$XDG_STATE_HOME/timer-rs/state.json
$XDG_STATE_HOME/timer-rs/daemon.log
```

When the corresponding variables are unset, state is stored under
`~/.local/state/timer-rs`, and the socket uses a user-specific directory under
`/tmp`.

The daemon can be stopped explicitly:

```bash
timer shutdown
```

Shutdown is refused while any timer is running or paused, because stopping the
service would prevent timely completion messages.

Active task metadata is persisted, but a terminal write handle cannot survive
a daemon restart. On systems configured to kill all user processes at logout,
a future systemd user-service integration may be needed for timers that must
survive a complete logout.
