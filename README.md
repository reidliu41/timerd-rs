# timerd-rs

`timerd-rs` is a small Linux terminal timer. Its name combines **timer** with
the Unix convention of using a trailing **d** for a daemon. It installs the
simple command `timer` and supports both a stopwatch and a countdown. A
per-user background process owns timer state, so the foreground display can
detach and attach again without stopping the timer.

## Install

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
timer pause [ID]       # alias: timer p [ID]
timer resume [ID]      # alias: timer r [ID]
timer stop [ID]        # alias: timer x [ID]
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
aliases, options, and shell names.

## Countdown completion

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
