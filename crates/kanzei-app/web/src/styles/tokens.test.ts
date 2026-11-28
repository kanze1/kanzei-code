import { expect, it } from "vitest";
import styles from "./tokens.css?raw";

const sheets = import.meta.glob("./*.css", { eager: true, query: "?raw", import: "default" });

it("all stylesheet variables resolve in the design system or a local declaration", () => {
  const source = Object.values(sheets).join("\n");
  const declared = new Set(Array.from(source.matchAll(/(--[\w-]+)\s*:/g), m => m[1]));
  const referenced = Array.from(source.matchAll(/var\(\s*(--[\w-]+)/g), m => m[1]);
  expect(styles).toContain("color-scheme: light");
  expect([...new Set(referenced.filter(name => !declared.has(name)))]).toEqual([]);
});
