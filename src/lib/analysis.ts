// The analysis that a grouping or a sort needs (quality metrics, similarity signatures). A listing
// reports the images without a result (`pendingAnalysis`); this module starts the analysis task
// for the location, and the windows list their location again when the task ends.
import { api, errorMessage, TaskDone } from "./api";
import { locationKey } from "./locations";
import { location, reload } from "./navigation";
import { pendingAnalysis, showHidden } from "./store";
import { setTask, task } from "./tasks";
import { toast } from "./toast";

/**
 * By location and need: the images that had no result after the last run (files that cannot be
 * decoded), or Infinity while a run is active and after a cancel. An automatic start needs more
 * missing images than this, so that a file that always fails does not start the task again and again.
 */
const baseline = new Map<string, number>();
/** The key of the run that this window started. */
let running: string | null = null;

/**
 * Starts the analysis of the current location, if the listing has images without a result and no
 * task runs. `manual`: from the "Analyze" button; ignores the baseline and shows errors.
 */
export async function startAnalysis(manual = false) {
  const l = location();
  const p = pendingAnalysis();
  if (!l || !p || task()) return;
  const key = `${locationKey(l)}|${p.need}`;
  if (!manual && p.missing <= (baseline.get(key) ?? 0)) return;
  baseline.set(key, Infinity);
  running = key;
  try {
    setTask(await api.startAnalyzePhotos(locationKey(l), p.need, showHidden()));
  } catch (e) {
    // For example another task runs: the effect in App.tsx tries again when it ends.
    running = null;
    baseline.delete(key);
    if (manual) toast(errorMessage(e), "error");
  }
}

/** Called in each window for each `task-done`. */
export function onAnalysisDone(d: TaskDone, windowLabel: string) {
  if (d.kind !== "analyzePhotos") return;
  if (d.window === windowLabel && running) {
    const key = running;
    running = null;
    if (d.outcome === "finished") {
      baseline.set(key, (d.result as { failed?: number } | null)?.failed ?? 0);
    } else if (d.outcome === "cancelled") {
      toast("The analysis was cancelled.", "info");
    } else {
      const err = (d.result as { error?: string } | null)?.error;
      toast(err ? `The analysis failed: ${err}` : "The analysis failed.", "error");
    }
  }
  // The results are in the shared cache: each window that waits for them lists again.
  if (pendingAnalysis()) reload();
}

/** Forgets the baselines (tests). */
export function resetAnalysis() {
  baseline.clear();
  running = null;
}
