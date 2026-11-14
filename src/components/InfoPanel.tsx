import { createEffect, createResource, createSignal, onCleanup, Show } from "solid-js";
import { api, baseName, ImageInfo, QualityInfo } from "../lib/api";
import { badgeLabels } from "../lib/badges";
import { formatBytes, formatDate, formatExposure } from "../lib/format";

function Row(props: { label: string; value?: string | number | null }) {
  return (
    <Show when={props.value !== undefined && props.value !== null && props.value !== ""}>
      <dt>{props.label}</dt>
      <dd>{props.value}</dd>
    </Show>
  );
}

const FORMAT_NAMES: Record<string, string> = {
  "public.jpeg": "JPEG",
  "public.png": "PNG",
  "public.heic": "HEIC",
  "public.heif": "HEIF",
  "org.webmproject.webp": "WebP",
  "com.compuserve.gif": "GIF",
  "public.tiff": "TIFF",
  "com.microsoft.bmp": "BMP",
  "public.avif": "AVIF",
  "public.svg-image": "SVG",
};

/** The image path, after the selection has been still for a moment, so that a fast walk through the grid does not start an analysis for each image. */
function settled(src: () => string | null, ms = 200) {
  const [v, setV] = createSignal<string | null>(null);
  createEffect(() => {
    const p = src();
    const t = setTimeout(() => setV(p), ms);
    onCleanup(() => clearTimeout(t));
  });
  return v;
}

const fmt = (n: number, digits = 0) => n.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits });
const pct = (f: number) => `${fmt(f * 100, 1)} %`;

/** INFO-01..INFO-04: file and EXIF information for one image. QUAL-23: its quality metrics. */
export function InfoPanel(props: { target: { path: string; isDir: boolean } | null; count: number }) {
  const imagePath = () => (props.target && !props.target.isDir ? props.target.path : null);
  const [info] = createResource(imagePath, (p) => api.getImageInfo(p).catch(() => null as ImageInfo | null));
  const qualityPath = settled(imagePath);
  const [quality] = createResource(qualityPath, (p) => api.getQuality(p).catch(() => null as QualityInfo | null));
  return (
    <aside class="info-panel" aria-label="Information">
      <Show
        when={props.target}
        fallback={<p class="info-empty">{props.count > 1 ? `${props.count} items selected` : "No selection"}</p>}
      >
        {(t) => (
          <Show
            when={!t().isDir}
            fallback={
              <dl>
                <Row label="Name" value={baseName(t().path)} />
                <Row label="Kind" value="Folder" />
                <Row label="Path" value={t().path} />
              </dl>
            }
          >
            <Show when={info()} fallback={<p class="info-empty">Loading…</p>}>
              {(i) => (
                <dl>
                  <Row label="Name" value={i().name} />
                  <Row label="Path" value={i().path} />
                  <Row label="Size" value={formatBytes(i().size)} />
                  <Row label="Dimensions" value={i().width && i().height ? `${i().width} × ${i().height}` : undefined} />
                  <Row label="Format" value={i().format ? FORMAT_NAMES[i().format!] ?? i().format : undefined} />
                  <Row label="Modified" value={formatDate(i().modified)} />
                  <Row label="Date Taken" value={i().dateTaken?.replace(/^(\d{4}):(\d{2}):(\d{2})/, "$1-$2-$3")} />
                  <Row label="Camera" value={i().camera} />
                  <Row label="Lens" value={i().lens} />
                  <Row label="Focal Length" value={i().focalLength ? `${Math.round(i().focalLength! * 10) / 10} mm` : undefined} />
                  <Row label="Aperture" value={i().aperture ? `ƒ/${i().aperture}` : undefined} />
                  <Row label="Shutter" value={i().exposureTime ? formatExposure(i().exposureTime!) : undefined} />
                  <Row label="ISO" value={i().iso} />
                  <Row label="GPS" value={i().gps ? `${i().gps![0].toFixed(5)}, ${i().gps![1].toFixed(5)}` : undefined} />
                  <Show when={qualityPath() === i().path && quality()}>
                    {(q) => (
                      <>
                        <dt class="info-section">Quality</dt>
                        <dd class="info-section" />
                        <Row
                          label="Assessment"
                          value={q().flags.length ? badgeLabels(q().flags.map((f) => `quality.${f}`)) : "No problems found"}
                        />
                        <Row label="Sharpness" value={`${fmt(q().metrics.lapVarLocal)} (local), ${fmt(q().metrics.lapVar)} (global)`} />
                        <Row label="Tenengrad" value={fmt(q().metrics.tenengrad)} />
                        <Row label="Brenner" value={fmt(q().metrics.brenner)} />
                        <Row label="Brightness" value={`${fmt(q().metrics.meanLuma)} / 255`} />
                        <Row label="Contrast" value={fmt(q().metrics.stdLuma, 1)} />
                        <Row label="Clipping" value={`${pct(q().metrics.clipDark)} dark, ${pct(q().metrics.clipBright)} bright`} />
                      </>
                    )}
                  </Show>
                </dl>
              )}
            </Show>
          </Show>
        )}
      </Show>
    </aside>
  );
}
