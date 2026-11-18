// The flatten of this window (D1): the backend session that walks the tree of a flat location,
// and its progress. The grid lists the session again on each `flat-progress` (D4).
import { createSignal } from "solid-js";
import { api, DirListing, FlatStatus, GroupKey, isCmdError, SortSpec } from "./api";

/** The flatten session of the window, or null. */
export const [flat, setFlat] = createSignal<FlatStatus | null>(null);

/** True while the walk runs (FLAT-06). */
export const flatWalking = (s: FlatStatus | null = flat()) => !!s && !s.done && !s.capped && !s.cancelled;

let starting: { root: string; p: Promise<FlatStatus> } | null = null;

/** The last `flat-progress` event and the number of events: an event can arrive before the reply
 * of `start_flatten`, and it is then newer than the reply. */
let lastEvent: FlatStatus | null = null;
let eventCount = 0;

/** A file change came while the walk ran: walk again when it ends (FLAT-14). */
let changedDuringWalk = false;

/** Starts a session for `root`, or a refresh of the session of `root` (FLAT-14). */
export async function startFlatten(root: string): Promise<FlatStatus> {
  if (starting?.root === root) return starting.p;
  const mark = eventCount;
  const p = api.startFlatten(root).then((s) => {
    const newer = eventCount > mark && lastEvent?.id === s.id ? lastEvent : s;
    if (flat()?.id !== s.id) changedDuringWalk = false;
    setFlat(newer);
    return newer;
  });
  starting = { root, p };
  try {
    return await p;
  } finally {
    if (starting?.p === p) starting = null;
  }
}

/** The session of `root`: the current one, else a new one. */
async function sessionFor(root: string): Promise<FlatStatus> {
  const s = flat();
  return s && s.root === root ? s : startFlatten(root);
}

/** `FlatProvider.list`: the photos found so far, sorted and grouped by the backend (D4). */
export async function listFlat(root: string, sort: SortSpec, group: GroupKey): Promise<DirListing> {
  const s = await sessionFor(root);
  try {
    return await api.listFlat(s.id, sort, group);
  } catch (e) {
    // The session ended, for example after a reload of the backend: start it again once.
    if (!isCmdError(e) || e.kind !== "notFound") throw e;
    setFlat(null);
    return api.listFlat((await startFlatten(root)).id, sort, group);
  }
}

/** Stops the walk; the photos found so far stay in the grid (FLAT-06). */
export async function cancelFlatten() {
  const s = flat();
  if (!s || !flatWalking(s)) return;
  setFlat(await api.cancelFlatten(s.id));
}

/** "Show More" (FLAT-09): continues the walk for `flattenCap` more photos. */
export async function flattenMore() {
  const s = flat();
  if (!s?.capped) return;
  setFlat(await api.continueFlatten(s.id));
}

/** The window left the flat location of the session: stop the walk (FLAT-07) and forget it. */
export function leaveFlatten() {
  const s = flat();
  if (!s) return;
  setFlat(null);
  changedDuringWalk = false;
  if (flatWalking(s) || s.refreshing) api.cancelFlatten(s.id).catch(() => {});
}

// ---------- Progress and refresh ----------

/**
 * Applies a `flat-progress` event. Returns true if the grid must list again: new photos, or the end
 * of a refresh walk. A refresh walk does not change the listing until it ends.
 */
export function applyFlatProgress(s: FlatStatus): boolean {
  lastEvent = s;
  eventCount++;
  const cur = flat();
  if (!cur || cur.id !== s.id) return false;
  setFlat(s);
  if (!flatWalking(s) && changedDuringWalk) {
    changedDuringWalk = false;
    if (!s.cancelled) startFlatten(s.root).catch(() => {});
  }
  return !s.refreshing;
}

/** FLAT-14: a change of photos in the tree starts a new walk; while a walk runs, after it. */
export function onFlatTreeChanged(root: string) {
  const s = flat();
  if (!s || s.root !== root) return;
  if (flatWalking(s) || s.refreshing) changedDuringWalk = true;
  else if (!s.cancelled) startFlatten(root).catch(() => {});
}

/** Forgets the session (tests). */
export function resetFlatten() {
  setFlat(null);
  starting = null;
  changedDuringWalk = false;
  lastEvent = null;
}
