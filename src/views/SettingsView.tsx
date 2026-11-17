import { createResource, createSignal, onMount, Show } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, QualitySensitivity, Settings, SortKey } from "../lib/api";
import { formatBytes } from "../lib/format";
import { initTasks } from "../lib/tasks";
import { actionState, runAction } from "../lib/actions";

const SENSITIVITIES: { value: QualitySensitivity; label: string }[] = [
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
];

const CACHE_LIMITS = [256, 512, 1024, 2048, 5120, 10240, 20480];

/** SET-01..SET-03, THM-10. Changes are saved at once (SET-04). */
export function SettingsView() {
  const [s, setS] = createSignal<Settings | null>(null);
  const [cache, { refetch }] = createResource(() => api.getCacheInfo());
  const [clearing, setClearing] = createSignal(false);
  const [clearError, setClearError] = createSignal<string | null>(null);
  // The value under the slider while it moves; saved on release.
  const [capDraft, setCapDraft] = createSignal<number | null>(null);

  onMount(async () => {
    setS(await api.getSettings());
    await listen<Settings>("settings-changed", (e) => setS(e.payload));
    // D6: Clear Cache is not available while a task runs.
    await initTasks();
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
                    disabled={clearing() || !actionState("data.clearCache").enabled}
                    title={actionState("data.clearCache").reason ?? undefined}
                    onClick={async () => {
                      setClearing(true);
                      setClearError(null);
                      try {
                        await runAction("data.clearCache");
                      } catch (e) {
                        setClearError(errorMessage(e));
                      }
                      await refetch();
                      setClearing(false);
                    }}
                  >
                    {clearing() ? "Clearing…" : "Clear Cache"}
                  </button>
                </div>
              </div>
              <Show when={clearError() ?? actionState("data.clearCache").reason}>
                {(m) => (
                  <div class="field">
                    <span />
                    <p class="hint">{m()}</p>
                  </div>
                )}
              </Show>
            </fieldset>
            <fieldset>
              <legend>Bad photo scan</legend>
              <div class="field">
                <span class="label" id="sens-label">
                  Sensitivity
                </span>
                {/* QUAL-25: a 3-way control; it changes only the blur threshold, so no new analysis. */}
                <div class="segmented" role="radiogroup" aria-labelledby="sens-label">
                  {SENSITIVITIES.map((o) => (
                    <button
                      type="button"
                      role="radio"
                      aria-checked={v().qualitySensitivity === o.value}
                      classList={{ on: v().qualitySensitivity === o.value }}
                      onClick={() => update({ qualitySensitivity: o.value })}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
              </div>
              <div class="field">
                <label for="scancap">Bad photo scan limit</label>
                <div class="inline">
                  {/* QUAL-26: 100 to 1,000 photos, step 100, default 200. */}
                  <input
                    id="scancap"
                    type="range"
                    min={100}
                    max={1000}
                    step={100}
                    value={capDraft() ?? v().scanCap}
                    onInput={(e) => setCapDraft(Number(e.currentTarget.value))}
                    onChange={(e) => update({ scanCap: Number(e.currentTarget.value) }).then(() => setCapDraft(null))}
                  />
                  <output>{(capDraft() ?? v().scanCap).toLocaleString()} photos</output>
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
