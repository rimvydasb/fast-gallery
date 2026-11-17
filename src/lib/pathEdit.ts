// The path field of the breadcrumb bar: the user can copy the current path or type another path
// and go there at once. A bad path shows an error, and the bar goes back to the current location.
import { createSignal } from "solid-js";
import { api, errorMessage } from "./api";
import { collectionName, collectionRoot, Location } from "./locations";
import { location, navigate, openPath } from "./navigation";
import { toast } from "./toast";

/** True while the breadcrumb bar shows the path field. */
export const [pathEditing, setPathEditing] = createSignal(false);

/** The text of the path field for a location. A virtual folder shows "[name]" after its root. */
export function pathText(loc: Location | null): string {
  if (!loc) return "";
  if (loc.kind === "disk") return loc.path;
  const root = collectionRoot(loc.id);
  const tail = `[${collectionName(loc.id)}]`;
  return root ? `${root === "/" ? "" : root}/${tail}` : `Collections/${tail}`;
}

/** Resolves "." and "..", removes repeated and trailing slashes. The input must be absolute. */
export function normalizePath(p: string): string {
  const out: string[] = [];
  for (const s of p.split("/")) {
    if (s === "" || s === ".") continue;
    if (s === "..") out.pop();
    else out.push(s);
  }
  return "/" + out.join("/");
}

/** Removes white space and the quotes of a pasted "quoted path". */
function cleanInput(text: string): string {
  const t = text.trim();
  const q = t.match(/^(["'])(.*)\1$/);
  return q ? q[2].trim() : t;
}

/**
 * Goes to the typed path: a folder opens, a file opens in its folder in the viewer.
 * "~" is the home folder, and a relative path starts at the current folder.
 * Returns true if the window went to the path, or if the text is the current location.
 */
export async function submitPath(text: string): Promise<boolean> {
  const loc = location();
  const t = cleanInput(text);
  if (t === "" || t === pathText(loc)) return true;
  let abs: string;
  if (t === "~" || t.startsWith("~/")) {
    abs = (await api.homeDir()) + t.slice(1);
  } else if (t.startsWith("/")) {
    abs = t;
  } else if (loc?.kind === "disk") {
    abs = `${loc.path}/${t}`;
  } else {
    toast(`“${t}” is not a full path. A full path starts with “/” or “~”.`, "error");
    return false;
  }
  const p = normalizePath(abs);
  if (loc?.kind === "disk" && loc.path === p) return true;
  let kind: "dir" | "file" | "missing";
  try {
    kind = await api.pathKind(p);
  } catch (e) {
    toast(`Cannot open “${p}”: ${errorMessage(e)}`, "error");
    return false;
  }
  if (kind === "missing") {
    toast(`“${p}” does not exist.`, "error");
    return false;
  }
  if (kind === "dir") await navigate(p);
  else await openPath(p);
  return true;
}
