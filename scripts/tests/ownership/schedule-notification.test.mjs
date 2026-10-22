import assert from "node:assert/strict";
import { loadSchedules } from "./schedules-fixture.mjs";
import test from "node:test";
import vm from "node:vm";

async function notifications() {
  const handlers = new Map();
  const messages = [];
  const context = vm.createContext({});
  const bindings = {
    "./00-surface.js": { openDialog() {}, closeSurface() {} },
    "./00-ipc.js": { invoke() {} },
    "./01-runtime-events.js": { on: (name, handler) => handlers.set(name, handler) },
    "./03-shell.js": { currentProject: "project", toast: message => messages.push(message), toastError() {} },
    "./02-i18n.js": { t: text => text },
    "./03-general-scope.js": { isGeneralChat: () => false },
  };
  await loadSchedules(context, bindings);
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
