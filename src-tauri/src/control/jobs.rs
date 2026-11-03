//! The control API's memory: the app's recent events (numbered, so a client can ask for what came
//! after the last one it saw) and the calls made through the API as jobs, with progress taken
//! from those events.

use super::registry::{Cancel, Track};
use serde_json::{json, Map, Value};
use std::collections::VecDeque;

pub const MAX_EVENTS: usize = 2000;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_JOBS: usize = 256;
/// Calls that may be unfinished at once.
pub const MAX_ACTIVE_JOBS: usize = 32;
const JOB_EVENTS: usize = 10;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct EventRec {
    pub seq: u64,
    pub at_ms: u64,
    pub name: String,
    pub payload: Value,
}

impl EventRec {
    pub fn to_json(&self) -> Value {
        json!({"seq": self.seq, "at_ms": self.at_ms, "name": self.name, "payload": self.payload})
    }
}

#[derive(Default)]
pub struct EventLog {
    next: u64,
    list: VecDeque<EventRec>,
}

impl EventLog {
    /// Keeps an event; returns it as kept (big payloads are left out).
    pub fn push(&mut self, name: &str, raw_payload: &str) -> EventRec {
        self.next += 1;
        let payload = if raw_payload.len() > MAX_EVENT_BYTES {
            json!({"truncated": true, "bytes": raw_payload.len()})
        } else {
            serde_json::from_str(raw_payload).unwrap_or_else(|_| Value::String(raw_payload.to_string()))
        };
        let rec = EventRec { seq: self.next, at_ms: now_ms(), name: name.to_string(), payload };
        self.list.push_back(rec.clone());
        while self.list.len() > MAX_EVENTS {
            self.list.pop_front();
        }
        rec
    }

    pub fn last_seq(&self) -> u64 {
        self.next
    }

    /// Events after `after`, optionally only some names, at most `limit`.
    pub fn after(&self, after: u64, names: &[String], limit: usize) -> (Vec<EventRec>, u64) {
        let oldest = self.list.front().map(|e| e.seq).unwrap_or(self.next + 1);
        let out: Vec<EventRec> = self.list.iter().filter(|e| e.seq > after && (names.is_empty() || names.iter().any(|n| n == &e.name))).take(limit).cloned().collect();
        (out, oldest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Cancelling => "cancelling",
            JobState::Succeeded => "succeeded",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }
    pub fn finished(self) -> bool {
        matches!(self, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub command: String,
    /// The arguments, with anything that looks like a secret hidden.
    pub args_shown: Value,
    /// The value of the tracked argument (the game id, say).
    pub key: Option<String>,
    pub state: JobState,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub finished_ms: Option<u64>,
    /// The command has answered.
    pub returned: bool,
    pub returned_ms: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub progress: Option<Value>,
    pub recent: VecDeque<Value>,
    pub track: Option<Track>,
    pub cancel: Option<Cancel>,
    pub note: Option<String>,
    /// For tracks with a `run_field`: the command's answer, naming this run (events of other runs
    /// of the same thing aren't this job's).
    pub run: Option<Value>,
    /// The last event number kept when the job was made: only later events can be its.
    pub since_seq: u64,
    /// Events up to this number were already applied when the command answered.
    pub replayed_to: u64,
    /// A cancel was asked for. Before the command answers it waits (see [`JobTable::returned`]).
    pub cancel_requested: bool,
}

impl Job {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "id": self.id,
            "command": self.command,
            "args": self.args_shown,
            "state": self.state.as_str(),
            "finished": self.state.finished(),
            "created_ms": self.created_ms,
            "updated_ms": self.updated_ms,
            "finished_ms": self.finished_ms,
            "returned": self.returned,
            "cancellable": self.cancel.is_some() && !self.state.finished(),
        });
        if self.cancel_requested {
            v["cancel_requested"] = Value::Bool(true);
        }
        if let Some(r) = &self.result {
            v["result"] = r.clone();
        }
        if let Some(e) = &self.error {
            v["error"] = e.clone();
        }
        if let Some(p) = &self.progress {
            v["progress"] = p.clone();
        }
        if !self.recent.is_empty() {
            v["recent_events"] = Value::Array(self.recent.iter().cloned().collect());
        }
        if let Some(t) = &self.track {
            v["progress_event"] = Value::String(t.event.into());
            v["completes_on_event"] = Value::Bool(t.completes_on_event);
        }
        if let Some(n) = &self.note {
            v["note"] = Value::String(n.clone());
        }
        if let Some(r) = &self.run {
            v["run"] = r.clone();
        }
        v
    }

    /// Whether an event is about this job (its event, its key, and its run once that's known).
    fn owns(&self, rec: &EventRec) -> bool {
        let Some(t) = self.track else { return false };
        if t.event != rec.name || rec.seq <= self.since_seq {
            return false;
        }
        if !t.key_field.is_empty() {
            let k = rec.payload.get(t.key_field).and_then(Value::as_str);
            if k.is_none() || k != self.key.as_deref() {
                return false;
            }
        }
        if !t.run_field.is_empty() {
            // Until the command answers, the run isn't known: the event waits in the log and is
            // looked at again then (see `returned`).
            if !self.returned {
                return false;
            }
            if let Some(r) = &self.run {
                if rec.payload.get(t.run_field) != Some(r) {
                    return false;
                }
            }
        }
        true
    }

    /// Progress from one of its events, and the end when it's a closing one.
    fn apply(&mut self, rec: &EventRec) {
        let Some(t) = self.track else { return };
        self.updated_ms = rec.at_ms;
        self.progress = Some(rec.payload.clone());
        self.recent.push_back(json!({"seq": rec.seq, "payload": rec.payload}));
        while self.recent.len() > JOB_EVENTS {
            self.recent.pop_front();
        }
        if !t.completes_on_event || t.state_field.is_empty() {
            return;
        }
        let st = rec.payload.get(t.state_field).and_then(Value::as_str).unwrap_or("");
        let msg = (!t.message_field.is_empty()).then(|| rec.payload.get(t.message_field).and_then(Value::as_str)).flatten().unwrap_or("").to_string();
        if t.done.contains(&st) {
            self.finish(JobState::Succeeded);
        } else if t.failed.contains(&st) {
            if !self.state.finished() {
                self.error = Some(json!({"code": "command_failed", "message": if msg.is_empty() { format!("{} failed", self.command) } else { msg }}));
            }
            self.finish(JobState::Failed);
        } else if t.cancelled.contains(&st) {
            if !self.state.finished() && !msg.is_empty() {
                self.note = Some(msg);
            }
            self.finish(JobState::Cancelled);
        }
    }

    /// A cancel that didn't go in: the job carries on as it was (and can be cancelled again).
    pub fn cancel_failed(&mut self, why: String) {
        self.cancel_requested = false;
        self.note = Some(why);
        if self.state.finished() {
            return;
        }
        let waits = self.track.map(|t| t.completes_on_event).unwrap_or(false);
        if self.returned && !waits {
            self.finish(JobState::Succeeded);
        } else {
            self.state = JobState::Running;
            self.updated_ms = now_ms();
        }
    }

    fn finish(&mut self, state: JobState) {
        if !self.state.finished() {
            self.state = state;
            let t = now_ms();
            self.finished_ms = Some(t);
            self.updated_ms = t;
        }
    }
}

/// The arguments of a job's cancel command: its `arg` from the job's call, and with a `run_arg`
/// the run the command answered with (a run-bound cancel is never sent without its run, which
/// would stop whichever run is going).
pub fn cancel_args(c: &Cancel, j: &Job) -> Result<Value, String> {
    let mut a = Map::new();
    if !c.arg.is_empty() {
        a.insert(c.arg.to_string(), Value::String(j.key.clone().unwrap_or_default()));
    }
    if !c.run_arg.is_empty() {
        match j.run.as_ref().filter(|r| !r.is_null()) {
            Some(r) => {
                a.insert(c.run_arg.to_string(), r.clone());
            }
            None => return Err(format!("`{}` didn't name its run, so `{}` wasn't sent (it would stop whichever run is going).", j.command, c.command)),
        }
    }
    Ok(Value::Object(a))
}

#[derive(Default)]
pub struct JobTable {
    next: u64,
    jobs: VecDeque<Job>,
}

/// Argument names that may hold something private: shown as "***" in job listings.
fn secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    ["key", "token", "secret", "password", "auth", "headers", "cookie"].iter().any(|s| k.contains(s))
}

pub fn redact(args: &Value) -> Value {
    match args {
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), if secret_key(k) { Value::String("***".into()) } else { redact(v) })).collect::<Map<_, _>>()),
        Value::Array(a) => Value::Array(a.iter().map(redact).collect()),
        other => other.clone(),
    }
}

impl JobTable {
    pub fn active(&self) -> usize {
        self.jobs.iter().filter(|j| !j.state.finished()).count()
    }

    /// A new job; `since_seq`: the last event number kept before the command is called.
    pub fn create(&mut self, command: &str, args: &Value, track: Option<Track>, cancel: Option<Cancel>, since_seq: u64) -> Result<String, ()> {
        if self.active() >= MAX_ACTIVE_JOBS {
            return Err(());
        }
        self.next += 1;
        let id = format!("job-{}", self.next);
        let key = track.and_then(|t| (!t.arg.is_empty()).then(|| args.get(t.arg).and_then(Value::as_str).map(str::to_string)).flatten()).or_else(|| cancel.and_then(|c| (!c.arg.is_empty()).then(|| args.get(c.arg).and_then(Value::as_str).map(str::to_string)).flatten()));
        let t = now_ms();
        self.jobs.push_back(Job {
            id: id.clone(),
            command: command.to_string(),
            args_shown: redact(args),
            key,
            state: JobState::Running,
            created_ms: t,
            updated_ms: t,
            finished_ms: None,
            returned: false,
            returned_ms: None,
            result: None,
            error: None,
            progress: None,
            recent: VecDeque::new(),
            track,
            cancel,
            note: None,
            run: None,
            since_seq,
            replayed_to: 0,
            cancel_requested: false,
        });
        // Old finished jobs make room.
        while self.jobs.len() > MAX_JOBS {
            match self.jobs.iter().position(|j| j.state.finished()) {
                Some(i) => {
                    self.jobs.remove(i);
                }
                None => break,
            }
        }
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn list(&self) -> Vec<Value> {
        self.jobs.iter().rev().map(|j| {
            let mut v = j.to_json();
            if let Some(o) = v.as_object_mut() {
                o.remove("result");
                o.remove("recent_events");
            }
            v
        }).collect()
    }

    /// The command answered. For tracks with a `run_field`, the answer names the run, and
    /// `backlog` (the job's events kept so far, up to event number `upto`) is gone through again
    /// now that its own events can be told apart: a run may report, even finish, before the
    /// command's answer arrives.
    ///
    /// A cancel asked for before this answer waited for it: Some((cancel, its arguments)) is that
    /// cancel, to send now (bound to the run just named), when the work answered and hasn't
    /// already ended. The job stays `cancelling` until then.
    pub fn returned(&mut self, id: &str, outcome: Result<Value, Value>, backlog: &[EventRec], upto: u64) -> Option<(Cancel, Value)> {
        let j = self.get_mut(id)?;
        let t = now_ms();
        j.returned = true;
        j.returned_ms = Some(t);
        j.updated_ms = t;
        match outcome {
            Ok(v) => {
                if j.track.is_some_and(|t| !t.run_field.is_empty()) && !v.is_null() {
                    j.run = Some(v.clone());
                }
                j.result = Some(v);
                let waits = j.track.map(|t| t.completes_on_event).unwrap_or(false);
                let deferred = j.cancel.filter(|_| j.cancel_requested);
                if !waits && deferred.is_none() {
                    j.finish(JobState::Succeeded);
                }
                if j.track.is_some_and(|t| !t.run_field.is_empty()) {
                    for rec in backlog {
                        if !j.state.finished() && j.owns(rec) {
                            j.apply(rec);
                        }
                    }
                    j.replayed_to = upto;
                }
                let c = deferred.filter(|_| !j.state.finished())?;
                match cancel_args(&c, j) {
                    Ok(args) => return Some((c, args)),
                    Err(why) => j.cancel_failed(why),
                }
            }
            Err(e) => {
                j.error = Some(e);
                j.finish(JobState::Failed);
            }
        }
        None
    }

    /// An event came: progress for the jobs it's about, and the end of the ones it finishes.
    pub fn on_event(&mut self, rec: &EventRec) -> bool {
        self.on_event_ended(rec).0
    }

    /// on_event, also answering the jobs the event ended: (job id, command, final state).
    pub fn on_event_ended(&mut self, rec: &EventRec) -> (bool, Vec<(String, String, JobState)>) {
        let mut changed = false;
        let mut ended = vec![];
        for j in self.jobs.iter_mut().filter(|j| !j.state.finished()) {
            // Already gone through when the command answered.
            if rec.seq <= j.replayed_to || !j.owns(rec) {
                continue;
            }
            changed = true;
            j.apply(rec);
            if j.state.finished() {
                ended.push((j.id.clone(), j.command.clone(), j.state));
            }
        }
        (changed, ended)
    }

    pub fn mark(&mut self, id: &str, state: JobState, note: Option<String>) {
        if let Some(j) = self.get_mut(id) {
            // A job that has already ended keeps the note it ended with.
            if j.state.finished() {
                return;
            }
            if state.finished() {
                j.finish(state);
            } else {
                j.state = state;
                j.updated_ms = now_ms();
            }
            if note.is_some() {
                j.note = note;
            }
        }
    }

    /// Background jobs whose command answered a while ago and that the app may have finished
    /// without a closing event: (job id, field of get_state, key).
    pub fn to_reconcile(&self, older_than_ms: u64) -> Vec<(String, &'static str, String)> {
        let now = now_ms();
        self.jobs
            .iter()
            .filter(|j| !j.state.finished() && j.returned && j.returned_ms.is_some_and(|r| now.saturating_sub(r) >= older_than_ms))
            .filter_map(|j| {
                let t = j.track?;
                (t.completes_on_event && !t.active_in.is_empty()).then(|| (j.id.clone(), t.active_in, j.key.clone().unwrap_or_default()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::registry::TRACKS;

    fn install_track() -> Track {
        TRACKS.iter().find(|(n, _)| *n == "install").unwrap().1
    }

    fn setup_track() -> Track {
        TRACKS.iter().find(|(n, _)| *n == "setup_copy").unwrap().1
    }

    /// The app's own setup_copy note: each run is told apart by its number, and every way a run
    /// ends (done, error, cancelled by setup_cancel or by a newer run) ends its job.
    #[test]
    fn setup_jobs_follow_their_own_run_and_end_when_cancelled_or_replaced() {
        let track = setup_track();
        assert_eq!((track.event, track.key_field, track.run_field), ("setup", "id", "run"));
        assert!(track.cancelled.contains(&"cancelled"));
        let mut t = JobTable::default();
        let mut log = EventLog::default();
        // Run 1 for game g, through the API.
        let j1 = t.create("setup_copy", &json!({"id": "g", "path": "/a"}), Some(track), None, log.last_seq()).unwrap();
        // It reports (and here even fails) before its answer comes back: kept, not lost.
        assert!(!t.on_event(&log.push("setup", r#"{"id":"g","run":1,"step":"read"}"#)), "the run isn't known yet");
        let upto = log.last_seq();
        t.returned(&j1, Ok(json!(1)), &log.after(0, &[], 100).0, upto);
        assert_eq!(t.get(&j1).unwrap().state, JobState::Running);
        assert_eq!(t.get(&j1).unwrap().progress.as_ref().unwrap()["step"], "read");
        // The same event isn't counted twice when its on_event arrives late.
        let again = log.after(0, &[], 100).0[0].clone();
        assert!(!t.on_event(&again));
        assert_eq!(t.get(&j1).unwrap().recent.len(), 1);
        // Run 2 for the same game replaces it: run 1 gets its closing event, run 2 goes on.
        let j2 = t.create("setup_copy", &json!({"id": "g", "path": "/b"}), Some(track), None, log.last_seq()).unwrap();
        assert!(t.on_event(&log.push("setup", r#"{"id":"g","run":1,"step":"cancelled","text":"Replaced by a newer setup of this game."}"#)));
        let upto = log.last_seq();
        let since = t.get(&j2).unwrap().since_seq;
        t.returned(&j2, Ok(json!(2)), &log.after(since, &[], 100).0, upto);
        assert_eq!(t.get(&j1).unwrap().state, JobState::Cancelled);
        assert_eq!(t.get(&j1).unwrap().note.as_deref(), Some("Replaced by a newer setup of this game."));
        assert_eq!(t.get(&j2).unwrap().state, JobState::Running, "run 1's closing event isn't run 2's");
        // Run 2's done doesn't touch run 1's job (already ended), and ends run 2's.
        assert!(t.on_event(&log.push("setup", r#"{"id":"g","run":2,"step":"verify","pct":0.5}"#)));
        assert!(t.on_event(&log.push("setup", r#"{"id":"g","run":2,"step":"done"}"#)));
        assert_eq!(t.get(&j2).unwrap().state, JobState::Succeeded);
        assert_eq!(t.get(&j1).unwrap().state, JobState::Cancelled);
        // Cancelled directly (setup_cancel from the window or another API call, not the job's
        // cancel): the run's cancelled event ends the job, so it doesn't stay counted as active.
        let j3 = t.create("setup_copy", &json!({"id": "g", "path": "/c"}), Some(track), None, log.last_seq()).unwrap();
        t.returned(&j3, Ok(json!(3)), &[], log.last_seq());
        assert_eq!(t.active(), 1);
        assert!(t.on_event(&log.push("setup", r#"{"id":"g","run":3,"step":"cancelled","text":"Cancelled."}"#)));
        assert_eq!(t.get(&j3).unwrap().state, JobState::Cancelled);
        assert_eq!(t.active(), 0, "no setup job left running");
        // A run that failed before its answer came back: the replay ends the job as failed.
        let j4 = t.create("setup_copy", &json!({"id": "g", "path": "/missing"}), Some(track), None, log.last_seq()).unwrap();
        log.push("setup", r#"{"id":"g","run":4,"step":"read"}"#);
        log.push("setup", r#"{"id":"g","run":4,"step":"error","text":"That file isn't there anymore."}"#);
        let since = t.get(&j4).unwrap().since_seq;
        t.returned(&j4, Ok(json!(4)), &log.after(since, &[], 100).0, log.last_seq());
        let j = t.get(&j4).unwrap();
        assert_eq!(j.state, JobState::Failed);
        assert_eq!(j.error.as_ref().unwrap()["message"], "That file isn't there anymore.");
        assert_eq!(j.to_json()["run"], 4);
    }

    #[test]
    fn events_are_numbered_bounded_and_filtered() {
        let mut log = EventLog::default();
        for i in 0..(MAX_EVENTS + 5) {
            log.push(if i % 2 == 0 { "install" } else { "toast" }, &format!("{{\"n\":{i}}}"));
        }
        assert_eq!(log.last_seq(), (MAX_EVENTS + 5) as u64);
        let (all, oldest) = log.after(0, &[], 10_000);
        assert_eq!(all.len(), MAX_EVENTS);
        assert_eq!(oldest, 6);
        let (some, _) = log.after(log.last_seq() - 4, &["toast".to_string()], 100);
        assert!(some.iter().all(|e| e.name == "toast"));
        assert_eq!(some.len(), 2);
        let big = "x".repeat(MAX_EVENT_BYTES + 1);
        let rec = log.push("toast", &format!("\"{big}\""));
        assert_eq!(rec.payload["truncated"], true);
    }

    #[test]
    fn background_install_follows_its_events() {
        let mut t = JobTable::default();
        let id = t.create("install", &json!({"id": "zelda64recomp"}), Some(install_track()), None, 0).unwrap();
        t.returned(&id, Ok(Value::Null), &[], 0);
        assert_eq!(t.get(&id).unwrap().state, JobState::Running, "install returns at once; the job waits for its events");
        let mut log = EventLog::default();
        // Another game's progress isn't this job's.
        assert!(!t.on_event(&log.push("install", r#"{"id":"other","phase":"done"}"#)));
        assert!(t.on_event(&log.push("install", r#"{"id":"zelda64recomp","phase":"download","done":5,"total":10}"#)));
        assert_eq!(t.get(&id).unwrap().progress.as_ref().unwrap()["done"], 5);
        assert!(t.on_event(&log.push("install", r#"{"id":"zelda64recomp","phase":"error","msg":"disk full"}"#)));
        let j = t.get(&id).unwrap();
        assert_eq!(j.state, JobState::Failed);
        assert_eq!(j.error.as_ref().unwrap()["message"], "disk full");
    }

    #[test]
    fn plain_commands_end_when_they_answer_and_secrets_are_hidden() {
        let mut t = JobTable::default();
        let id = t.create("github_art", &json!({"game": "x", "sgdbKey": "abc", "nested": {"token": "t"}}), None, None, 0).unwrap();
        let shown = t.get(&id).unwrap().to_json();
        assert_eq!(shown["args"]["sgdbKey"], "***");
        assert_eq!(shown["args"]["nested"]["token"], "***");
        assert_eq!(shown["args"]["game"], "x");
        t.returned(&id, Err(json!({"code": "command_failed"})), &[], 0);
        assert_eq!(t.get(&id).unwrap().state, JobState::Failed);
    }

    #[test]
    fn active_calls_are_capped() {
        let mut t = JobTable::default();
        for _ in 0..MAX_ACTIVE_JOBS {
            t.create("x", &Value::Null, None, None, 0).unwrap();
        }
        assert!(t.create("x", &Value::Null, None, None, 0).is_err());
    }
}
