use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerKind {
    Stopwatch,
    Countdown,
}

impl TimerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopwatch => "stopwatch",
            Self::Countdown => "countdown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerStatus {
    Running,
    Paused,
    Done,
    Stopped,
    Cancelled,
}

impl TimerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }

    pub fn is_final(self) -> bool {
        !self.is_active()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerTask {
    pub id: u64,
    pub kind: TimerKind,
    pub status: TimerStatus,
    pub note: Option<String>,
    pub duration_millis: Option<u64>,
    pub accumulated_millis: u64,
    pub run_started_at_millis: Option<u64>,
    pub created_at_millis: u64,
    pub last_touched_at_millis: u64,
    pub finished_at_millis: Option<u64>,
}

impl TimerTask {
    pub fn new(
        id: u64,
        kind: TimerKind,
        duration_millis: Option<u64>,
        note: Option<String>,
        now: u64,
    ) -> Self {
        Self {
            id,
            kind,
            status: TimerStatus::Running,
            note: note.filter(|note| !note.trim().is_empty()),
            duration_millis,
            accumulated_millis: 0,
            run_started_at_millis: Some(now),
            created_at_millis: now,
            last_touched_at_millis: now,
            finished_at_millis: None,
        }
    }

    pub fn elapsed_millis(&self, now: u64) -> u64 {
        let current_run = self
            .run_started_at_millis
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        self.accumulated_millis.saturating_add(current_run)
    }

    pub fn display_millis(&self, now: u64) -> u64 {
        match self.kind {
            TimerKind::Stopwatch => self.elapsed_millis(now),
            TimerKind::Countdown => self
                .duration_millis
                .unwrap_or_default()
                .saturating_sub(self.elapsed_millis(now)),
        }
    }

    pub fn is_due(&self, now: u64) -> bool {
        self.kind == TimerKind::Countdown
            && self.status == TimerStatus::Running
            && self.elapsed_millis(now) >= self.duration_millis.unwrap_or_default()
    }

    pub fn complete(&mut self, now: u64) {
        self.accumulated_millis = self.duration_millis.unwrap_or_default();
        self.run_started_at_millis = None;
        self.status = TimerStatus::Done;
        self.finished_at_millis = Some(now);
        self.last_touched_at_millis = now;
    }

    pub fn pause(&mut self, now: u64) -> Result<(), &'static str> {
        if self.status != TimerStatus::Running {
            return Err("timer is not running");
        }
        self.accumulated_millis = self.elapsed_millis(now);
        self.run_started_at_millis = None;
        self.status = TimerStatus::Paused;
        self.last_touched_at_millis = now;
        Ok(())
    }

    pub fn resume(&mut self, now: u64) -> Result<(), &'static str> {
        if self.status != TimerStatus::Paused {
            return Err("timer is not paused");
        }
        self.run_started_at_millis = Some(now);
        self.status = TimerStatus::Running;
        self.last_touched_at_millis = now;
        Ok(())
    }

    pub fn toggle_pause(&mut self, now: u64) -> Result<(), &'static str> {
        match self.status {
            TimerStatus::Running => self.pause(now),
            TimerStatus::Paused => self.resume(now),
            _ => Err("timer has already finished"),
        }
    }

    pub fn stop(&mut self, now: u64) -> Result<(), &'static str> {
        if !self.status.is_active() {
            return Err("timer has already finished");
        }
        self.accumulated_millis = self.elapsed_millis(now);
        self.run_started_at_millis = None;
        self.status = match self.kind {
            TimerKind::Stopwatch => TimerStatus::Stopped,
            TimerKind::Countdown => TimerStatus::Cancelled,
        };
        self.finished_at_millis = Some(now);
        self.last_touched_at_millis = now;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerView {
    pub id: u64,
    pub kind: TimerKind,
    pub status: TimerStatus,
    pub note: Option<String>,
    pub value_millis: u64,
}

impl TimerView {
    pub fn from_task(task: &TimerTask, now: u64) -> Self {
        Self {
            id: task.id,
            kind: task.kind,
            status: task.status,
            note: task.note.clone(),
            value_millis: task.display_millis(now),
        }
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_uses_a_deadline_instead_of_ticks() {
        let task = TimerTask::new(1, TimerKind::Countdown, Some(15_000), None, 1_000);
        assert_eq!(task.display_millis(6_250), 9_750);
        assert!(!task.is_due(15_999));
        assert!(task.is_due(16_000));
    }

    #[test]
    fn pause_and_resume_preserve_elapsed_time() {
        let mut task = TimerTask::new(1, TimerKind::Stopwatch, None, None, 1_000);
        task.pause(6_000).unwrap();
        assert_eq!(task.display_millis(20_000), 5_000);
        task.resume(20_000).unwrap();
        assert_eq!(task.display_millis(23_000), 8_000);
    }

    #[test]
    fn stop_has_kind_specific_final_state() {
        let mut stopwatch = TimerTask::new(1, TimerKind::Stopwatch, None, None, 0);
        stopwatch.stop(1_000).unwrap();
        assert_eq!(stopwatch.status, TimerStatus::Stopped);

        let mut countdown = TimerTask::new(2, TimerKind::Countdown, Some(5_000), None, 0);
        countdown.stop(1_000).unwrap();
        assert_eq!(countdown.status, TimerStatus::Cancelled);
    }
}
