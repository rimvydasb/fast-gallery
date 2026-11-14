//! TaskRunner: runs at most 1 long background task for the whole app (D1, QUAL-01), with a task lock
//! on folder trees, throttled progress events, and cancel. Generic: a task kind supplies only its
//! task function and its `TaskDone.result`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

/// Minimum interval between 2 `task-progress` events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    ScanBadPhotos,
    /// Quality metrics and similarity signatures for the images of 1 location (grouping, sorting).
    AnalyzePhotos,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub id: u64,
    pub kind: TaskKind,
    /// Label of the window that started the task.
    pub window: String,
    pub root: Option<String>,
    /// Folder trees that the task locks against file changes.
    pub locks: Vec<String>,
    pub done: u32,
    pub total: u32,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Finished,
    Cancelled,
    Failed,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TaskDone {
    pub id: u64,
    pub kind: TaskKind,
    pub window: String,
    pub root: Option<String>,
    pub outcome: Outcome,
    /// Defined by the task kind.
    pub result: serde_json::Value,
}

/// Errors of the guard checks. The command layer maps them to `CmdError` kinds `busy` and `locked`.
#[derive(Debug, PartialEq, Eq)]
pub enum TaskError {
    Busy(String),
    Locked(String),
}

impl TaskError {
    pub fn message(&self) -> &str {
        match self {
            TaskError::Busy(m) | TaskError::Locked(m) => m,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Sends task events to all windows. `AppHandle` implements it; tests use a recorder.
pub trait TaskEvents: Send + Sync + 'static {
    fn progress(&self, status: &TaskStatus);
    fn done(&self, done: &TaskDone);
}

impl<R: tauri::Runtime> TaskEvents for tauri::AppHandle<R> {
    fn progress(&self, status: &TaskStatus) {
        use tauri::Emitter;
        let _ = self.emit("task-progress", status);
    }
    fn done(&self, done: &TaskDone) {
        use tauri::Emitter;
        let _ = self.emit("task-done", done);
    }
}

struct Running {
    status: TaskStatus,
    cancel: CancelToken,
}

#[derive(Default)]
pub struct TaskRunner {
    cur: Mutex<Option<Running>>,
    seq: AtomicU64,
}

/// What the task function gets: the cancel token and the progress report.
pub struct TaskContext {
    id: u64,
    runner: Arc<TaskRunner>,
    events: Arc<dyn TaskEvents>,
    cancel: CancelToken,
    last_emit: Mutex<Option<Instant>>,
}

impl TaskContext {
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Updates the progress. Emits `task-progress` at most every 100 ms, and always for the last step.
    pub fn report_progress(&self, done: u32, total: u32) {
        let status = {
            let mut cur = self.runner.cur.lock();
            let Some(r) = cur.as_mut().filter(|r| r.status.id == self.id) else { return };
            r.status.done = done;
            r.status.total = total;
            r.status.clone()
        };
        let mut last = self.last_emit.lock();
        let now = Instant::now();
        if done >= total || last.is_none_or(|t| now.duration_since(t) >= PROGRESS_INTERVAL) {
            *last = Some(now);
            self.events.progress(&status);
        }
    }

    fn force_progress(&self) {
        let st = self.runner.status().filter(|s| s.id == self.id);
        if let Some(s) = st {
            self.events.progress(&s);
        }
    }
}

/// True if `path` conflicts with the locked tree `lock`: it is the root, inside the tree, or a folder
/// that contains the root (a move to the Trash of a parent also moves the tree).
pub fn conflicts(path: &str, lock: &str) -> bool {
    let (p, l) = (Path::new(path), Path::new(lock));
    p.starts_with(l) || l.starts_with(p)
}

fn display_name(p: &str) -> String {
    Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

fn busy_message(s: &TaskStatus) -> String {
    match &s.root {
        Some(r) => format!("A scan of “{}” is running.", display_name(r)),
        None => "A task is running.".into(),
    }
}

impl TaskRunner {
    pub fn status(&self) -> Option<TaskStatus> {
        self.cur.lock().as_ref().map(|r| r.status.clone())
    }

    /// `Err(busy)` while a task runs (D6).
    pub fn guard(&self) -> Result<(), TaskError> {
        match self.cur.lock().as_ref() {
            Some(r) => Err(TaskError::Busy(busy_message(&r.status))),
            None => Ok(()),
        }
    }

    /// `Err(locked)` if a path conflicts with the task lock (D6, QUAL-03).
    pub fn guard_paths<S: AsRef<str>>(&self, paths: &[S]) -> Result<(), TaskError> {
        let cur = self.cur.lock();
        let Some(r) = cur.as_ref() else { return Ok(()) };
        for lock in &r.status.locks {
            if paths.iter().any(|p| conflicts(p.as_ref(), lock)) {
                return Err(TaskError::Locked(format!("“{}” is being scanned.", display_name(lock))));
            }
        }
        Ok(())
    }

    /// Starts a task on its own thread. Returns `Err(busy)` if a task runs (QUAL-01).
    /// The task function returns its `TaskDone.result`, or an error message.
    pub fn start<F>(
        self: &Arc<Self>,
        kind: TaskKind,
        window: &str,
        root: Option<String>,
        locks: Vec<String>,
        events: Arc<dyn TaskEvents>,
        f: F,
    ) -> Result<TaskStatus, TaskError>
    where
        F: FnOnce(&TaskContext) -> Result<serde_json::Value, String> + Send + 'static,
    {
        let cancel = CancelToken::default();
        let status = {
            let mut cur = self.cur.lock();
            if let Some(r) = cur.as_ref() {
                return Err(TaskError::Busy(busy_message(&r.status)));
            }
            let id = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
            let status = TaskStatus { id, kind, window: window.to_string(), root, locks, done: 0, total: 0 };
            *cur = Some(Running { status: status.clone(), cancel: cancel.clone() });
            status
        };
        let ctx = TaskContext {
            id: status.id,
            runner: Arc::clone(self),
            events,
            cancel,
            last_emit: Mutex::new(None),
        };
        let st = status.clone();
        let spawned = std::thread::Builder::new().name("task".into()).spawn(move || {
            // All events of a task come from this thread, so they arrive in order.
            ctx.force_progress();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ctx)));
            let (outcome, result) = match r {
                Ok(Ok(v)) if ctx.is_cancelled() => (Outcome::Cancelled, v),
                Ok(Ok(v)) => (Outcome::Finished, v),
                Ok(Err(e)) => (Outcome::Failed, serde_json::json!({ "error": e })),
                Err(_) => (Outcome::Failed, serde_json::json!({ "error": "The task stopped unexpectedly." })),
            };
            if outcome == Outcome::Finished {
                // The indicator shows 100 % before the result opens.
                let total = ctx.runner.status().map(|s| s.total).unwrap_or(0);
                if total > 0 {
                    ctx.report_progress(total, total);
                }
            }
            ctx.runner.cur.lock().take();
            ctx.events.done(&TaskDone {
                id: st.id,
                kind: st.kind,
                window: st.window,
                root: st.root,
                outcome,
                result,
            });
        });
        if let Err(e) = spawned {
            self.cur.lock().take();
            return Err(TaskError::Busy(format!("Could not start the task: {e}")));
        }
        Ok(status)
    }

    /// Asks the running task to stop. The task ends when its current jobs end.
    pub fn cancel(&self) {
        if let Some(r) = self.cur.lock().as_ref() {
            r.cancel.cancel();
        }
    }

    /// A close of the source window cancels its task.
    pub fn cancel_for_window(&self, window: &str) {
        if let Some(r) = self.cur.lock().as_ref().filter(|r| r.status.window == window) {
            r.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[derive(Default)]
    struct Recorder {
        progress: Mutex<Vec<TaskStatus>>,
        done: Mutex<Option<mpsc::Sender<TaskDone>>>,
    }

    impl TaskEvents for Recorder {
        fn progress(&self, s: &TaskStatus) {
            self.progress.lock().push(s.clone());
        }
        fn done(&self, d: &TaskDone) {
            if let Some(tx) = self.done.lock().as_ref() {
                let _ = tx.send(d.clone());
            }
        }
    }

    fn recorder() -> (Arc<Recorder>, mpsc::Receiver<TaskDone>) {
        let (tx, rx) = mpsc::channel();
        let r = Recorder::default();
        *r.done.lock() = Some(tx);
        (Arc::new(r), rx)
    }

    fn start_blocking(
        runner: &Arc<TaskRunner>,
        locks: Vec<String>,
        ev: Arc<Recorder>,
    ) -> (TaskStatus, mpsc::Sender<()>) {
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let s = runner
            .start(TaskKind::ScanBadPhotos, "main", locks.first().cloned(), locks, ev, move |ctx| {
                let _ = go_rx.recv();
                ctx.report_progress(1, 2);
                Ok(serde_json::json!({ "ok": true }))
            })
            .unwrap();
        (s, go_tx)
    }

    #[test]
    fn second_start_is_busy_and_guard_fails_while_running() {
        let runner = Arc::new(TaskRunner::default());
        let (ev, rx) = recorder();
        let (_, go) = start_blocking(&runner, vec!["/a/b".into()], Arc::clone(&ev));
        assert!(matches!(runner.guard(), Err(TaskError::Busy(_))));
        let again = runner.start(TaskKind::ScanBadPhotos, "w2", None, vec![], ev.clone(), |_| Ok(serde_json::Value::Null));
        assert!(matches!(again, Err(TaskError::Busy(_))));
        go.send(()).unwrap();
        let d = rx.recv().unwrap();
        assert_eq!(d.outcome, Outcome::Finished);
        assert_eq!(d.result["ok"], true);
        assert!(runner.guard().is_ok());
        assert!(runner.status().is_none());
    }

    #[test]
    fn guard_paths_rejects_the_tree_and_its_parents() {
        let runner = Arc::new(TaskRunner::default());
        let (ev, rx) = recorder();
        let (_, go) = start_blocking(&runner, vec!["/a/b".into()], ev);
        for bad in ["/a/b", "/a/b/c.jpg", "/a", "/"] {
            assert!(matches!(runner.guard_paths(&[bad]), Err(TaskError::Locked(_))), "{bad}");
        }
        for ok in ["/a/x.jpg", "/a/bb", "/a/bb/c.jpg", "/z"] {
            assert!(runner.guard_paths(&[ok]).is_ok(), "{ok}");
        }
        go.send(()).unwrap();
        rx.recv().unwrap();
        assert!(runner.guard_paths(&["/a/b"]).is_ok());
    }

    #[test]
    fn cancel_gives_cancelled_and_window_close_cancels_only_its_task() {
        let runner = Arc::new(TaskRunner::default());
        let (ev, rx) = recorder();
        runner
            .start(TaskKind::ScanBadPhotos, "main", None, vec![], ev, |ctx| {
                while !ctx.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(serde_json::Value::Null)
            })
            .unwrap();
        runner.cancel_for_window("other");
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        runner.cancel_for_window("main");
        assert_eq!(rx.recv().unwrap().outcome, Outcome::Cancelled);
    }

    #[test]
    fn failure_and_progress_events() {
        let runner = Arc::new(TaskRunner::default());
        let (ev, rx) = recorder();
        runner
            .start(TaskKind::ScanBadPhotos, "main", None, vec![], ev.clone(), |ctx| {
                for i in 0..=10 {
                    ctx.report_progress(i, 10);
                }
                Err("boom".into())
            })
            .unwrap();
        let d = rx.recv().unwrap();
        assert_eq!(d.outcome, Outcome::Failed);
        let p = ev.progress.lock();
        // The start event, then throttled updates; the last step is always sent.
        assert!(p.len() < 11, "{}", p.len());
        assert_eq!(p.last().unwrap().done, 10);
    }
}
