// Rules for a new file name. They match `validate_file_name` in the Rust core, which is the
// authority; this copy shows the reason in the dialog before the request is sent.

const MAX_NAME_BYTES = 255;

/** Returns the reason why the name is not valid, or null. */
export function validateFileName(name: string): string | null {
  if (name.trim() === "") return "The name cannot be empty.";
  if (name === "." || name === "..") return `“${name}” is not a valid name.`;
  if (/[/:\0]/.test(name)) return "The name cannot contain “/” or “:”.";
  if (new TextEncoder().encode(name).length > MAX_NAME_BYTES) return "The name is too long.";
  return null;
}

/** The length of the name without its extension, so that the dialog selects only that part. */
export function stemLength(name: string): number {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? dot : name.length;
}
