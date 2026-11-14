// Locations (D5): what a window shows. A disk folder or an in-memory collection, each behind a
// `LocationProvider`. Navigation, the grid, and the action registry use only these interfaces.
import { createSignal } from "solid-js";
import { api, baseName, CollectionSummary, DirListing, GroupKey, SortSpec } from "./api";
import { collectionEmptyText } from "./badges";

export type DiskLocation = { kind: "disk"; path: string };
export type CollectionLocation = { kind: "collection"; id: string };
export type Location = DiskLocation | CollectionLocation;

export interface LocationCaps {
  /** Enclosing Folder (⌘↑) is possible. */
  parent: boolean;
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
}

export const NO_CAPS: LocationCaps = {
  parent: false,
  favorites: false,
  recents: false,
  folderPrefs: false,
  folderTiles: false,
  watch: false,
  scan: false,
};

const DISK_CAPS: LocationCaps = {
  parent: true,
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
};

export const CollectionProvider: LocationProvider<CollectionLocation> = {
  list: (loc, sort, _showHidden, group) => api.listCollection(loc.id, sort, group),
  title: (loc) => collectionName(loc.id),
  crumbs: (loc) => [
    { label: "Collections", title: "Collections", target: null },
    { label: collectionName(loc.id), title: collectionName(loc.id), target: loc },
  ],
  caps: () => NO_CAPS,
  emptyText: (loc) => collectionEmptyText(loc.id),
};

export function providerFor(loc: Location): LocationProvider<Location> {
  const p = loc.kind === "disk" ? DiskProvider : CollectionProvider;
  return p as LocationProvider<Location>;
}

const COLLECTION_PREFIX = "collection:";

/** Stable string: the disk path, or "collection:<id>". */
export function locationKey(loc: Location): string {
  return loc.kind === "disk" ? loc.path : COLLECTION_PREFIX + loc.id;
}

/** Inverse of `locationKey`, for history and window restore. */
export function parseLocation(key: string): Location {
  return key.startsWith(COLLECTION_PREFIX)
    ? { kind: "collection", id: key.slice(COLLECTION_PREFIX.length) }
    : { kind: "disk", path: key };
}

export function sameLocation(a: Location | null, b: Location | null): boolean {
  return a === b || (!!a && !!b && locationKey(a) === locationKey(b));
}
