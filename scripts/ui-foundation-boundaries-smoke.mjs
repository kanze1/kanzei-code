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
for (const name of ["19-research.js", "19-research-navigation.js"]) assert(!startup.has(name), `${name} eagerly loaded`);
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
assert(!before.length || (after[0]?.length || 0) <= (before[0]?.length || 0),
  `最大环路从 ${before[0]?.length} 增长到 ${after[0]?.length} 个模块:新增的 import 必须保持单向,否则先把被依赖的一抽出去。`);

// 环路本身不是缺陷:ESM 在 link 期就初始化 `export function` 绑定,环内模块在求值期互相
// 调用函数是安全的。缺陷是求值期碰环内伙伴的 `let`/`const`/re-export——那是 TDZ,
// 连 typeof 兜底都会抛 ReferenceError(见 ui-lint-smoke 的恒真守卫一节)。这条闸把
// 「环内只允许求值期调用函数导出」钉死:45 模块的大环可以存在,但它的安全前提必须可验证。
const asts = new Map(await Promise.all(files.map(async name => [name, espree.parse(source.get(name), { ecmaVersion: "latest", sourceType: "module", loc: true })])));
function exportKinds(ast) {
  const fn = new Set(), other = new Set();
  for (const node of ast.body) {
    if (node.type !== "ExportNamedDeclaration") continue;
    const d = node.declaration;
    if (d?.type === "FunctionDeclaration" && d.id) fn.add(d.id.name);
    else if (d?.type === "VariableDeclaration") for (const item of d.declarations) other.add(item.id.name);
    else if (d?.type === "ClassDeclaration" && d.id) other.add(d.id.name);
    for (const spec of node.specifiers || []) if (spec.exported.name !== "default") other.add(spec.exported.name);
  }
  return { fn, other };
}
// 只走「模块求值时会执行」的语句:函数体、类方法体里的引用要等到调用时才发生,不算。
function evaluatedRefs(node, out, parentKey) {
  if (!node || typeof node !== "object") return;
  if (["FunctionDeclaration", "FunctionExpression", "ArrowFunctionExpression", "ClassBody"].includes(node.type)) return;
  if ((node.type === "CallExpression" || node.type === "NewExpression") && node.callee?.type === "Identifier") out.push(node.callee.name);
  if (node.type === "Identifier" && !["property", "key", "id", "local", "exported"].includes(parentKey)) out.push(node.name);
  for (const [key, value] of Object.entries(node)) {
    if (key === "loc" || key === "range" || key === "callee") continue;
    if (Array.isArray(value)) for (const item of value) evaluatedRefs(item, out, key);
    else if (value && typeof value === "object" && value.type) evaluatedRefs(value, out, key);
  }
}
const kinds = new Map([...asts].map(([name, ast]) => [name, exportKinds(ast)]));
const localOrigin = new Map([...asts].map(([name, ast]) => [name, new Map(ast.body
  .filter(node => node.type === "ImportDeclaration" && node.source.value.startsWith("./"))
  .flatMap(node => node.specifiers.filter(spec => spec.type === "ImportSpecifier").map(spec => [spec.local.name, basename(node.source.value)])))]));
const cyclic = new Set(after.flat());
const hazards = [];
for (const [name, ast] of asts) {
  if (!cyclic.has(name)) continue;
  const origins = localOrigin.get(name);
  for (const statement of ast.body) {
    if (["ImportDeclaration", "ExportAllDeclaration"].includes(statement.type)) continue;
    const refs = []; evaluatedRefs(statement, refs, "root");
    for (const id of new Set(refs)) {
      const provider = origins.get(id);
      if (!provider || !cyclic.has(provider)) continue;
      const kind = kinds.get(provider);
      if (kind?.fn.has(id)) continue;
      hazards.push(`${name}:${statement.loc.start.line} 求值期${kind?.other.has(id) ? "引用" : "引用未确认导出"} ${provider} 的 ${id}(非 export function)`);
    }
  }
}
// 自检:探测器必须真的抓得到 TDZ 形态,否则 0 违规只是静默失效。
{
  const probeAst = espree.parse(`import { fn, val } from "./99-probe.js";
const now = fn();
const later = val;
function deferred() { return val + fn(); }
`, { ecmaVersion: "latest", sourceType: "module", loc: true });
  const probeLocal = new Map([["fn", "99-probe.js"], ["val", "99-probe.js"]]);
  const refs = []; for (const statement of probeAst.body) if (statement.type !== "ImportDeclaration") evaluatedRefs(statement, refs, "root");
  const seen = [...new Set(refs)];
  assert.deepEqual(seen, ["fn", "val"], "TDZ 探测器自检:求值期引用应抓到 fn 与 val");
  assert(!seen.includes("deferred"), "TDZ 探测器自检:函数声明名不该被当成求值期引用");
  const bodyRefs = []; evaluatedRefs(probeAst.body.find(node => node.type === "FunctionDeclaration"), bodyRefs, "root");
  assert.deepEqual(bodyRefs, [], "TDZ 探测器自检:函数体要等调用才执行,不得计入求值期");
  const probeKinds = new Map([["99-probe.js", { fn: new Set(["fn"]), other: new Set(["val"]) }]]);
  const flagged = [...new Set(refs)].filter(id => !probeKinds.get("99-probe.js").fn.has(id));
  assert.deepEqual(flagged, ["val"], "TDZ 探测器自检:只有非函数导出该被判为危险");
}
assert.equal(hazards.length, 0, `环路求值期 TDZ 风险(${hazards.length} 处):\n${hazards.join("\n")}`);

const result = { at: new Date().toISOString(), foundationModules: foundation, foundationCycles: 0, productionUmbrellaImports: 0, eagerResearchModules: 0,
  bootstrap: { entries: roots, installationOrder: "explicit-ready", lazyModules: "once", duplicateInitialization: "rejected" },
  before: { largestComponent: before[0]?.length || 0, cycles: before }, after: { largestComponent: after[0]?.length || 0, cycles: after },
  cyclicModules: cyclic.size, evaluationTimeCrossModuleRefsInsideCycles: { nonFunctionExports: hazards.length },
  remaining: "View orchestration and chat presentation still contain cycles; this boundary test does not claim all application modules are acyclic. Cycles are tolerated only because every evaluation-time cross-module reference inside them targets an export function." };
const artifactPath = "output/design-audit-20261016/round2/foundation-boundaries.json";
await mkdir(dirname(artifactPath), { recursive: true });
await writeFile(artifactPath, JSON.stringify(result, null, 2));
console.log(`Foundation boundaries passed: ${foundation.length} modules without view dependencies/cycles; umbrella imports 0; eager research 0; largest application SCC ${before[0]?.length || 0} -> ${after[0]?.length || 0}; 环内 ${cyclic.size} 个模块的求值期跨模块引用里 ${hazards.length} 处落在非函数导出上。`);
