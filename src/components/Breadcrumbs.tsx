import { For, Show } from "solid-js";
import { navigate, path } from "../lib/store";

/** NAV-03, NAV-04: a breadcrumb bar with each component of the current path. */
export function Breadcrumbs() {
  const parts = () => {
    const p = path();
    if (!p) return [];
    const segs = p.split("/").filter(Boolean);
    const out = [{ label: "/", target: "/" }];
    let acc = "";
    for (const s of segs) {
      acc += `/${s}`;
      out.push({ label: s, target: acc });
    }
    // Show "/Volumes/X" as one component.
    if (out.length > 2 && out[1].label === "Volumes") out.splice(0, 2);
    return out;
  };
  return (
    <nav class="breadcrumbs" aria-label="Path">
      <For each={parts()}>
        {(c, i) => (
          <>
            <Show when={i() > 0}>
              <span class="crumb-sep" aria-hidden="true">
                ›
              </span>
            </Show>
            <button
              class="crumb"
              classList={{ last: i() === parts().length - 1 }}
              aria-current={i() === parts().length - 1 ? "location" : undefined}
              title={c.target}
              onClick={() => navigate(c.target)}
            >
              {c.label === "/" ? "Computer" : c.label}
            </button>
          </>
        )}
      </For>
    </nav>
  );
}
