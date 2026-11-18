// Locations (D5): what a window shows. A disk folder, an in-memory collection, or a flattened disk
// folder, each behind a `LocationProvider`. Navigation, the grid, and the action registry use only
// these interfaces.
import { createSignal } from "solid-js";
import { api, baseName, CollectionSummary, DirListing, Entry, GroupKey, parentPath, SortSpec } from "./api";
import { collectionEmptyText } from "./badges";
import { flat, listFlat } from "./flatten";

export type DiskLocation = { kind: "disk"; path: string };
export type CollectionLocation = { kind: "collection"; id: string };
/** A flattened disk folder: the photos of its tree in 1 grid (FLAT-03); key "flat:<path>". */
export type FlatLocation = { kind: "flat"; path: string };
export type Location = DiskLocation | CollectionLocation | FlatLocation;

export interface LocationCaps {
  /** The location can be a favorite. */
  favorites: boolean;
  /** The location goes to Recents and lastFolder. */
  recents: boolean;
  /** Sort and tile size are stored per location. */
  folderPrefs: boolean;
  /** The listing can contain folders. */
  folderTiles: boolean;
  /** FsWatcher watches the location. */
  watch: boolean;
  /** A scan can start from the location. */
  scan: boolean;
}

export interface Crumb {
  label: string;
  /** Tooltip. */
  title: string;
  /** Null for a crumb that only groups (for example "Collections"). */
  target: Location | null;
}

export interface LocationProvider<L extends Location> {
  /** Entries in grid order, with the groups of `group`. */
  list(loc: L, sort: SortSpec, showHidden: boolean, group: GroupKey): Promise<DirListing>;
  /** Window title. */
  title(loc: L): string;
  /** Breadcrumb components, each with a target Location. */
  crumbs(loc: L): Crumb[];
  /** Capabilities for actions and views. */
  caps(loc: L): LocationCaps;
  /** Text of the grid when the listing is empty. */
  emptyText(loc: L): string;
  /** Target of Enclosing Folder (⌘↑) and the entry key to select there, or null. */
  parent(loc: L): { to: Location; select?: string } | null;
  /** The sort of the location before the user changes it; absent: the default sort of Settings. */
  defaultSort?(loc: L): SortSpec;
}

export const NO_CAPS: LocationCaps = {
  favorites: false,
  recents: false,
  folderPrefs: false,
  folderTiles: false,
  watch: false,
  scan: false,
};

const DISK_CAPS: LocationCaps = {
  favorites: true,
  recents: true,
  folderPrefs: true,
  folderTiles: true,
  watch: true,
  scan: true,
};

/** The collection list of the app, from `list_collections` and `collections-changed`. */
export const [collections, setCollections] = createSignal<CollectionSummary[]>([]);

export function collectionName(id: string): string {
  return collections().find((c) => c.id === id)?.name ?? "Collection";
}

/** The folder the collection came from, or null. */
export function collectionRoot(id: string): string | null {
  return collections().find((c) => c.id === id)?.root ?? null;
}

export const DiskProvider: LocationProvider<DiskLocation> = {
  list: (loc, sort, showHidden, group) => api.listDir(loc.path, sort, showHidden, group),
  title: (loc) => baseName(loc.path) || "Fast Gallery",
  crumbs(loc) {
    const segs = loc.path.split("/").filter(Boolean);
    const out: Crumb[] = [{ label: "Computer", title: "/", target: { kind: "disk", path: "/" } }];
    let acc = "";
    for (const s of segs) {
      acc += `/${s}`;
      out.push({ label: s, title: acc, target: { kind: "disk", path: acc } });
    }
    // Show "/Volumes/X" as one component.
    if (out.length > 2 && out[1].label === "Volumes") out.splice(0, 2);
    return out;
  },
  caps: () => DISK_CAPS,
  emptyText: () => "This folder has no images or folders.",
  parent(loc) {
    if (loc.path === "/") return null;
    const p = parentPath(loc.path);
    return p ? { to: { kind: "disk", path: p }, select: baseName(loc.path) } : null;
  },
};

export const CollectionProvider: LocationProvider<CollectionLocation> = {
  list: (loc, sort, _showHidden, group) => api.listCollection(loc.id, sort, group),
  title: (loc) => collectionName(loc.id),
  // A collection from a scan shows the path of its root, then "[name]", so the user can step back.
  crumbs(loc) {
    const name = collectionName(loc.id);
    const root = collectionRoot(loc.id);
    if (!root) return [{ label: "Collections", title: "Collections", target: null }, { label: name, title: name, target: loc }];
    return [...DiskProvider.crumbs({ kind: "disk", path: root }), { label: `[${name}]`, title: `${name} in ${root}`, target: loc }];
  },
  caps: () => NO_CAPS,
  emptyText: (loc) => collectionEmptyText(loc.id),
  parent(loc) {
    const root = collectionRoot(loc.id);
    return root ? { to: { kind: "disk", path: root } } : null;
  },
};

const FLAT_CAPS: LocationCaps = { ...NO_CAPS, recents: true, folderPrefs: true, watch: true };

/** "Photos (Flattened)" (FLAT-15). */
export const flatTitle = (path: string) => `${baseName(path)} (Flattened)`;

export const FlatProvider: LocationProvider<FlatLocation> = {
  list: (loc, sort, _showHidden, group) => listFlat(loc.path, sort, group),
  title: (loc) => flatTitle(loc.path),
  crumbs: (loc) => [
    ...DiskProvider.crumbs({ kind: "disk", path: loc.path }),
    { label: "[Flattened]", title: `${loc.path} and its subfolders`, target: loc },
  ],
  caps: () => FLAT_CAPS,
  emptyText: (loc) => {
    const s = flat();
    return s && s.root === loc.path && (s.done || s.capped) ? "No photos in this folder or its subfolders" : "Looking for photos…";
  },
  parent: (loc) => ({ to: { kind: "disk", path: loc.path } }),
  // FLAT-16: the newest photos of the tree first.
  defaultSort: () => ({ key: "modified", desc: true }),
};

export function providerFor(loc: Location): LocationProvider<Location> {
  const p = loc.kind === "disk" ? DiskProvider : loc.kind === "flat" ? FlatProvider : CollectionProvider;
  return p as LocationProvider<Location>;
}

const COLLECTION_PREFIX = "collection:";
const FLAT_PREFIX = "flat:";

/** Stable string: the disk path, "collection:<id>", or "flat:<path>". */
export function locationKey(loc: Location): string {
  if (loc.kind === "disk") return loc.path;
  return loc.kind === "flat" ? FLAT_PREFIX + loc.path : COLLECTION_PREFIX + loc.id;
}

/** Inverse of `locationKey`, for history, Recents, and window restore. */
export function parseLocation(key: string): Location {
  if (key.startsWith(COLLECTION_PREFIX)) return { kind: "collection", id: key.slice(COLLECTION_PREFIX.length) };
  if (key.startsWith(FLAT_PREFIX)) return { kind: "flat", path: key.slice(FLAT_PREFIX.length) };
  return { kind: "disk", path: key };
}

/** The sidebar text of a Recents key: the folder name, or "<name> (Flattened)" (FLAT-15). */
export function recentLabel(key: string): string {
  const l = parseLocation(key);
  return l.kind === "flat" ? flatTitle(l.path) : l.kind === "disk" ? baseName(l.path) : key;
}

/**
 * The folder of a photo in a flat location, relative to the flattened root; the root name for a
 * photo directly in it (FLAT-11). Null outside a flat location.
 */
export function flatFolder(loc: Location | null, e: Entry | string): string | null {
  if (loc?.kind !== "flat") return null;
  const p = typeof e === "string" ? e : e.p;
  const dir = p ? parentPath(p) : null;
  if (!dir) return null;
  if (dir === loc.path) return baseName(loc.path);
  const prefix = loc.path === "/" ? "/" : `${loc.path}/`;
  return dir.startsWith(prefix) ? dir.slice(prefix.length) : dir;
}

export function sameLocation(a: Location | null, b: Location | null): boolean {
  return a === b || (!!a && !!b && locationKey(a) === locationKey(b));
}
