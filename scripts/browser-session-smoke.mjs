// AF-T09: exercise the actual helper and browser, including targetless actions.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const child = spawn(process.execPath, [fileURLToPath(new URL("./browser-helper.mjs", import.meta.url))], {
  stdio: ["pipe", "pipe", "inherit"], windowsHide: true,
});
const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
const deadline = setTimeout(() => child.kill(), 60000);
let id = 0;
async function rpc(method, owner, params = {}) {
  child.stdin.write(JSON.stringify({ id: ++id, method, params: { ...params, owner } }) + "\n");
  const line = await lines.next();
  assert.equal(line.done, false, "helper exited before response");
  const reply = JSON.parse(line.value);
  assert.equal(reply.id, id);
  return reply.result;
}
try {
  assert.match((await rpc("eval", "B", { expression: "document.title" })).error, /call open first/);
  for (const owner of ["parent", "A", "B"]) {
    const result = await rpc("open", owner, { url: `data:text/html,<title>${owner}</title>`, channel: "msedge" });
    assert.equal(result.title, owner, JSON.stringify(result));
  }
  for (const owner of ["parent", "A", "B", "A", "parent"]) {
    const result = await rpc("eval", owner, { expression: "document.title" });
    assert.equal(result.json, JSON.stringify(owner), JSON.stringify(result));
  }
  assert.match((await rpc("eval", undefined, { expression: "document.title" })).error, /owner is required/);
  await rpc("shutdown", undefined);
  assert.match((await rpc("eval", "A", { expression: "document.title" })).error, /call open first/);
  console.log("PASS browser session isolation: parent/A/B/A/parent, unopened owner, missing owner, shutdown");
} finally {
  child.stdin.end();
  if (child.exitCode === null) await new Promise((resolve) => child.on("exit", resolve));
  clearTimeout(deadline);
}
