use serde::{Deserialize, Serialize};

use crate::model::{TimerKind, TimerView};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Create {
        kind: TimerKind,
        duration_millis: Option<u64>,
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
