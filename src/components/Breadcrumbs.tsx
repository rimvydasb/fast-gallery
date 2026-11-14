import { For, Show } from "solid-js";
import { location, path } from "../lib/navigation";
import { Crumb, providerFor } from "../lib/locations";
import { runAction } from "../lib/actions";
import { inTaskTree, task } from "../lib/tasks";
import { TaskIndicator } from "./TaskIndicator";

/**
 * NAV-03, NAV-04: a breadcrumb bar with the components of the current location, from its provider.
 * The component of a task root shows the task indicator while the current folder is in its tree.
 */
export function Breadcrumbs() {
  const parts = (): Crumb[] => {
    const l = location();
    return l ? providerFor(l).crumbs(l) : [];
  };
  const showTaskOn = (c: Crumb) => {
    const t = task();
    return !!t && c.target?.kind === "disk" && c.target.path === t.root && inTaskTree(path(), t);
  };
  return (
    <nav class="breadcrumbs" aria-label="Path">
      <For each={parts()}>
        {(c, i) => (
          <>
            <Show when={i() > 0}>
              <span class="crumb-sep" aria-hidden="true">
                ›
              </span>
            </Show>
            <button
              class="crumb"
              classList={{ last: i() === parts().length - 1, group: !c.target }}
              aria-current={i() === parts().length - 1 ? "location" : undefined}
              title={c.title}
              disabled={!c.target}
              onClick={() => c.target && runAction("nav.open", c.target)}
            >
              {c.label}
              <Show when={showTaskOn(c)}>
                <TaskIndicator variant="crumb" root={c.target?.kind === "disk" ? c.target.path : null} />
              </Show>
            </button>
          </>
        )}
      </For>
    </nav>
  );
}
