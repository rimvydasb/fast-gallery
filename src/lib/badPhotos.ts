// The bad photo scan in the frontend: start, and the result after `task-done` (QUAL-12, QUAL-13).
import { createSignal } from "solid-js";
import { api, baseName, errorMessage, ScanResult, TaskDone } from "./api";
import { plural } from "./format";
import { locationKey } from "./locations";
import { location, navigate } from "./navigation";
import { viewerIndex } from "./store";
import { setTask } from "./tasks";
import { toast } from "./toast";

export interface ScanCapInfo {
  cap: number;
  rootName: string;
  found: number;
  collectionId: string;
  /** True if the window did not open the virtual folder. */
  showOpen: boolean;
}

/** The ScanCapDialog content, or null. */
export const [scanCapInfo, setScanCapInfo] = createSignal<ScanCapInfo | null>(null);

/** The location of this window when it started the scan. */
let origin: string | null = null;

/** Starts the scan at once, with no dialog (QUAL-08). */
export async function startBadPhotoScan(root: string) {
  const l = location();
  origin = l ? locationKey(l) : null;
  try {
    setTask(await api.startScanBadPhotos(root));
  } catch (e) {
    origin = null;
    toast(errorMessage(e), "error");
  }
}

export function foundText(r: ScanResult): string {
  return `Found ${plural(r.found, "bad photo", "bad photos")} in ${plural(r.scanned, "photo", "photos")}.`;
}

/** Called in each window for each `task-done`; only the source window acts. */
export function onBadPhotoScanDone(d: TaskDone, windowLabel: string) {
  if (d.kind !== "scanBadPhotos" || d.window !== windowLabel) return;
  const o = origin;
  origin = null;
  if (d.outcome === "cancelled") {
    toast("The scan was cancelled.", "info");
    return;
  }
  if (d.outcome === "failed") {
    const err = (d.result as { error?: string } | null)?.error;
    toast(err ? `The scan failed: ${err}` : "The scan failed.", "error");
    return;
  }
  const r = d.result as ScanResult;
  const l = location();
  // The viewer counts as "not at the origin location", so the app does not close it.
  const atOrigin = o !== null && !!l && locationKey(l) === o && viewerIndex() === null;
  const info: ScanCapInfo = {
    cap: r.scanned,
    rootName: d.root ? baseName(d.root) : "",
    found: r.found,
    collectionId: r.collectionId,
    showOpen: !atOrigin,
  };
  if (atOrigin) navigate({ kind: "collection", id: r.collectionId });
  if (r.capped) setScanCapInfo(info);
  else if (atOrigin) toast(foundText(r), "info");
  else toast(foundText(r), "info", { label: "Open", actionId: "nav.openCollection", args: r.collectionId });
}
