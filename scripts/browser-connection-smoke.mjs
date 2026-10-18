// Actual existing Edge + local CDP connection. This is helper acceptance, not native UI acceptance.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { formAction } from "./browser-form-action.mjs";

const output = path.resolve("output/playwright/browser-connection/helper"), run = path.join(output, String(Date.now()));
await mkdir(run, { recursive: true });
const checks = [], check = (value, label) => { assert(value, label); checks.push(label); };
const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
  if (req.url === "/embedded") { res.end('<title>Embedded frame</title><input id="frame-value" value="frame-original">'); return; }
  const logged = req.headers.cookie?.includes("owned-login=ready");
  res.end(`<!doctype html><title>${req.url}</title><h1 id="logged">${logged ? "Existing login preserved" : "Login required"}</h1><input id="value" value="original"><iframe src="/embedded"></iframe><button id="popup" onclick="window.open('/popup','_blank')">Popup</button>`);
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const reserve = net.createServer(); await new Promise(resolve => reserve.listen(0, "127.0.0.1", resolve)); const port = reserve.address().port; await new Promise(resolve => reserve.close(resolve));
let context, helper, reader, id = 0, redirectOrigin, redirectTarget;
const timer = setTimeout(() => helper?.kill(), 60000);
async function rpc(method, params = {}, owner = "authorized-conversation") {
  const request = ++id; helper.stdin.write(JSON.stringify({ id: request, method, params: { ...params, owner } }) + "\n");
  const line = await reader.next(); assert(!line.done, "Helper must return a response");
  const response = JSON.parse(line.value); assert.equal(response.id, request); return response.result;
}
try {
  context = await chromium.launchPersistentContext(path.join(run, "existing-edge"), { channel: "msedge", headless: true, args: [`--remote-debugging-port=${port}`] });
  await context.addCookies([{ name: "owned-login", value: "ready", url: origin, httpOnly: true, sameSite: "Lax" }]);
  let selected = context.pages()[0]; await selected.goto(`${origin}/selected`);
  const unrelated = await context.newPage(); await unrelated.goto(`${origin}/unrelated`);
  const privileged = await context.newPage();
  await privileged.route("http://tauri.localhost/**", route => route.fulfill({ contentType: "text/html", body: "APP_PRIVILEGED_MARKER" }));
  await privileged.goto("http://tauri.localhost/");
  check(await selected.locator("#logged").textContent() === "Existing login preserved", "Edge has an authenticated tab before Kanzei connects");
  helper = spawn(process.execPath, [path.resolve("scripts/browser-helper.mjs")], { stdio: ["pipe", "pipe", "inherit"], windowsHide: true });
  reader = createInterface({ input: helper.stdout })[Symbol.asyncIterator]();
  const connected = await rpc("connect", { endpoint: `http://127.0.0.1:${port}/` });
  assert(!connected.error, JSON.stringify(connected));
  const page = connected.pages.find(page => page.url.endsWith("/selected")); assert(page, JSON.stringify(connected));
  const target = { connection_id: connected.connection_id, page_id: page.page_id, expected_url: page.url };
  check(connected.pages.length === 2 && connected.pages.some(page => page.url.endsWith("/unrelated")), "Connection lists ordinary tabs with stable IDs and excludes application pages");
  const observed = await rpc("eval", { ...target, expression: "document.body.innerText" });
  check(!observed.error && observed.json.includes("Existing login preserved"), "Attached operations reuse the original login instead of a fresh context");
  check((await rpc("eval", { ...target, expression: "document.title" }, "different-conversation")).error.includes("another conversation"), "Connection identity cannot be reused by another conversation");
  check((await rpc("eval", { connection_id: target.connection_id, expression: "document.title" })).error.includes("PAGE_UNAVAILABLE"), "A connected operation requires an explicit page identity");
  const fill = await rpc("eval", { ...target, expression: `(${formAction.toString()})(${JSON.stringify({ action: "fill", selector: "#value", text: "attached edit" })})` });
  assert(!fill.error, JSON.stringify(fill));
  check(await selected.locator("#value").inputValue() === "attached edit" && await unrelated.locator("#value").inputValue() === "original", "Only the explicitly selected existing tab changes");
  await selected.frameLocator("iframe").locator("#frame-value").waitFor();
  const frames = await rpc("frames", target), embedded = frames.frames.find(frame => !frame.main && frame.url.endsWith("/embedded")); assert(embedded);
  const frameTarget = { ...target, frame_id: embedded.frame_id, expected_frame_url: embedded.url };
  const frameFill = await rpc("eval", { ...frameTarget, expression: `(${formAction.toString()})(${JSON.stringify({ action: "fill", selector: "#frame-value", text: "frame-only edit" })})` }); assert(!frameFill.error, JSON.stringify(frameFill));
  check(await selected.frameLocator("iframe").locator("#frame-value").inputValue() === "frame-only edit" && await selected.locator("#value").inputValue() === "attached edit", "An explicit iframe identity changes only its intended document");
  await selected.frames().find(frame => frame.url() === embedded.url).goto(`${origin}/frame-changed`);
  const staleFrame = await rpc("eval", { ...frameTarget, expression: "document.body.textContent='wrong-frame'" });
  check(staleFrame.error.includes("FRAME_CHANGED"), "A frame navigation invalidates its old authorization before mutation");
  await selected.goto(`${origin}/changed`);
  const stale = await rpc("eval", { ...target, expression: "document.querySelector('#value').value='wrong-page'" });
  check(stale.error.includes("PAGE_CHANGED") && await selected.locator("#value").inputValue() === "original", "Navigation invalidates the old URL before any mutation");
  const refreshed = await rpc("pages", { connection_id: target.connection_id });
  const current = refreshed.pages.find(item => item.page_id === target.page_id); assert(current);
  target.expected_url = current.url;
  await rpc("eval", { ...target, expression: "console.warn('selected-page-warning'); true" });
  const logs = await rpc("console", target);
  check(logs.errors.some(item => item.text === "selected-page-warning"), "Attached console observations belong to the selected page");
  const popupCreated = context.waitForEvent("page");
  await rpc("click", { ...target, selector: "#popup" });
  const popup = await popupCreated; await popup.waitForURL(`${origin}/popup`);
  const listed = await rpc("pages", { connection_id: target.connection_id });
  check(listed.pages.some(item => item.url.endsWith("/popup")) && listed.pages.some(item => item.page_id === target.page_id), "New popup tabs receive separate IDs without replacing the original target");
  await selected.close(); selected = await context.newPage(); await selected.goto(target.expected_url);
  const recreated = await rpc("pages", { connection_id: target.connection_id });
  const oldIdentity = await rpc("eval", { ...target, expression: "document.querySelector('#value').value='wrong-recreated-tab'" });
  check(oldIdentity.error.includes("PAGE_UNAVAILABLE") && recreated.pages.some(item => item.url === target.expected_url && item.page_id !== target.page_id) && await selected.locator("#value").inputValue() === "original", "Recreating a tab at the same URL cannot reuse its old authorization identity");
  const detached = await rpc("disconnect", { connection_id: target.connection_id });
  check(detached.disconnected && !selected.isClosed() && !unrelated.isClosed(), "Disconnect leaves the user's existing browser and tabs open");
  await selected.reload();
  check(await selected.locator("#logged").textContent() === "Existing login preserved", "Existing authentication survives disconnect");
  let followed = 0;
  redirectTarget = http.createServer((_req, res) => { followed++; res.end(); });
  redirectTarget.on("upgrade", (_req, socket) => { followed++; socket.end(); });
  await new Promise(resolve => redirectTarget.listen(0, "127.0.0.1", resolve));
  redirectOrigin = http.createServer((_req, res) => res.end(JSON.stringify({ webSocketDebuggerUrl: `ws://127.0.0.1:${redirectOrigin.address().port}/devtools/browser/redirect-test` })));
  redirectOrigin.on("upgrade", (_req, socket) => socket.end(`HTTP/1.1 302 Found\r\nLocation: ws://127.0.0.1:${redirectTarget.address().port}/devtools/browser/other\r\nConnection: close\r\n\r\n`));
  await new Promise(resolve => redirectOrigin.listen(0, "127.0.0.1", resolve));
  const redirect = await rpc("connect", { endpoint: `http://127.0.0.1:${redirectOrigin.address().port}/` });
  check(Boolean(redirect.error) && followed === 0, "CDP WebSocket redirects cannot contact another endpoint outside the granted origin");
  const second = await rpc("connect", { endpoint: `http://127.0.0.1:${port}/` }); assert(!second.error, JSON.stringify(second));
  await rpc("shutdown");
  check(!selected.isClosed() && await selected.title() === "/changed", "Helper shutdown also detaches without closing the existing browser");
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, boundary: "Actual Edge was running and authenticated before the helper connected; isolated owned profile and local HTTP service; native Kanzei UI not involved." }, null, 2));
  console.log(`Browser connection helper PASS: ${checks.length} checks`);
} finally {
  helper?.stdin.end();
  if (helper && helper.exitCode === null) await new Promise(resolve => helper.on("exit", resolve));
  await context?.close(); clearTimeout(timer); server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  for (const server of [redirectOrigin, redirectTarget]) if (server) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}
