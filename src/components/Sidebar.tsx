import { createResource, For, Show } from "solid-js";
import { api, baseName } from "../lib/api";
import { settings } from "../lib/store";
import { location, path } from "../lib/navigation";
import { runAction } from "../lib/actions";
import { locationKey, parseLocation, recentLabel } from "../lib/locations";

/** The full path of a location key; a flat location says so. */
function tooltip(key: string): string {
  const l = parseLocation(key);
  return l.kind === "flat" ? `${l.path} (Flattened)` : l.kind === "disk" ? l.path : recentLabel(key);
}

/** `target`: a location key, for example a folder path or "flat:<path>" (FLAT-15). */
function Item(props: { label: string; target: string; icon: string; onRemove?: () => void; removeLabel?: string }) {
  const current = () => {
    const l = location();
    return !!l && locationKey(l) === props.target;
  };
  return (
    <li class="side-item-row">
      <button
        class="side-item"
        classList={{ current: current() }}
        title={tooltip(props.target)}
        onClick={() => runAction("nav.open", parseLocation(props.target))}
      >
        <span class="side-icon" aria-hidden="true">
          {props.icon}
        </span>
        <span class="side-label">{props.label}</span>
      </button>
      <Show when={props.onRemove}>
        <button class="side-remove" aria-label={props.removeLabel} title={props.removeLabel} onClick={() => props.onRemove!()}>
          ×
        </button>
      </Show>
    </li>
  );
}

/** NAV-08: Favorites, Recent folders, and mounted volumes. NAV-09: add and remove Favorites. */
export function Sidebar() {
  const [volumes, { refetch }] = createResource(() => api.listVolumes());
  const favs = () => settings()?.favorites ?? [];
  const recents = () => (settings()?.recents ?? []).slice(0, 8);
  const cur = () => path();

  return (
    <nav class="sidebar" aria-label="Sidebar">
      <section>
        <header class="side-header">
          <h2>Favorites</h2>
          <Show when={cur() && !favs().includes(cur()!)}>
            <button class="side-add" title="Add the current folder to Favorites (⌘D)" onClick={() => runAction("view.toggleFavorite", cur())}>
              +
            </button>
          </Show>
        </header>
        <ul>
          <For each={favs()} fallback={<li class="side-empty">Add a folder with + or ⌘D</li>}>
            {(f) => (
              <Item
                label={baseName(f)}
                target={f}
                icon="★"
                onRemove={() => runAction("view.toggleFavorite", f)}
                removeLabel={`Remove ${baseName(f)} from Favorites`}
              />
            )}
          </For>
        </ul>
      </section>
      <section>
        <header class="side-header">
          <h2>Recent</h2>
        </header>
        <ul>
          <For each={recents()} fallback={<li class="side-empty">No recent folders</li>}>
            {(r) => (
              <Item
                label={recentLabel(r)}
                target={r}
                icon="◷"
                onRemove={() => runAction("view.removeRecent", r)}
                removeLabel={`Remove ${recentLabel(r)} from Recent`}
              />
            )}
          </For>
        </ul>
      </section>
      <section>
        <header class="side-header">
          <h2>Locations</h2>
          <button class="side-add" title="Refresh volumes" onClick={() => refetch()}>
            ↻
          </button>
        </header>
        <ul>
          <For each={volumes() ?? []}>{(v) => <Item label={v.name} target={v.path} icon="⛁" />}</For>
        </ul>
      </section>
    </nav>
  );
}
