import { Show } from "solid-js";
import { task, taskPercent, taskText } from "../lib/tasks";

/**
 * QUAL-11: an animated scan icon and the percentage of the running task. Reads only tasks.ts, so
 * any task with a root shows on that root with no feature code. The animation stops with the
 * macOS "Reduce motion" setting (A11Y-03, CSS).
 * - `root`: show only when the task root is this path (folder tile, breadcrumb).
 * - no `root`: show for any task (toolbar).
 */
export function TaskIndicator(props: { root?: string | null; variant: "tile" | "crumb" | "toolbar" }) {
  const t = () => {
    const cur = task();
    if (!cur) return null;
    if (props.variant !== "toolbar" && (!props.root || cur.root !== props.root)) return null;
    return cur;
  };
  return (
    <Show when={t()}>
      {(cur) => (
        <span class={`task-indicator ti-${props.variant}`} title={taskText(cur())} role="status" aria-label={taskText(cur())}>
          <svg class="task-icon" viewBox="0 0 20 20" aria-hidden="true">
            <path
              d="M3 7V4.5A1.5 1.5 0 0 1 4.5 3H7M13 3h2.5A1.5 1.5 0 0 1 17 4.5V7M17 13v2.5a1.5 1.5 0 0 1-1.5 1.5H13M7 17H4.5A1.5 1.5 0 0 1 3 15.5V13"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
            />
            <path class="task-beam" d="M4 10h12" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
          </svg>
          <span class="task-pct">{taskPercent(cur())} %</span>
        </span>
      )}
    </Show>
  );
}
