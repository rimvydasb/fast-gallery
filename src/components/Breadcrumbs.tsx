import { For, onMount, Show } from "solid-js";
import { location, path } from "../lib/navigation";
import { Crumb, providerFor } from "../lib/locations";
import { runAction } from "../lib/actions";
import { pathEditing, pathText, setPathEditing, submitPath } from "../lib/pathEdit";
import { inTaskTree, task } from "../lib/tasks";
import { TaskIndicator } from "./TaskIndicator";
import { Icon, ICONS } from "./Toolbar";

/**
 * NAV-03, NAV-04: a breadcrumb bar with the components of the current location, from its provider.
 * The component of a task root shows the task indicator while the current folder is in its tree.
 * The button at the end, a click on the empty bar, or ⌘L changes the bar to a path field.
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
    <Show when={!pathEditing()} fallback={<PathField />}>
      <nav
        class="breadcrumbs"
        aria-label="Path"
        onClick={(e) => e.target === e.currentTarget && setPathEditing(true)}
      >
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
        <Show when={location()}>
          <button class="crumb-edit" title="Edit Path (⌘L)" aria-label="Edit path" onClick={() => setPathEditing(true)}>
            <Icon d={ICONS.pathEdit} />
          </button>
        </Show>
      </nav>
    </Show>
  );
}

/** The path field. Enter goes to the path; Escape, a click outside, or a bad path restores the crumbs. */
function PathField() {
  let input!: HTMLInputElement;
  let done = false;
  const close = () => {
    done = true;
    setPathEditing(false);
  };
  onMount(() => {
    input.focus();
    input.select();
  });
  return (
    <input
      ref={input}
      class="path-field"
      type="text"
      aria-label="Path"
      spellcheck={false}
      autocomplete="off"
      value={pathText(location())}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          const text = input.value;
          close();
          void submitPath(text);
        } else if (e.key === "Escape") {
          e.preventDefault();
          close();
        }
      }}
      onBlur={() => !done && close()}
    />
  );
}
