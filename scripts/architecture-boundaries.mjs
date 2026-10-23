import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export function validateArchitecture(packages, sources) {
  const entrypoints = new Set(["kanzei", "kanzei-app"]);
  for (const crate of packages) {
    const dependencies = crate.dependencies.filter(dependency => dependency.kind !== "dev").map(dependency => dependency.name);
    if (crate.name !== "kanzei-runtime" && !entrypoints.has(crate.name)) {
      assert(!dependencies.includes("kanzei-runtime"), `${crate.name} must not depend on the composition root`);
    }
    const source = sources.get(crate.name) ?? "";
    assert(!/eprintln!\s*\(\s*"跳过:/.test(source), `${crate.name} must report optional tests as ignored instead of returning a false pass`);
    assert(!/kanzei_tools::(?:memory|docstore|embed|replay_eval)\b/.test(source), `${crate.name} consumes a memory facade`);
    assert(!/kanzei_core::store\b/.test(source), `${crate.name} consumes private storage implementation`);
    if (crate.name !== "kanzei-memory" && /kanzei_memory::/.test(source)) assert(dependencies.includes("kanzei-memory"), `${crate.name} must declare its memory dependency`);
  }
  const tools = sources.get("kanzei-tools") ?? "";
  assert(!/pub\s+use\s+kanzei_memory::(?:memory|docstore|embed|replay_eval)\s*;/.test(tools), "tools must not reexport the memory capability");
  assert(!/pub\s+(?:async\s+)?fn\s+build_(?:harness|runner_config|subagent_runtime)\b/.test(tools), "tools must not own runtime composition");
  assert(!/pub\s+mod\s+store\s*;/.test(sources.get("kanzei-core") ?? ""), "core storage implementation must be private");
}

function readSources(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const filename = path.join(directory, entry.name);
    return entry.isDirectory() ? readSources(filename) : filename.endsWith(".rs") ? [fs.readFileSync(filename, "utf8")] : [];
  }).join("\n");
}

export function checkRepository(root) {
  const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd: root, encoding: "utf8" }));
  const packages = metadata.packages.filter(crate => metadata.workspace_members.includes(crate.id));
  const sources = new Map(packages.map(crate => [crate.name, readSources(path.join(path.dirname(crate.manifest_path), "src"))]));
  validateArchitecture(packages, sources);
  for (const filename of ["AGENTS.md", "CONTRIBUTING.md"]) assert(fs.existsSync(path.join(root, filename)), `${filename} is required`);
  console.log(`Architecture boundaries passed for ${packages.length} workspace crates.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) checkRepository(path.resolve(import.meta.dirname, ".."));
