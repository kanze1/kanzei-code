import assert from "node:assert/strict";
import { validateArchitecture, checkRepository } from "./architecture-boundaries.mjs";
import path from "node:path";

const packages = [
  { name: "kanzei-tools", dependencies: [{ name: "kanzei-memory", kind: null }] },
  { name: "kanzei-core", dependencies: [] },
  { name: "kanzei-app", dependencies: [{ name: "kanzei-memory", kind: null }] },
];
const good = new Map([["kanzei-core", "mod store;"], ["kanzei-app", "use kanzei_memory::memory::MemoryStore;"]]);
validateArchitecture(packages, good);
for (const [crate, source] of [
  ["kanzei-tools", "pub use kanzei_memory::memory;"],
  ["kanzei-tools", "pub fn build_harness() {}"],
  ["kanzei-core", "pub mod store;"],
  ["kanzei-app", "use kanzei_tools::memory::MemoryStore;"],
  ["kanzei-app", "use kanzei_core::store::SessionStore;"],
  ["kanzei-app", 'fn optional_test() { eprintln!("跳过:missing dependency"); return; }'],
]) assert.throws(() => validateArchitecture(packages, new Map([...good, [crate, source]])));
assert.throws(() => validateArchitecture([{ name: "kanzei-tools", dependencies: [{ name: "kanzei-runtime", kind: null }] }], new Map()));
assert.throws(() => validateArchitecture([{ name: "kanzei-app", dependencies: [] }], good));
checkRepository(path.resolve(import.meta.dirname, ".."));
console.log("Architecture regression fixtures reject each forbidden dependency and facade.");
