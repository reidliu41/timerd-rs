use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use nix::libc;
use nix::unistd::setsid;
use serde::{Deserialize, Serialize};

use crate::model::{TimerKind, TimerStatus, TimerTask, TimerView, now_millis};
use crate::paths::AppPaths;
use crate::protocol::{Request, Response};

struct RuntimeTask {
    task: TimerTask,
    notify_tty: Option<File>,
    attached: bool,
    notified: bool,
}

struct DaemonState {
    next_id: u64,
    tasks: BTreeMap<u64, RuntimeTask>,
    state_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredState {
    next_id: u64,
    tasks: Vec<TimerTask>,
}

impl DaemonState {
    fn load(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                next_id: 1,
                tasks: BTreeMap::new(),
                state_path: path,
            });
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let stored: StoredState = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let max_id = stored.tasks.iter().map(|task| task.id).max().unwrap_or(0);
        let tasks = stored
            .tasks
            .into_iter()
            .map(|task| {
                (
                    task.id,
                    RuntimeTask {
                        notified: task.status.is_final(),
                        task,
                        notify_tty: None,
                        attached: false,
                    },
                )
            })
            .collect();

        let mut state = Self {
            next_id: stored.next_id.max(max_id.saturating_add(1)).max(1),
            tasks,
            state_path: path,
        };
        let now = now_millis();
        let mut recovered_completion = false;
        for runtime in state.tasks.values_mut() {
            if runtime.task.is_due(now) {
                runtime.task.complete(now);
                runtime.notified = true;
                recovered_completion = true;
            }
        }
        if recovered_completion {
            state.persist()?;
        }
        Ok(state)
    }

    fn persist(&self) -> Result<()> {
        let stored = StoredState {
            next_id: self.next_id,
            tasks: self
                .tasks
                .values()
                .map(|runtime| runtime.task.clone())
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)?;
        let temporary = self.state_path.with_extension("json.tmp");
        fs::write(&temporary, bytes)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        fs::rename(&temporary, &self.state_path)
            .with_context(|| format!("failed to replace {}", self.state_path.display()))?;
        Ok(())
    }

    fn latest_id<F>(&self, mut predicate: F) -> Option<u64>
    where
        F: FnMut(&RuntimeTask) -> bool,
    {
        self.tasks
            .values()
            .filter(|runtime| predicate(runtime))
            .max_by_key(|runtime| runtime.task.last_touched_at_millis)
            .map(|runtime| runtime.task.id)
    }

    fn resolve_active_id(&self, requested: Option<u64>) -> Result<u64, String> {
        if let Some(id) = requested {
            let runtime = self
                .tasks
                .get(&id)
                .ok_or_else(|| format!("timer #{id} does not exist"))?;
            if !runtime.task.status.is_active() {
                return Err(format!("timer #{id} has already finished"));
            }
            return Ok(id);
        }

        self.latest_id(|runtime| runtime.task.status.is_active())
            .ok_or_else(|| "no active timers".to_owned())
    }
}

pub fn ensure_running(paths: &AppPaths) -> Result<()> {
    if ping(&paths.socket) {
        return Ok(());
    }

    let executable = std::env::current_exe().context("failed to locate timer executable")?;
    let daemon_log = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&paths.daemon_log)
        .with_context(|| format!("failed to open {}", paths.daemon_log.display()))?;

    Command::new(executable)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(daemon_log))
        .current_dir("/")
        .spawn()
        .context("failed to start timer daemon")?;

    for _ in 0..100 {
        if ping(&paths.socket) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }

    let details = fs::read_to_string(&paths.daemon_log)
        .ok()
        .and_then(|contents| contents.lines().next_back().map(str::to_owned))
        .unwrap_or_else(|| "no diagnostic output was recorded".to_owned());
    bail!("timer daemon did not become ready: {details}")
}

fn ping(socket: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    if serde_json::to_writer(&mut stream, &Request::Ping).is_err()
        || stream.write_all(b"\n").is_err()
    {
        return false;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let mut response = String::new();
    if BufReader::new(stream).read_line(&mut response).is_err() {
        return false;
    }
    matches!(serde_json::from_str(&response), Ok(Response::Pong))
}

pub fn run(paths: AppPaths) -> Result<()> {
    let _ = setsid();

    let listener = bind_listener(&paths.socket)?;
    listener.set_nonblocking(true)?;
    fs::set_permissions(&paths.socket, fs::Permissions::from_mode(0o600))?;

    let state = Arc::new(Mutex::new(DaemonState::load(paths.state.clone())?));
    let running = Arc::new(AtomicBool::new(true));

    {
        let state = Arc::clone(&state);
        let running = Arc::clone(&running);
        thread::spawn(move || completion_loop(state, running));
    }

    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                let running = Arc::clone(&running);
                thread::spawn(move || {
                    let _ = handle_connection(stream, state, running);
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.into()),
        }
    }

    let _ = fs::remove_file(&paths.socket);
    Ok(())
}

fn bind_listener(socket: &Path) -> Result<UnixListener> {
    match UnixListener::bind(socket) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            for _ in 0..20 {
                if ping(socket) {
                    bail!("timer daemon is already running");
                }
                thread::sleep(Duration::from_millis(10));
            }
            fs::remove_file(socket)
                .with_context(|| format!("failed to remove stale socket {}", socket.display()))?;
            UnixListener::bind(socket)
                .with_context(|| format!("failed to bind {}", socket.display()))
        }
        Err(error) => Err(error).with_context(|| format!("failed to bind {}", socket.display())),
    }
}

fn completion_loop(state: Arc<Mutex<DaemonState>>, running: Arc<AtomicBool>) {
    while running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
        let now = now_millis();
        let mut state = state.lock().expect("daemon state lock poisoned");
        let mut changed = false;

        for runtime in state.tasks.values_mut() {
            if runtime.task.is_due(now) {
                runtime.task.complete(now);
                if !runtime.attached {
                    notify_terminal(runtime);
                }
                changed = true;
            }
        }

        if changed {
            let _ = state.persist();
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    state: Arc<Mutex<DaemonState>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?)
        .read_line(&mut line)
        .context("failed to read client request")?;
    let request: Request = serde_json::from_str(&line).context("invalid client request")?;

    if let Request::Attach { id } = request {
        return handle_attach(stream, state, id);
    }

    let shutdown = matches!(request, Request::Shutdown);
    let response = handle_one_shot(request, &state);
    let should_shutdown = shutdown && matches!(response, Response::Ok { .. });
    write_response(&mut stream, &response)?;
    if should_shutdown {
        running.store(false, Ordering::Relaxed);
    }
    Ok(())
}

fn handle_one_shot(request: Request, state: &Arc<Mutex<DaemonState>>) -> Response {
    match handle_one_shot_inner(request, state) {
        Ok(response) => response,
        Err(error) => Response::Error {
            message: error.to_string(),
        },
    }
}

fn handle_one_shot_inner(request: Request, state: &Arc<Mutex<DaemonState>>) -> Result<Response> {
    match request {
        Request::Ping => Ok(Response::Pong),
        Request::Create {
            kind,
            duration_millis,
            note,
            tty_path,
        } => {
            if kind == TimerKind::Countdown && duration_millis.unwrap_or_default() == 0 {
                bail!("countdown duration must be greater than zero");
            }
            if kind == TimerKind::Stopwatch && duration_millis.is_some() {
                bail!("a stopwatch cannot have a countdown duration");
            }

            let now = now_millis();
            let notify_tty = tty_path.as_deref().and_then(open_notification_tty);
            let terminal_notify = notify_tty.is_some();
            let mut state = state.lock().expect("daemon state lock poisoned");
            let id = state.next_id;
            state.next_id = state.next_id.saturating_add(1);
            let task = TimerTask::new(id, kind, duration_millis, sanitize_note(note), now);
            let view = TimerView::from_task(&task, now);
            state.tasks.insert(
                id,
                RuntimeTask {
                    task,
                    notify_tty,
                    attached: false,
                    notified: false,
                },
            );
            state.persist()?;
            Ok(Response::Created {
                task: view,
                terminal_notify,
            })
        }
        Request::List => {
            let now = now_millis();
            let state = state.lock().expect("daemon state lock poisoned");
            let tasks = state
                .tasks
                .values()
                .map(|runtime| TimerView::from_task(&runtime.task, now))
                .collect();
            Ok(Response::Tasks { tasks })
        }
        Request::Pause { id } => change_task(state, id, Change::Pause),
        Request::Resume { id } => change_task(state, id, Change::Resume),
        Request::Toggle { id } => change_task(state, Some(id), Change::Toggle),
        Request::Stop { id } => change_task(state, id, Change::Stop),
        Request::Clear => {
            let mut state = state.lock().expect("daemon state lock poisoned");
            let before = state.tasks.len();
            state
                .tasks
                .retain(|_, runtime| !runtime.task.status.is_final());
            let count = before - state.tasks.len();
            state.persist()?;
            Ok(Response::Cleared { count })
        }
        Request::Shutdown => {
            let state = state.lock().expect("daemon state lock poisoned");
            if state
                .tasks
                .values()
                .any(|runtime| runtime.task.status.is_active())
            {
                bail!("cannot stop the daemon while timers are active; stop them first");
            }
            Ok(Response::Ok {
                message: "timer daemon stopped".to_owned(),
            })
        }
        Request::Attach { .. } => unreachable!(),
    }
}

enum Change {
    Pause,
    Resume,
    Toggle,
    Stop,
}

fn change_task(
    state: &Arc<Mutex<DaemonState>>,
    requested: Option<u64>,
    change: Change,
) -> Result<Response> {
    let now = now_millis();
    let mut state = state.lock().expect("daemon state lock poisoned");
    let id = match (&change, requested) {
        (_, Some(id)) => state
            .resolve_active_id(Some(id))
            .map_err(|error| anyhow!(error))?,
        (Change::Pause, None) => state
            .latest_id(|runtime| runtime.task.status == TimerStatus::Running)
            .ok_or_else(|| anyhow!("no running timers"))?,
        (Change::Resume, None) => state
            .latest_id(|runtime| runtime.task.status == TimerStatus::Paused)
            .ok_or_else(|| anyhow!("no paused timers"))?,
        (_, None) => state
            .resolve_active_id(None)
            .map_err(|error| anyhow!(error))?,
    };

    let runtime = state.tasks.get_mut(&id).expect("resolved task must exist");
    match change {
        Change::Pause => runtime.task.pause(now),
        Change::Resume => runtime.task.resume(now),
        Change::Toggle => runtime.task.toggle_pause(now),
        Change::Stop => runtime.task.stop(now),
    }
    .map_err(|error| anyhow!(error))?;

    if runtime.task.status.is_final() {
        runtime.notified = true;
        runtime.notify_tty = None;
    }
    let view = TimerView::from_task(&runtime.task, now);
    state.persist()?;
    Ok(Response::Changed { task: view })
}

fn handle_attach(
    mut stream: UnixStream,
    state: Arc<Mutex<DaemonState>>,
    requested: Option<u64>,
) -> Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let id = {
        let mut state = state.lock().expect("daemon state lock poisoned");
        let id = match requested {
            Some(id) => state.resolve_active_id(Some(id)),
            None => state
                .latest_id(|runtime| runtime.task.status.is_active() && !runtime.attached)
                .ok_or_else(|| "no detached timers".to_owned()),
        };
        let id = match id {
            Ok(id) => id,
            Err(message) => {
                drop(state);
                return write_response(&mut stream, &Response::Error { message });
            }
        };
        let runtime = state.tasks.get_mut(&id).expect("resolved task must exist");
        if runtime.attached {
            drop(state);
            return write_response(
                &mut stream,
                &Response::Error {
                    message: format!("timer #{id} is already attached"),
                },
            );
        }
        runtime.attached = true;
        runtime.task.last_touched_at_millis = now_millis();
        let _ = state.persist();
        id
    };

    let mut delivered_final = false;
    loop {
        let view = {
            let state = state.lock().expect("daemon state lock poisoned");
            let Some(runtime) = state.tasks.get(&id) else {
                break;
            };
            TimerView::from_task(&runtime.task, now_millis())
        };
        let final_state = view.status.is_final();
        if write_response(&mut stream, &Response::Snapshot { task: view }).is_err() {
            break;
        }
        if final_state {
            delivered_final = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let mut state = state.lock().expect("daemon state lock poisoned");
    if let Some(runtime) = state.tasks.get_mut(&id) {
        runtime.attached = false;
        runtime.task.last_touched_at_millis = now_millis();
        if delivered_final {
            runtime.notified = true;
            runtime.notify_tty = None;
        } else if runtime.task.status == TimerStatus::Done && !runtime.notified {
            notify_terminal(runtime);
        }
    }
    let _ = state.persist();
    Ok(())
}

fn write_response(stream: &mut UnixStream, response: &Response) -> Result<()> {
    serde_json::to_writer(&mut *stream, response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn open_notification_tty(path: &str) -> Option<File> {
    let path = Path::new(path);
    let path_text = path.to_str()?;
    if !(path_text.starts_with("/dev/pts/")
        || path_text.starts_with("/dev/tty")
        || path_text == "/dev/console")
    {
        return None;
    }
    let metadata = fs::metadata(path).ok()?;
    if !metadata.file_type().is_char_device() {
        return None;
    }
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .ok()
}

fn notify_terminal(runtime: &mut RuntimeTask) {
    if runtime.notified || runtime.task.status != TimerStatus::Done {
        return;
    }
    let note = runtime
        .task
        .note
        .as_deref()
        .map(|note| format!(" · {note}"))
        .unwrap_or_default();
    let message = format!(
        "\r\n[timer #{}] ✓ countdown finished{}\r\n",
        runtime.task.id, note
    );
    if let Some(mut tty) = runtime.notify_tty.take() {
        let _ = tty.write_all(message.as_bytes());
        let _ = tty.flush();
    }
    runtime.notified = true;
}

fn sanitize_note(note: Option<String>) -> Option<String> {
    let cleaned: String = note?
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use super::*;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn test_state() -> (Arc<Mutex<DaemonState>>, PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "timer-rs-test-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let state_path = directory.join("state.json");
        let state = DaemonState::load(state_path).unwrap();
        (Arc::new(Mutex::new(state)), directory)
    }

    #[test]
    fn command_lifecycle_is_persisted() {
        let (state, directory) = test_state();

        let created = handle_one_shot_inner(
            Request::Create {
                kind: TimerKind::Countdown,
                duration_millis: Some(60_000),
                note: Some(" tea\n\u{1b}[31m ".to_owned()),
                tty_path: None,
            },
            &state,
        )
        .unwrap();
        let Response::Created { task, .. } = created else {
            panic!("expected created response");
        };
        assert_eq!(task.id, 1);
        assert_eq!(task.note.as_deref(), Some("tea[31m"));
        assert!(handle_one_shot_inner(Request::Shutdown, &state).is_err());

        let paused = change_task(&state, Some(1), Change::Pause).unwrap();
        assert!(matches!(
            paused,
            Response::Changed {
                task: TimerView {
                    status: TimerStatus::Paused,
                    ..
                }
            }
        ));

        let resumed = change_task(&state, Some(1), Change::Resume).unwrap();
        assert!(matches!(
            resumed,
            Response::Changed {
                task: TimerView {
                    status: TimerStatus::Running,
                    ..
                }
            }
        ));

        let stopped = change_task(&state, Some(1), Change::Stop).unwrap();
        assert!(matches!(
            stopped,
            Response::Changed {
                task: TimerView {
                    status: TimerStatus::Cancelled,
                    ..
                }
            }
        ));
        assert!(matches!(
            handle_one_shot_inner(Request::Shutdown, &state).unwrap(),
            Response::Ok { .. }
        ));

        let state_file = directory.join("state.json");
        let restored = DaemonState::load(state_file).unwrap();
        assert_eq!(restored.tasks.len(), 1);
        assert_eq!(
            restored.tasks.get(&1).unwrap().task.status,
            TimerStatus::Cancelled
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn completion_is_exactly_once_in_state() {
        let (state, directory) = test_state();
        let notification_path = directory.join("notification.txt");
        let now = now_millis();
        {
            let mut state = state.lock().unwrap();
            state.tasks.insert(
                1,
                RuntimeTask {
                    task: TimerTask::new(
                        1,
                        TimerKind::Countdown,
                        Some(1_000),
                        Some("done".to_owned()),
                        now.saturating_sub(2_000),
                    ),
                    notify_tty: Some(File::create(&notification_path).unwrap()),
                    attached: false,
                    notified: false,
                },
            );
            let runtime = state.tasks.get_mut(&1).unwrap();
            assert!(runtime.task.is_due(now));
            runtime.task.complete(now);
            notify_terminal(runtime);
            assert_eq!(runtime.task.status, TimerStatus::Done);
            assert!(runtime.notified);
            assert!(!runtime.task.is_due(now.saturating_add(10_000)));
        }
        assert_eq!(
            fs::read_to_string(notification_path).unwrap(),
            "\r\n[timer #1] ✓ countdown finished · done\r\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn overdue_countdown_is_completed_during_recovery() {
        let (state, directory) = test_state();
        let state_path = {
            let mut state = state.lock().unwrap();
            let now = now_millis();
            state.tasks.insert(
                7,
                RuntimeTask {
                    task: TimerTask::new(
                        7,
                        TimerKind::Countdown,
                        Some(1_000),
                        None,
                        now.saturating_sub(5_000),
                    ),
                    notify_tty: None,
                    attached: false,
                    notified: false,
                },
            );
            state.next_id = 8;
            state.persist().unwrap();
            state.state_path.clone()
        };

        let restored = DaemonState::load(state_path).unwrap();
        assert_eq!(
            restored.tasks.get(&7).unwrap().task.status,
            TimerStatus::Done
        );
        assert!(restored.tasks.get(&7).unwrap().notified);
        fs::remove_dir_all(directory).unwrap();
    }
}
