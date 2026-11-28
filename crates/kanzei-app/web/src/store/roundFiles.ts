const WRITE_TOOL_NAMES = new Set([
  "edit",
  "write",
  "insert",
  "delete",
  "remove",
  "rename",
  "move",
  "apply_patch",
  "patch",
  "multi_edit",
  "create_file",
  "replace_file",
]);

export function normalizeRelPath(raw: string): string {
  return raw.trim().replace(/\\/g, "/").replace(/^\.\//, "");
}

export function modifiedPaths(name: string, input: unknown): string[] {
  if (!WRITE_TOOL_NAMES.has(name.toLowerCase()) || !input || typeof input !== "object") return [];
  return Object.entries(input)
    .filter(
      ([key, value]) =>
        ["path", "file_path", "target_path", "target_file"].includes(key) &&
        typeof value === "string" &&
        value.trim(),
    )
    .map(([, value]) => normalizeRelPath(value));
}
