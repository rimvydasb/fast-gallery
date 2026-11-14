export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

export function formatDate(ms: number): string {
  return dateFmt.format(new Date(ms));
}

const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric", month: "long", year: "numeric" });

/** The header text of a group: a date group has an ISO date as its label. */
export function groupLabel(g: { label: string; kind?: string }): string {
  if (g.kind !== "date") return g.label;
  const [y, m, d] = g.label.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  if (Number.isNaN(date.getTime())) return g.label;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = Math.round((today.getTime() - date.getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return dayFmt.format(date);
}

export function formatExposure(s: number): string {
  if (s >= 1) return `${s} s`;
  return `1/${Math.round(1 / s)} s`;
}

/** "12 images", or "124+ images" when the count is a lower bound. */
export function imageCount(n: number, more = false): string {
  return more ? `${n.toLocaleString()}+ images` : plural(n, "image", "images");
}

export function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}
