import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";

const directory = dirname(fileURLToPath(import.meta.url));
it("all stylesheet variables resolve in the design system or a local declaration", () => {
  const source = readdirSync(directory)
    .filter((name) => name.endsWith(".css"))
    .map((name) => readFileSync(join(directory, name), "utf8"))
    .join("\n");
  const declared = new Set(Array.from(source.matchAll(/(--[\w-]+)\s*:/g), (m) => m[1]));
  const referenced = Array.from(source.matchAll(/var\(\s*(--[\w-]+)/g), (m) => m[1]);
  expect(source).toContain("color-scheme: light");
  expect([...new Set(referenced.filter((name) => !declared.has(name)))]).toEqual([]);
});
