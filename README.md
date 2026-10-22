# timer-rs

`timer-rs` is a small Linux terminal timer. It installs one command, `timer`,
and supports both a stopwatch and a countdown. A per-user background process
owns timer state, so the foreground display can detach and attach again without
stopping the timer.

## Install

```bash
cargo install --path .
```

The Cargo package is named `timer-rs`; the installed executable is named
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
timer 25m --note "起来活动一下"
timer 25m -n "起来活动一下" -d
timer stopwatch -n "编程用时" -d
```

## Foreground controls

While the compact foreground display is active:

| Key | Action |
| --- | --- |
| `Space` | Pause or resume |
| `d` | Detach while keeping the timer running |
| `Ctrl-B`, then `D` | tmux-style alias for detach |
| `x` | Stop the stopwatch or cancel the countdown |
| `Ctrl-C` | Same as `x` |

Attach again with the complete command or its short alias:

```bash
timer attach
timer a
timer attach 3
timer a 3
```

Without an ID, attach selects the most recently used detached timer. Only one
foreground client may control a timer at a time.

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
[timer #3] ✓ countdown finished · 起来活动一下
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
