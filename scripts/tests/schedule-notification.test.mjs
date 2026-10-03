import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

async function notifications() {
  const handlers = new Map();
  const messages = [];
  const context = vm.createContext({});
  const bindings = {
    "./00-surface.js": { openDialog() {}, closeSurface() {} },
    "./01-core.js": { invoke() {}, on: (name, handler) => handlers.set(name, handler), uiPrefsLoad() {} },
    "./03-shell.js": { currentProject: "project", toast: message => messages.push(message), toastError() {} },
    "./02-i18n.js": { t: text => text },
    "./03-general-scope.js": { isGeneralChat: () => false },
  };
  const source = await readFile(new URL("../../crates/kanzei-app/ui/24-schedules.js", import.meta.url), "utf8");
  const module = new vm.SourceTextModule(source, { context });
  await module.link(specifier => {
    const exports = bindings[specifier];
    assert.ok(exports, `Unexpected dependency ${specifier}`);
    return new vm.SyntheticModule(Object.keys(exports), function () {
      for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
    }, { context });
  });
  await module.evaluate();
  return { handler: handlers.get("kz:schedule-run"), messages };
}

test("schedule completion uses the Tauri payload envelope", async () => {
  const { handler, messages } = await notifications();
  handler({ payload: { notify: true, name: "daily", result: { summary: "finished" } } });
  assert.deepEqual(messages, ["daily: finished"]);
});

test("schedule failure reaches the notification", async () => {
  const { handler, messages } = await notifications();
  handler({ payload: { notify: true, name: "daily", error: "request failed" } });
  assert.deepEqual(messages, ["request failed"]);
});

test("schedule with notifications disabled stays silent", async () => {
  const { handler, messages } = await notifications();
  handler({ payload: { notify: false, name: "daily", result: { summary: "finished" } } });
  assert.deepEqual(messages, []);
});
