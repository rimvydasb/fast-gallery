// Thumbnail request queue.
// - GRID-09: requests start in viewport order, visible tiles first.
// - PERF-12: a tile that leaves the viewport removes its job before the job starts.
// - The number of requests in flight is limited, so the backend works on visible tiles first.

interface Job {
  priority: number;
  start: () => void;
  state: "queued" | "running" | "done";
}

const MAX_IN_FLIGHT = 12;
let queue: Job[] = [];
let inFlight = 0;
let visFirst = 0;
let visLast = 0;
let scheduled = false;
let paused = false;

function rank(j: Job): number {
  // Visible tiles first, in index order; then the buffer rows.
  const p = j.priority;
  if (p >= visFirst && p <= visLast) return p - visFirst;
  return 1e6 + (p < visFirst ? visFirst - p : p - visLast);
}

function pump() {
  scheduled = false;
  while (!paused && inFlight < MAX_IN_FLIGHT && queue.length > 0) {
    let best = 0;
    let bestRank = rank(queue[0]);
    for (let i = 1; i < queue.length; i++) {
      const r = rank(queue[i]);
      if (r < bestRank) {
        best = i;
        bestRank = r;
      }
    }
    const job = queue[best];
    queue[best] = queue[queue.length - 1];
    queue.pop();
    job.state = "running";
    inFlight++;
    job.start();
  }
}

function schedule() {
  if (!scheduled) {
    scheduled = true;
    queueMicrotask(pump);
  }
}

export interface ThumbTicket {
  /** Call when the image loaded or failed. */
  done(): void;
  /** Call when the tile unmounts. Removes the job if it has not started. */
  cancel(): void;
}

export function requestThumb(priority: number, start: () => void): ThumbTicket {
  const job: Job = { priority, start, state: "queued" };
  queue.push(job);
  schedule();
  const finish = () => {
    if (job.state === "running") {
      inFlight--;
      schedule();
    }
    job.state = "done";
  };
  return {
    done: finish,
    cancel() {
      if (job.state === "queued") {
        const i = queue.indexOf(job);
        if (i >= 0) queue.splice(i, 1);
        job.state = "done";
      } else finish();
    },
  };
}

let resumeTimer: number | undefined;

/**
 * Pauses new requests while the grid scrolls fast. Tiles that pass through the viewport during a
 * fling are cancelled before their job starts (PERF-12). Requests resume 120 ms after the fast
 * scroll ends.
 */
export function noteFastScroll() {
  paused = true;
  clearTimeout(resumeTimer);
  resumeTimer = window.setTimeout(() => {
    paused = false;
    schedule();
  }, 120);
}

export function setVisibleRange(first: number, last: number) {
  visFirst = first;
  visLast = last;
}

/** For tests and debugging. */
export function queueStats() {
  return { queued: queue.length, inFlight };
}

export function clearQueue() {
  for (const j of queue) j.state = "done";
  queue = [];
}
