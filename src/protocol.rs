use serde::{Deserialize, Serialize};

use crate::model::{TimerKind, TimerView};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Create {
        kind: TimerKind,
        duration_millis: Option<u64>,
        #[serde(default)]
        target_at_millis: Option<u64>,
        #[serde(default)]
        bell: bool,
        note: Option<String>,
        tty_path: Option<String>,
    },
    List,
    Attach {
        id: Option<u64>,
    },
    Pause {
        id: Option<u64>,
    },
    Resume {
        id: Option<u64>,
    },
    Toggle {
        id: u64,
    },
    Stop {
        id: Option<u64>,
    },
    Snooze {
        id: u64,
        delay_millis: u64,
    },
    Edit {
        id: u64,
        note: Option<String>,
        schedule_millis: Option<u64>,
    },
    Clear,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Created {
        task: TimerView,
        terminal_notify: bool,
    },
    Tasks {
        tasks: Vec<TimerView>,
    },
    Snapshot {
        task: TimerView,
    },
    Changed {
        task: TimerView,
    },
    Cleared {
        count: usize,
    },
    Ok {
        message: String,
    },
    Error {
        message: String,
    },
}
