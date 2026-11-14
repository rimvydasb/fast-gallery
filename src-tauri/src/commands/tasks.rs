//! Generic task commands (D1). A task kind adds only its own start command.

use tauri::State;

use crate::tasks::TaskStatus;
use crate::AppState;

/// The running task, or null. A window calls this when it opens or reloads.
#[tauri::command]
pub fn get_task_status(state: State<'_, AppState>) -> Option<TaskStatus> {
    state.tasks.status()
}

/// QUAL-05: asks the running task to stop.
#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>) {
    state.tasks.cancel();
}
