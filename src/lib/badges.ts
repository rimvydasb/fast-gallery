// Badge codes of collection items to labels, and the texts of feature collections. The Rust
// `CollectionStore` knows only the codes; each feature adds its codes here.

export interface BadgeInfo {
  label: string;
  /** "warn" for a problem, "info" for a neutral mark. */
  tone: "warn" | "info";
}

const BADGES: Record<string, BadgeInfo> = {
  "quality.blurry": { label: "Blurry", tone: "warn" },
  "quality.underexposed": { label: "Underexposed", tone: "warn" },
  "quality.overexposed": { label: "Overexposed", tone: "warn" },
  "quality.lowContrast": { label: "Low Contrast", tone: "warn" },
  "quality.screenshot": { label: "Screenshot", tone: "info" },
};

export function badgeInfo(code: string): BadgeInfo {
  return BADGES[code] ?? { label: code.slice(code.lastIndexOf(".") + 1), tone: "info" };
}

/** "Blurry, Underexposed" */
export function badgeLabels(codes: readonly string[] | undefined): string {
  return (codes ?? []).map((c) => badgeInfo(c).label).join(", ");
}

const EMPTY_TEXT: Record<string, string> = {
  "bad-photos": "No bad photos found.",
};

export function collectionEmptyText(id: string): string {
  return EMPTY_TEXT[id] ?? "This collection is empty.";
}
