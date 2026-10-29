use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerKind {
    Stopwatch,
    Countdown,
    Alarm,
}

impl TimerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopwatch => "stopwatch",
            Self::Countdown => "countdown",
            Self::Alarm => "alarm",
        }
    }

    pub fn counts_down(self) -> bool {
        matches!(self, Self::Countdown | Self::Alarm)
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
    #[serde(default)]
    pub target_at_millis: Option<u64>,
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
            target_at_millis: None,
            accumulated_millis: 0,
            run_started_at_millis: Some(now),
            created_at_millis: now,
            last_touched_at_millis: now,
            finished_at_millis: None,
        }
    }

    pub fn new_alarm(id: u64, target_at_millis: u64, note: Option<String>, now: u64) -> Self {
        Self {
            id,
            kind: TimerKind::Alarm,
            status: TimerStatus::Running,
            note: note.filter(|note| !note.trim().is_empty()),
            duration_millis: None,
            target_at_millis: Some(target_at_millis),
            accumulated_millis: 0,
            run_started_at_millis: None,
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
            TimerKind::Alarm if self.status.is_active() => self
                .target_at_millis
                .unwrap_or_default()
                .saturating_sub(now),
            TimerKind::Alarm => self.accumulated_millis,
        }
    }

    pub fn is_due(&self, now: u64) -> bool {
        if self.status != TimerStatus::Running {
            return false;
        }
        match self.kind {
            TimerKind::Stopwatch => false,
            TimerKind::Countdown => {
                self.elapsed_millis(now) >= self.duration_millis.unwrap_or_default()
            }
            TimerKind::Alarm => now >= self.target_at_millis.unwrap_or_default(),
        }
    }

    pub fn complete(&mut self, now: u64) {
        self.accumulated_millis = self.duration_millis.unwrap_or_default();
        self.run_started_at_millis = None;
        self.status = TimerStatus::Done;
        self.finished_at_millis = Some(now);
        self.last_touched_at_millis = now;
    }

    pub fn pause(&mut self, now: u64) -> Result<(), &'static str> {
        if self.kind == TimerKind::Alarm {
            return Err("alarms cannot be paused; cancel the alarm instead");
        }
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
        if self.kind == TimerKind::Alarm {
            return Err("alarms cannot be resumed");
        }
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
        self.accumulated_millis = match self.kind {
            TimerKind::Alarm => self.display_millis(now),
            TimerKind::Stopwatch | TimerKind::Countdown => self.elapsed_millis(now),
        };
        self.run_started_at_millis = None;
        self.status = match self.kind {
            TimerKind::Stopwatch => TimerStatus::Stopped,
            TimerKind::Countdown | TimerKind::Alarm => TimerStatus::Cancelled,
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
    #[serde(default)]
    pub target_at_millis: Option<u64>,
}

impl TimerView {
    pub fn from_task(task: &TimerTask, now: u64) -> Self {
        Self {
            id: task.id,
            kind: task.kind,
            status: task.status,
            note: task.note.clone(),
            value_millis: task.display_millis(now),
            target_at_millis: task.target_at_millis,
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

    #[test]
    fn alarm_tracks_an_absolute_deadline_and_cannot_pause() {
        let mut alarm = TimerTask::new_alarm(3, 10_000, Some("breakfast".to_owned()), 1_000);
        assert_eq!(alarm.display_millis(4_000), 6_000);
        assert!(!alarm.is_due(9_999));
        assert!(alarm.is_due(10_000));
        assert_eq!(
            alarm.pause(5_000),
            Err("alarms cannot be paused; cancel the alarm instead")
        );
        alarm.stop(5_000).unwrap();
        assert_eq!(alarm.status, TimerStatus::Cancelled);
        assert_eq!(alarm.display_millis(9_000), 5_000);
    }

    #[test]
    fn old_state_without_alarm_target_still_deserializes() {
        let task: TimerTask = serde_json::from_value(serde_json::json!({
            "id": 1,
            "kind": "countdown",
            "status": "running",
            "note": null,
            "duration_millis": 60000,
            "accumulated_millis": 0,
            "run_started_at_millis": 1000,
            "created_at_millis": 1000,
            "last_touched_at_millis": 1000,
            "finished_at_millis": null
        }))
        .unwrap();
        assert_eq!(task.kind, TimerKind::Countdown);
        assert_eq!(task.target_at_millis, None);
    }
}
