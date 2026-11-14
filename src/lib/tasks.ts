// Busy state of the app, from the generic task events (D1). Every window has the same state; a
// window that opens or reloads asks for it with `get_task_status`.
import { createSignal } from "solid-js";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { api, baseName, TaskDone, TaskKind, TaskStatus } from "./api";

export const [task, setTask] = createSignal<TaskStatus | null>(null);

/** Text for the toolbar tooltip and the status bar, by task kind. */
const TASK_LABELS: Record<TaskKind, string> = {
  scanBadPhotos: "Scanning for bad photos",
  analyzePhotos: "Analyzing photos",
};

export const taskLabel = (t: TaskStatus) => TASK_LABELS[t.kind] ?? "Working";

/** "Scanning for bad photos: 70 / 200" */
export function taskText(t: TaskStatus): string {
  return t.total > 0 ? `${taskLabel(t)}: ${t.done.toLocaleString()} / ${t.total.toLocaleString()}` : `${taskLabel(t)}…`;
}

export function taskPercent(t: TaskStatus): number {
  return t.total > 0 ? Math.min(100, Math.floor((t.done * 100) / t.total)) : 0;
}

/** True if `p` conflicts with the locked tree `lock`: the root, inside it, or a folder that contains it. */
export function pathConflicts(p: string, lock: string): boolean {
  const inside = (a: string, b: string) => a === b || b === "/" || a.startsWith(b + "/");
  return inside(p, lock) || inside(lock, p);
}

/** True if `p` is the task root or a folder in its tree. */
export function inTaskTree(p: string | null, t: TaskStatus | null = task()): boolean {
  if (!p || !t?.root) return false;
  return p === t.root || t.root === "/" || p.startsWith(t.root + "/");
}

export function lockedBy(paths: readonly string[], t: TaskStatus | null = task()): string | null {
  if (!t) return null;
  for (const lock of t.locks) if (paths.some((p) => pathConflicts(p, lock))) return lock;
  return null;
}

export function busyReason(t: TaskStatus): string {
  return t.root ? `A scan of “${baseName(t.root)}” is running.` : "A task is running.";
}

export function lockedReason(lock: string): string {
  return `“${baseName(lock)}” is being scanned.`;
}

let finished = new Set<number>();

/** Starts to follow the task events. `onDone` gets each finished task. */
export async function initTasks(onDone?: (d: TaskDone) => void): Promise<UnlistenFn[]> {
  const ls = await Promise.all([
    listen<TaskStatus>("task-progress", (e) => {
      // A late progress event of a task that ended does not make the app busy again.
      if (!finished.has(e.payload.id)) setTask(e.payload);
    }),
    listen<TaskDone>("task-done", (e) => {
      finished.add(e.payload.id);
      if (finished.size > 50) finished = new Set([e.payload.id]);
      setTask(null);
      onDone?.(e.payload);
    }),
  ]);
  const t = await api.getTaskStatus();
  if (!t || !finished.has(t.id)) setTask(t);
  return ls;
}
