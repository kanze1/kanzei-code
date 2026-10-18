// Check frontend call sites against the manifest generated from Rust, not a JS fixture.
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import { parse } from "espree";
const root = path.resolve(import.meta.dirname, "..");
const contracts = JSON.parse(fs.readFileSync(path.join(root, "scripts/ipc-commands.json"), "utf8"));
const ui = path.join(root, "crates/kanzei-app/ui");
let calls = 0;
const failures = [];
function walk(node, file) {
  if (!node || typeof node !== "object") return;
  if (node.type === "CallExpression" && node.callee?.name === "invoke" && typeof node.arguments[0]?.value === "string") {
    const command = node.arguments[0].value, contract = contracts[command];
    if (command.startsWith("plugin:")) return;
    if (!contract) failures.push(`${file}:${node.loc.start.line}: unknown command ${command}`);
    else {
      calls++;
      const object = node.arguments[1];
      const properties = object?.type === "ObjectExpression" ? object.properties : [];
      const keys = properties.filter(p => p.type === "Property" && !p.computed).map(p => p.key.name || p.key.value);
      for (const key of keys) if (!(key in contract.parameters)) failures.push(`${file}:${node.loc.start.line}: ${command} has no argument ${key}`);
      if (!object || object.type === "ObjectExpression" && properties.every(p => p.type === "Property" && !p.computed)) {
        for (const [key, field] of Object.entries(contract.parameters)) if (field.required && !keys.includes(key)) failures.push(`${file}:${node.loc.start.line}: ${command} requires ${key}`);
      }
    }
  }
  for (const [key, value] of Object.entries(node)) if (key !== "loc") {
    if (Array.isArray(value)) for (const child of value) walk(child, file);
    else if (value && typeof value === "object") walk(value, file);
  }
}
for (const file of fs.readdirSync(ui).filter(f => f.endsWith(".js"))) walk(parse(fs.readFileSync(path.join(ui, file), "utf8"), { ecmaVersion: "latest", sourceType: "module", loc: true }), file);
assert.deepEqual(failures, [], failures.join("\n"));
assert.ok(calls > 100, "Frontend contract coverage must not silently become empty");
const sent = [];
let rejection;
globalThis.window = { __TAURI__: { core: { invoke: async (...args) => { sent.push(args); if (rejection !== undefined) throw rejection; return "accepted"; } }, event: { listen() {} } } };
const { invoke } = await import("../crates/kanzei-app/ui/00-ipc.js");
const { uiConsoleLog } = await import("../crates/kanzei-app/ui/00-console-log.js");
await assert.rejects(invoke("process_list", { project_dir: "fixture" }), /Unknown process_list argument/);
await assert.rejects(invoke("run_prompt", { projectDir: "fixture" }), /Missing run_prompt argument: prompt/);
await assert.rejects(invoke("stop_run", { processId: "conversation" }), /Missing stop_run argument: projectDir/);
assert.equal(sent.length, 0, "Invalid commands must fail before crossing IPC");
assert.equal(await invoke("process_list", { projectDir: "fixture" }), "accepted");
assert.equal(await invoke("stop_run", { projectDir: "fixture", processId: "conversation" }), "accepted");
assert.equal(await invoke("ui_prefs_set", { work_priority: {} }), "accepted", "Explicit snake_case command metadata is preserved");
rejection = "拒绝访问。 (os error 5)";
await assert.rejects(invoke("process_list", { projectDir: "private-user-path" }), error => {
  assert.equal(error, rejection, "The original string rejection must remain unchanged for UI consumers");
  assert.equal(uiConsoleLog.at(-1).level, "ipc-failure");
  assert.equal(uiConsoleLog.at(-1).text, "IPC process_list 拒绝访问。 (os error 5)");
  assert(!uiConsoleLog.at(-1).text.includes("private-user-path"), "Transport context must not serialize argument values");
  return true;
});
rejection = new Error("storage unavailable");
await assert.rejects(invoke("process_list", { projectDir: "fixture" }), error => error === rejection);
assert(uiConsoleLog.at(-1).text.includes("IPC process_list"));
console.log(`IPC argument contract PASS: ${calls} call sites, ${Object.keys(contracts).length} registered commands, transport rejection and rename cases`);
