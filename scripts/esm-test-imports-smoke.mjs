import assert from "node:assert/strict";
import vm from "node:vm";
import { collectEsmImports } from "./esm-test-imports.mjs";

const source = `
// import { phantom } from './comment.js';
import './initialize.js';
import { value as local } from './dependency.js';
export { value as projected } from './dependency.js';
globalThis.observed = local;
`;
const context = vm.createContext({});
const root = new vm.SourceTextModule(source, { context });
const imports = collectEsmImports(new Map([["root", source]]), new Map());
assert.deepEqual([...imports.keys()], ["./initialize.js", "./dependency.js"]);
const modules = new Map([...imports].map(([specifier, names]) => [specifier, new vm.SyntheticModule([...names], function () {
  if (specifier === "./initialize.js") context.initialized = true;
  for (const name of names) this.setExport(name, 7);
}, { context })]));
await root.link(specifier => modules.get(specifier));
await root.evaluate();
assert.equal(context.initialized, true);
assert.equal(context.observed, 7);
assert.equal(root.namespace.projected, 7);
console.log("ESM fixture loader links side effects, aliases and reexports without phantom comment imports.");
