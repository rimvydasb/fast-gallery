import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { api, Entry, imageUrl, thumbUrl } from "../lib/api";
import {
  editMode,
  entryPath,
  folderCount,
  images,
  infoOpen,
  selectOnly,
  setInfoOpen,
  setSettings,
  setViewerIndex,
  settings,
  toggleEditMode,
  trashViewerImage,
  viewerIndex,
} from "../lib/store";
import { Icon, ICONS } from "../components/Toolbar";
import { scrollToIndex } from "./GridView";
import { prefersReducedMotion } from "../lib/ui";

// ---------- Viewer state (one viewer per window) ----------

const [zoom, setZoom] = createSignal<number | "fit">("fit");
const [rotation, setRotation] = createSignal(0);
const [pan, setPan] = createSignal({ x: 0, y: 0 });
const [natural, setNatural] = createSignal<{ w: number; h: number } | null>(null);
const [stageSize, setStageSize] = createSignal({ w: 1, h: 1 });
const [playing, setPlaying] = createSignal(false);
const [failed, setFailed] = createSignal(false);

export const ZOOM_STEP = 1.25;
const MIN_ZOOM = 0.02;
const MAX_ZOOM = 16;

const rotatedSize = () => {
  const n = natural();
  if (!n) return null;
  return rotation() % 180 === 0 ? n : { w: n.h, h: n.w };
};

/** VIEW-01: fit to the window; small images are not scaled up. */
const fitScale = () => {
  const r = rotatedSize();
  const s = stageSize();
  if (!r) return 1;
  return Math.min(s.w / r.w, s.h / r.h, 1);
};
export const scale = () => {
  const z = zoom();
  return z === "fit" ? fitScale() : z;
};

function clampPan(p: { x: number; y: number }, s = scale()) {
  const r = rotatedSize();
  if (!r) return { x: 0, y: 0 };
  const st = stageSize();
  const mx = Math.max(0, (r.w * s - st.w) / 2);
  const my = Math.max(0, (r.h * s - st.h) / 2);
  return { x: Math.max(-mx, Math.min(mx, p.x)), y: Math.max(-my, Math.min(my, p.y)) };
}

/** Zooms to `next` around a point relative to the stage center. */
export function zoomTo(next: number, cx = 0, cy = 0) {
  const s1 = scale();
  const s2 = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, next));
  const p = pan();
  const k = s2 / s1;
  setZoom(s2);
  setPan(clampPan({ x: (p.x - cx) * k + cx, y: (p.y - cy) * k + cy }, s2));
}

export const zoomIn = () => zoomTo(scale() * ZOOM_STEP);
export const zoomOut = () => zoomTo(scale() / ZOOM_STEP);
/** VIEW-07 */
export const zoomFit = () => {
  setZoom("fit");
  setPan({ x: 0, y: 0 });
};
export const zoomActual = () => zoomTo(1);
/** VIEW-14: rotates the view only; the file does not change. */
export const rotate = (deg: number) => {
  setRotation((rotation() + deg + 360) % 360);
  zoomFit();
};

export function go(i: number) {
  const n = images().length;
  if (n === 0) return;
  setViewerIndex(Math.max(0, Math.min(n - 1, i)));
}
export const next = () => go((viewerIndex() ?? 0) + 1);
export const prev = () => go((viewerIndex() ?? 0) - 1);
export const first = () => go(0);
export const last = () => go(images().length - 1);

/** VIEW-09: closes the viewer and selects the current image in the grid. */
export function closeViewer() {
  const vi = viewerIndex();
  setPlaying(false);
  setViewerIndex(null);
  if (vi !== null) {
    const gi = folderCount() + vi;
    selectOnly(gi);
    queueMicrotask(() => scrollToIndex(gi));
  }
}

export const showFilmstrip = () => settings()?.showFilmstrip ?? true;
export async function toggleFilmstrip() {
  const s = settings();
  if (!s) return;
  setSettings({ ...s, showFilmstrip: !s.showFilmstrip });
  setSettings(await api.updateSettings({ showFilmstrip: !s.showFilmstrip }));
}

/** VIEW-13 */
export const toggleSlideshow = () => setPlaying(!playing());

// ---------- Image loading and prefetch (VIEW-04, VIEW-05) ----------

interface Loaded {
  img: HTMLImageElement;
  ready: Promise<boolean>;
  ok: boolean | null;
}
const cache = new Map<string, Loaded>();

function load(url: string): Loaded {
  let c = cache.get(url);
  if (c) return c;
  const img = new Image();
  img.decoding = "async";
  img.draggable = false;
  img.className = "viewer-img";
  img.alt = "";
  img.src = url;
  const entry: Loaded = { img, ok: null, ready: Promise.resolve(false) };
  // decode() finishes when the image is decoded and ready to paint.
  entry.ready = img.decode().then(
    () => (entry.ok = true),
    () => (entry.ok = false),
  );
  cache.set(url, entry);
  return entry;
}

/** Full image dimensions (after EXIF orientation), by path. */
const dimsCache = new Map<string, { w: number; h: number } | null>();
const dimsPending = new Map<string, Promise<{ w: number; h: number } | null>>();
function fullDims(path: string): Promise<{ w: number; h: number } | null> {
  if (dimsCache.has(path)) return Promise.resolve(dimsCache.get(path)!);
  let p = dimsPending.get(path);
  if (!p) {
    p = api
      .getImageInfo(path)
      .then((i) => (i.width && i.height ? { w: i.width, h: i.height } : null))
      .catch(() => null)
      .then((d) => {
        dimsCache.set(path, d);
        dimsPending.delete(path);
        return d;
      });
    dimsPending.set(path, p);
  }
  return p;
}

/**
 * PERF-10: the viewer first loads a display-size version (a subsampled decode in the Rust core,
 * about the window size in device pixels) and loads the full image only when a zoom needs more
 * pixels. This is much faster than a full 24 MP decode, and it uses less memory for the prefetch.
 */
function displayTarget(): number {
  const px = Math.max(window.innerWidth, window.innerHeight) * window.devicePixelRatio;
  return Math.ceil(px / 256) * 256;
}
const fullUrlOf = (e: Entry) => imageUrl(entryPath(e), e.m);
const displayUrlOf = (e: Entry) => `${fullUrlOf(e)}&w=${displayTarget()}`;

export function ViewerView() {
  let stage!: HTMLDivElement;
  let holder!: HTMLDivElement;
  const current = createMemo(() => {
    const i = viewerIndex();
    return i === null ? undefined : images()[i];
  });
  const [thumb, setThumb] = createSignal<string | null>(null);
  const [shown, setShown] = createSignal<HTMLImageElement | null>(null);

  onMount(() => {
    const ro = new ResizeObserver(() => setStageSize({ w: stage.clientWidth, h: stage.clientHeight }));
    ro.observe(stage);
    onCleanup(() => ro.disconnect());
    stage.focus();
  });

  const show = (img: HTMLImageElement, dims: { w: number; h: number } | null) => {
    holder.replaceChildren(img);
    setNatural(dims ?? { w: img.naturalWidth, h: img.naturalHeight });
    setShown(img);
    setThumb(null);
  };

  createEffect(
    on(current, (e) => {
      if (!e) return;
      const path = entryPath(e);
      const url = displayUrlOf(e);
      setZoom("fit");
      setPan({ x: 0, y: 0 });
      setRotation(0);
      setFailed(false);
      const c = load(url);
      const dims = fullDims(path);
      if (c.ok && dimsCache.has(path)) {
        show(c.img, dimsCache.get(path)!); // PERF-09: prefetched and decoded, shown at once
      } else {
        holder.replaceChildren();
        setShown(null);
        setNatural(null);
        // VIEW-05: the cached thumbnail first.
        setThumb(thumbUrl(path, 512, e.m));
        Promise.all([c.ready, dims]).then(([ok, d]) => {
          if (current() !== e) return;
          if (ok) show(c.img, d);
          else setFailed(true);
        });
      }
      // VIEW-04: prefetch the next 2 and the previous 1 image. The prefetch starts after the
      // current image is decoded, so that it does not compete with the current decode.
      const imgs = images();
      const i = viewerIndex() ?? 0;
      const keep = new Set<string>([url, fullUrlOf(e)]);
      const neighbours: Entry[] = [];
      for (const j of [i + 1, i + 2, i - 1]) {
        const n = imgs[j];
        if (n) {
          keep.add(displayUrlOf(n));
          neighbours.push(n);
        }
      }
      c.ready.then(() => {
        if (current() !== e) return;
        for (const n of neighbours) {
          load(displayUrlOf(n));
          fullDims(entryPath(n));
        }
      });
      for (const k of [...cache.keys()]) if (!keep.has(k)) cache.delete(k);
      if (dimsCache.size > 200) dimsCache.clear();
    }),
  );

  // Load the full image when the zoom needs more pixels than the display-size version has.
  createEffect(() => {
    const n = natural();
    const el = shown();
    const e = current();
    if (!n || !el || !e) return;
    const needed = scale() * n.w * window.devicePixelRatio;
    if (el.naturalWidth >= n.w || el.naturalWidth >= needed * 0.98) return;
    const full = load(fullUrlOf(e));
    full.ready.then((ok) => {
      if (ok && current() === e && shown() === el) {
        holder.replaceChildren(full.img);
        setShown(full.img);
      }
    });
  });

  // Slideshow
  createEffect(() => {
    if (!playing()) return;
    const secs = settings()?.slideshowSeconds ?? 3;
    const t = setInterval(() => {
      const n = images().length;
      const i = viewerIndex() ?? 0;
      go(i + 1 >= n ? 0 : i + 1);
    }, secs * 1000);
    onCleanup(() => clearInterval(t));
  });
  onCleanup(() => {
    setPlaying(false);
    cache.clear();
    dimsCache.clear();
  });

  // ----- Zoom and pan input (VIEW-06, VIEW-08) -----
  const rel = (e: { clientX: number; clientY: number }) => {
    const r = stage.getBoundingClientRect();
    return { x: e.clientX - r.left - r.width / 2, y: e.clientY - r.top - r.height / 2 };
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      // Pinch (ctrl + wheel) or ⌘ + scroll wheel.
      const p = rel(e);
      const factor = Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.003));
      zoomTo(scale() * factor, p.x, p.y);
    } else {
      setPan(clampPan({ x: pan().x - e.deltaX, y: pan().y - e.deltaY }));
    }
  };

  let gestureStart = 1;
  const onGestureStart = (e: Event) => {
    e.preventDefault();
    gestureStart = scale();
  };
  const onGestureChange = (e: Event) => {
    e.preventDefault();
    const ge = e as Event & { scale: number; clientX: number; clientY: number };
    const p = rel(ge);
    zoomTo(gestureStart * ge.scale, p.x, p.y);
  };

  let drag: { x: number; y: number; px: number; py: number } | null = null;
  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    stage.focus();
    drag = { x: e.clientX, y: e.clientY, px: pan().x, py: pan().y };
    stage.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent) => {
    if (!drag) return;
    setPan(clampPan({ x: drag.px + e.clientX - drag.x, y: drag.py + e.clientY - drag.y }));
  };
  const onPointerUp = () => (drag = null);

  onMount(() => {
    stage.addEventListener("gesturestart", onGestureStart);
    stage.addEventListener("gesturechange", onGestureChange);
    stage.addEventListener("wheel", onWheel, { passive: false });
  });

  // Keep the displayed transform in sync.
  createEffect(() => {
    const n = natural();
    const img = shown();
    if (!n || !img) return;
    const p = pan();
    img.style.width = `${n.w}px`;
    img.style.height = `${n.h}px`;
    img.style.transform = `translate(-50%, -50%) translate(${p.x}px, ${p.y}px) rotate(${rotation()}deg) scale(${scale()})`;
  });

  const zoomLabel = () => (natural() ? `${Math.round(scale() * 100)} %` : "");
  const pannable = () => {
    const r = rotatedSize();
    const s = stageSize();
    return !!r && (r.w * scale() > s.w + 1 || r.h * scale() > s.h + 1);
  };

  return (
    <div class="viewer" role="dialog" aria-label="Image viewer">
      <header class="toolbar viewer-toolbar" data-tauri-drag-region>
        <div class="traffic-space" data-tauri-drag-region />
        <button class="tb-btn" title="Back to Grid (Esc)" aria-label="Back to grid" onClick={closeViewer}>
          <Icon d={ICONS.grid} />
        </button>
        <div class="viewer-title" data-tauri-drag-region>
          <span class="viewer-name">{current()?.n}</span>
          <span class="viewer-meta" aria-live="polite">
            {(viewerIndex() ?? 0) + 1} / {images().length}
            <Show when={zoomLabel()}> · {zoomLabel()}</Show>
            <Show when={rotation()}> · {rotation()}°</Show>
          </span>
        </div>
        <div class="tb-spacer" data-tauri-drag-region />
        <button class="tb-btn" title="Rotate Left (⌘L)" aria-label="Rotate left" onClick={() => rotate(-90)}>
          <Icon d={ICONS.rotL} />
        </button>
        <button class="tb-btn" title="Rotate Right (⌘R)" aria-label="Rotate right" onClick={() => rotate(90)}>
          <Icon d={ICONS.rotR} />
        </button>
        <button class="tb-btn text" title="Zoom to Fit (⌘0)" onClick={zoomFit}>
          Fit
        </button>
        <button class="tb-btn text" title="Actual Size (⌘1)" onClick={zoomActual}>
          1:1
        </button>
        <button class="tb-btn" classList={{ on: showFilmstrip() }} title="Film Strip (F)" aria-pressed={showFilmstrip()} aria-label="Film strip" onClick={toggleFilmstrip}>
          <Icon d={ICONS.strip} />
        </button>
        <button class="tb-btn" classList={{ on: playing() }} title="Slideshow (S)" aria-pressed={playing()} aria-label="Slideshow" onClick={toggleSlideshow}>
          <Icon d={playing() ? ICONS.pause : ICONS.play} />
        </button>
        <label class="sort-control" title="Slideshow interval">
          <span class="visually-hidden">Slideshow interval</span>
          <select
            value={settings()?.slideshowSeconds ?? 3}
            onChange={async (e) => setSettings(await api.updateSettings({ slideshowSeconds: Number(e.currentTarget.value) }))}
          >
            {[1, 2, 3, 5, 10, 30].map((s) => (
              <option value={s}>{s} s</option>
            ))}
          </select>
        </label>
        <button class="tb-btn edit-toggle" classList={{ on: editMode() }} title="Edit Mode (⌘E)" aria-pressed={editMode()} aria-label="Edit mode" onClick={toggleEditMode}>
          <Icon d={ICONS.edit} />
        </button>
        <Show when={editMode()}>
          <button class="tb-btn danger" title="Move to Trash (⌘⌫)" aria-label="Move to Trash" onClick={trashViewerImage}>
            <Icon d={ICONS.trash} />
          </button>
        </Show>
        <button class="tb-btn" classList={{ on: infoOpen() }} title="Info (⌘I)" aria-pressed={infoOpen()} aria-label="Info" onClick={() => setInfoOpen(!infoOpen())}>
          <Icon d={ICONS.info} />
        </button>
      </header>
      <div
        class="stage"
        classList={{ pannable: pannable() }}
        ref={stage}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDblClick={(e) => {
          if (zoom() === "fit" && fitScale() < 1) {
            const p = rel(e);
            zoomTo(1, p.x, p.y);
          } else zoomFit();
        }}
      >
        <Show when={thumb()}>{(t) => <img class="viewer-thumb" src={t()} alt="" draggable={false} />}</Show>
        <div class="viewer-holder" ref={holder} />
        <Show when={failed()}>
          <div class="viewer-error" role="alert">
            This image could not be displayed.
          </div>
        </Show>
      </div>
      <Show when={showFilmstrip()}>
        <Filmstrip />
      </Show>
    </div>
  );
}

const STRIP_ITEM = 72;

/** VIEW-12: film strip of thumbnails. Only the items near the current image are rendered. */
function Filmstrip() {
  const range = createMemo(() => {
    const i = viewerIndex() ?? 0;
    const n = images().length;
    const lo = Math.max(0, i - 30);
    const hi = Math.min(n, i + 31);
    return { lo, hi, items: images().slice(lo, hi) };
  });
  const reduce = prefersReducedMotion();
  return (
    <div class="filmstrip" role="listbox" aria-label="Film strip">
      <div
        class="filmstrip-track"
        style={{
          transform: `translateX(${-((viewerIndex() ?? 0) * STRIP_ITEM + STRIP_ITEM / 2)}px)`,
          transition: reduce ? "none" : undefined,
        }}
      >
        <For each={range().items}>
          {(e, k) => {
            const idx = () => range().lo + k();
            return (
              <button
                class="strip-item"
                classList={{ current: idx() === viewerIndex() }}
                role="option"
                aria-selected={idx() === viewerIndex()}
                aria-label={e.n}
                tabIndex={-1}
                style={{ left: `${idx() * STRIP_ITEM}px` }}
                onClick={() => go(idx())}
              >
                <img src={thumbUrl(entryPath(e), 256, e.m)} alt="" decoding="async" draggable={false} />
              </button>
            );
          }}
        </For>
      </div>
    </div>
  );
}
