import { createResource, createSignal, onMount, Show } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { api, Settings, SortKey } from "../lib/api";
import { formatBytes } from "../lib/format";

const CACHE_LIMITS = [256, 512, 1024, 2048, 5120, 10240, 20480];

/** SET-01..SET-03, THM-10. Changes are saved at once (SET-04). */
export function SettingsView() {
  const [s, setS] = createSignal<Settings | null>(null);
  const [cache, { refetch }] = createResource(() => api.getCacheInfo());
  const [clearing, setClearing] = createSignal(false);

  onMount(async () => {
    setS(await api.getSettings());
    await listen<Settings>("settings-changed", (e) => setS(e.payload));
    await getCurrentWindow().show();
  });

  const update = async (patch: Partial<Settings>) => setS(await api.updateSettings(patch));

  return (
    <main class="settings" onKeyDown={(e) => e.key === "Escape" && getCurrentWindow().close()}>
      <Show when={s()}>
        {(v) => (
          <form onSubmit={(e) => e.preventDefault()}>
            <fieldset>
              <legend>Grid</legend>
              <div class="field">
                <label for="sort">Default sort order</label>
                <div class="inline">
                  <select
                    id="sort"
                    value={v().defaultSort.key}
                    onChange={(e) => update({ defaultSort: { ...v().defaultSort, key: e.currentTarget.value as SortKey } })}
                  >
                    <option value="name">Name</option>
                    <option value="modified">Date Modified</option>
                    <option value="taken">Date Taken</option>
                    <option value="size">Size</option>
                  </select>
                  <label class="check">
                    <input
                      type="checkbox"
                      checked={v().defaultSort.desc}
                      onChange={(e) => update({ defaultSort: { ...v().defaultSort, desc: e.currentTarget.checked } })}
                    />
                    Descending
                  </label>
                </div>
              </div>
              <div class="field">
                <label for="tile">Default tile size</label>
                <div class="inline">
                  <input
                    id="tile"
                    type="range"
                    min={96}
                    max={512}
                    step={4}
                    value={v().defaultTileSize}
                    onChange={(e) => update({ defaultTileSize: Number(e.currentTarget.value) })}
                  />
                  <output>{v().defaultTileSize} px</output>
                </div>
              </div>
              <div class="field">
                <span />
                <label class="check">
                  <input type="checkbox" checked={v().showHidden} onChange={(e) => update({ showHidden: e.currentTarget.checked })} />
                  Show hidden files at start
                </label>
              </div>
            </fieldset>
            <fieldset>
              <legend>Folder preview</legend>
              <div class="field">
                <label for="depth">Search depth</label>
                <div class="inline">
                  <input
                    id="depth"
                    type="range"
                    min={1}
                    max={10}
                    step={1}
                    value={v().previewDepth}
                    onChange={(e) => update({ previewDepth: Number(e.currentTarget.value) })}
                  />
                  <output>
                    {v().previewDepth} {v().previewDepth === 1 ? "level" : "levels"}
                  </output>
                </div>
              </div>
            </fieldset>
            <fieldset>
              <legend>Thumbnail cache</legend>
              <div class="field">
                <label for="limit">Cache limit</label>
                <select id="limit" value={v().cacheLimitMb} onChange={(e) => update({ cacheLimitMb: Number(e.currentTarget.value) }).then(() => refetch())}>
                  {CACHE_LIMITS.map((mb) => (
                    <option value={mb}>{formatBytes(mb * 1024 * 1024)}</option>
                  ))}
                  <Show when={!CACHE_LIMITS.includes(v().cacheLimitMb)}>
                    <option value={v().cacheLimitMb}>{formatBytes(v().cacheLimitMb * 1024 * 1024)}</option>
                  </Show>
                </select>
              </div>
              <div class="field">
                <span class="label">In use</span>
                <div class="inline">
                  <span>{cache() ? formatBytes(cache()!.used) : "…"}</span>
                  <button
                    type="button"
                    class="btn"
                    disabled={clearing()}
                    onClick={async () => {
                      setClearing(true);
                      await api.clearCache();
                      await refetch();
                      setClearing(false);
                    }}
                  >
                    {clearing() ? "Clearing…" : "Clear Cache"}
                  </button>
                </div>
              </div>
            </fieldset>
            <fieldset>
              <legend>Viewer</legend>
              <div class="field">
                <label for="slide">Slideshow interval</label>
                <select id="slide" value={v().slideshowSeconds} onChange={(e) => update({ slideshowSeconds: Number(e.currentTarget.value) })}>
                  {[1, 2, 3, 5, 10, 30].map((n) => (
                    <option value={n}>{n} s</option>
                  ))}
                </select>
              </div>
            </fieldset>
          </form>
        )}
      </Show>
    </main>
  );
}
