import assert from "node:assert/strict";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve, basename, dirname } from "node:path";
import { pathToFileURL } from "node:url";
import * as espree from "espree";
const ui = resolve("crates/kanzei-app/ui");
const files = (await readdir(ui)).filter(name => name.endsWith(".js"));
function edges(source) {
  const ast = espree.parse(source, { ecmaVersion: "latest", sourceType: "module" });
  return ast.body.filter(node => ["ImportDeclaration", "ExportNamedDeclaration", "ExportAllDeclaration"].includes(node.type) && node.source?.value.startsWith("./")).map(node => basename(node.source.value));
}
const source = new Map(await Promise.all(files.map(async name => [name, await readFile(resolve(ui, name), "utf8")])));
const graph = new Map([...source].map(([name, text]) => [name, edges(text)]));
const foundation = files.filter(name => name.startsWith("00-") || ["02-i18n.js", "01-project-resources.js", "04-status-words.js"].includes(name));
const allowed = new Set(foundation);
for (const name of foundation) for (const dependency of graph.get(name)) assert(allowed.has(dependency), `Foundation ${name} imports view/lifecycle owner ${dependency}`);
for (const [name, dependencies] of graph) if (name !== "01-core.js") assert(!dependencies.includes("01-core.js"), `${name} imports obsolete umbrella`);
const reachable = (name, seen = new Set()) => { if (seen.has(name)) return seen; seen.add(name); for (const next of graph.get(name) || []) reachable(next, seen); return seen; };
const html = await readFile(resolve(ui, "index.html"), "utf8");
const roots = [...html.matchAll(/<script[^>]+src="([^\"]+\.js)"/g)].map(match => match[1]).filter(name => graph.has(name));
assert.deepEqual(roots, ["bootstrap.js"], "Production must use one explicit module bootstrap");
const initialDocument = globalThis.document;
globalThis.document = new EventTarget();
try {
  const dom = await import(pathToFileURL(resolve(ui, "00-dom.js")).href);
  const installed = [];
  dom.defer(() => installed.push("first"));
  dom.defer(() => installed.push("second"));
  await Promise.resolve();
  assert.deepEqual(installed, [], "A microtask is not module readiness");
  dom.initializeUi();
  assert.deepEqual(installed, ["first", "second"], "Bootstrap installs collected owners in registration order");
  dom.defer(() => installed.push("lazy"));
  await Promise.resolve();
  assert.deepEqual(installed, ["first", "second", "lazy"], "Modules loaded after bootstrap install exactly once");
  assert.throws(() => dom.initializeUi(), /already initialized/);
  assert.deepEqual(installed, ["first", "second", "lazy"], "Duplicate bootstrap does not reinstall owners");
} finally {
  if (initialDocument === undefined) delete globalThis.document;
  else globalThis.document = initialDocument;
}
const startup = new Set(roots.flatMap(name => [...reachable(name)]));
for (const name of ["19-research.js", "19-research-auto.js", "19-research-navigation.js"]) assert(!startup.has(name), `${name} eagerly loaded`);
function cycles(input) {
  let index = 0; const indices = new Map(), low = new Map(), stack = [], onStack = new Set(), result = [];
  function visit(name) {
    indices.set(name, index); low.set(name, index++); stack.push(name); onStack.add(name);
    for (const next of input.get(name) || []) {
      if (!input.has(next)) continue;
      if (!indices.has(next)) { visit(next); low.set(name, Math.min(low.get(name), low.get(next))); }
      else if (onStack.has(next)) low.set(name, Math.min(low.get(name), indices.get(next)));
    }
    if (low.get(name) === indices.get(name)) { const component = []; let next; do { next = stack.pop(); onStack.delete(next); component.push(next); } while (next !== name); if (component.length > 1) result.push(component.sort()); }
  }
  for (const name of input.keys()) if (!indices.has(name)) visit(name);
  return result.sort((a, b) => b.length - a.length);
}
const previous = new Map();
for (const name of files) {
  try { previous.set(name, edges(execFileSync("git", ["show", `HEAD:crates/kanzei-app/ui/${name}`], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }))); } catch { /* New modules have no HEAD predecessor. */ }
}
const before = cycles(previous), after = cycles(graph);
for (const component of after) assert(!component.some(name => allowed.has(name)), `Foundation cycle: ${component.join(", ")}`);
const result = { at: new Date().toISOString(), foundationModules: foundation, foundationCycles: 0, productionUmbrellaImports: 0, eagerResearchModules: 0,
  bootstrap: { entries: roots, installationOrder: "explicit-ready", lazyModules: "once", duplicateInitialization: "rejected" },
  before: { largestComponent: before[0]?.length || 0, cycles: before }, after: { largestComponent: after[0]?.length || 0, cycles: after },
  remaining: "View orchestration and chat presentation still contain cycles; this boundary test does not claim all application modules are acyclic." };
const artifactPath = "output/design-audit-20261016/round2/foundation-boundaries.json";
await mkdir(dirname(artifactPath), { recursive: true });
await writeFile(artifactPath, JSON.stringify(result, null, 2));
console.log(`Foundation boundaries passed: ${foundation.length} modules without view dependencies/cycles; umbrella imports 0; eager research 0; largest application SCC ${before[0]?.length || 0} -> ${after[0]?.length || 0}.`);
