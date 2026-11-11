// Typed wrappers for the Tauri commands and the image protocols.
import { invoke } from "@tauri-apps/api/core";

export type SortKey = "name" | "modified" | "taken" | "size";
export interface SortSpec {
  key: SortKey;
  desc: boolean;
}

/** A directory entry: n = name, d = folder, s = size, m = mtime ms, t = date taken s. */
export interface Entry {
  n: string;
  d: boolean;
  s: number;
  m: number;
  t?: number;
}

export interface DirListing {
  path: string;
  entries: Entry[];
}

export interface PreviewImage {
  p: string;
  m: number;
}

export interface FolderPreview {
  folder: string;
  sort: string;
  images: PreviewImage[];
  /** Images seen in the folder tree; a lower bound when `more` is true. */
  count: number;
  more?: boolean;
}

export interface FolderPrefs {
  sort: SortSpec;
  tileSize: number;
}

export interface Settings {
  defaultSort: SortSpec;
  defaultTileSize: number;
  showHidden: boolean;
  cacheLimitMb: number;
  previewDepth: number;
  slideshowSeconds: number;
  showFilmstrip: boolean;
  sidebarCollapsed: boolean;
  favorites: string[];
  recents: string[];
  lastFolder: string | null;
  folderPrefs: Record<string, FolderPrefs>;
}

export interface CmdError {
  kind: "permission" | "notFound" | "notDir" | "editOff" | "other";
  message: string;
}

export interface ImageInfo {
  name: string;
  path: string;
  size: number;
  modified: number;
  width?: number;
  height?: number;
  format?: string;
  camera?: string;
  lens?: string;
  focalLength?: number;
  aperture?: number;
  exposureTime?: number;
  iso?: number;
  dateTaken?: string;
  gps?: [number, number];
}

export interface FsChange {
  dir: string;
  dirChanged: boolean;
  folders: string[];
  files: string[];
}

export interface TrashResult {
  trashed: string[];
  failed: { path: string; error: string }[];
}

export interface AppInfo {
  name: string;
  path: string;
  default: boolean;
}

export function isCmdError(e: unknown): e is CmdError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isCmdError(e)) return e.message;
  return String(e);
}

export const sortKeyString = (s: SortSpec) => `${s.key}:${s.desc ? "desc" : "asc"}`;

export const api = {
  listDir: (path: string, sort: SortSpec, showHidden: boolean) =>
    invoke<DirListing>("list_dir", { path, sort, showHidden }),
  getFolderPreviews: (folders: string[], sort: SortSpec) =>
    invoke<FolderPreview[]>("get_folder_previews", { folders, sort }),
  refreshFolderPreviews: (folders: string[], sort: SortSpec) =>
    invoke<void>("refresh_folder_previews", { folders, sort }),
  getImageInfo: (path: string) => invoke<ImageInfo>("get_image_info", { path }),
  listVolumes: () => invoke<{ name: string; path: string }[]>("list_volumes"),
  pathKind: (path: string) => invoke<"dir" | "file" | "missing">("path_kind", { path }),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>("update_settings", { patch }),
  getCacheInfo: () => invoke<{ used: number; limit: number; path: string }>("get_cache_info"),
  clearCache: () => invoke<void>("clear_cache"),
  setEditMode: (on: boolean) => invoke<boolean>("set_edit_mode", { on }),
  trashFiles: (paths: string[]) => invoke<TrashResult>("trash_files", { paths }),
  undoTrash: () => invoke<string[]>("undo_trash"),
  copyFiles: (paths: string[]) => invoke<boolean>("copy_files", { paths }),
  revealInFinder: (paths: string[]) => invoke<void>("reveal_in_finder", { paths }),
  openWithApps: (path: string) => invoke<AppInfo[]>("open_with_apps", { path }),
  openWith: (path: string, app: string | null) => invoke<void>("open_with", { path, app }),
  setDesktopPicture: (path: string) => invoke<void>("set_desktop_picture", { path }),
  openPrivacySettings: () => invoke<void>("open_privacy_settings"),
  startDrag: (paths: string[]) => invoke<void>("start_drag", { paths }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  newWindow: () => invoke<void>("new_window"),
  openSettings: () => invoke<void>("open_settings"),
  takePendingOpen: () => invoke<string | null>("take_pending_open"),
  toggleFavorite: (path: string) => invoke<Settings>("toggle_favorite", { path }),
  setFolderPrefs: (path: string, prefs: FolderPrefs) => invoke<void>("set_folder_prefs", { path, prefs }),
  homeDir: () => invoke<string>("home_dir"),
};

/** thumb:// URL. The mtime in the URL makes a changed file get a new URL. */
export function thumbUrl(path: string, size: 256 | 512, mtime: number): string {
  return `thumb://localhost/${encodeURIComponent(path)}?s=${size}&v=${mtime}`;
}

/** image:// URL for the full image. */
export function imageUrl(path: string, mtime: number): string {
  return `image://localhost/${encodeURIComponent(path)}?v=${mtime}`;
}

export function joinPath(dir: string, name: string): string {
  return dir.endsWith("/") ? dir + name : `${dir}/${name}`;
}

export function parentPath(p: string): string | null {
  if (p === "/") return null;
  const i = p.lastIndexOf("/");
  return i <= 0 ? "/" : p.slice(0, i);
}

export function baseName(p: string): string {
  if (p === "/") return "/";
  return p.slice(p.lastIndexOf("/") + 1);
}
