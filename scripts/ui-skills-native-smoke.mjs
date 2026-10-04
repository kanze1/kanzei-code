/* WebView2 + real Rust IPC + global files, using a local model fixture. */
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-skills-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/skills-native"), run = path.join(output, String(Date.now()));
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei"), project = path.join(run, "project");
for (const dir of [home, project, path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
const checks = [], errors = [], requests = [];
let mode = "valid";
const model = http.createServer(async (request, response) => {
  if (request.method === "GET") { response.writeHead(200, { "content-type": "application/json" }); response.end(JSON.stringify({ data: [{ id: "skills-global-model", object: "model" }] })); return; }
  let body = ""; for await (const part of request) body += part;
  if (!body) { response.writeHead(400); response.end("empty request"); return; }
  requests.push(JSON.parse(body));
  const draft = { name: "weekly-review", description: "整理用户提供的周报", instructions: "读取用户模板，基于实际资料整理周报；不编造未完成的工作。" };
  const text = mode === "invalid" ? "invalid json" : JSON.stringify(draft);
  response.writeHead(200, { "content-type": "text/event-stream" });
  response.end(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: text }, finish_reason: mode === "truncated" ? "length" : "stop" }] })}\n\ndata: [DONE]\n\n`);
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(home, "kanzei.toml"), `proxy = "off"\n[models]\nprimary = "mock:skills-global-model"\nfast = "mock:skills-global-model"\n[providers.mock]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\n`);
await mkdir(path.join(project, ".kanzei"), { recursive: true });
await writeFile(path.join(project, ".kanzei/kanzei.toml"), '[models]\nprimary = "mock:wrong-project-model"\n');
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [], current: null, theme: "light" }));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
let app, browser, page;
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label) {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
async function start() {
  const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
  const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  app = spawn(exe, [], { cwd: run, windowsHide: true, stdio: "ignore", env: { ...process.env,
    KANZEI_HOME: home, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
    APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: undefined, KANZEI_E2E_CDP: String(port),
  } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2 startup");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000);
  page.on("pageerror", error => errors.push(String(error)));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 45000 });
}
async function close() {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  for (const entry of await readdir(path.join(home, "runtime"), { withFileTypes: true }).catch(() => [])) {
    if (!entry.isDirectory()) continue;
    const endpoint = JSON.parse(await readFile(path.join(home, "runtime", entry.name, "endpoint.json"), "utf8").catch(() => "null"));
    if (!endpoint || path.resolve(endpoint.executable || "").toLowerCase() !== exe.toLowerCase() || !/^127\.0\.0\.1:\d+$/.test(endpoint.addr || "")) continue;
    await new Promise(resolve => {
      const socket = net.createConnection({ host: "127.0.0.1", port: Number(endpoint.addr.split(":").at(-1)) });
      const finish = () => { socket.destroy(); resolve(); };
      socket.setTimeout(1500, finish); socket.on("error", finish); socket.on("data", finish);
      socket.on("connect", () => { const body = Buffer.from(JSON.stringify({ action: "shutdown", token: endpoint.token })), size = Buffer.alloc(4); size.writeUInt32BE(body.length); socket.write(Buffer.concat([size, body])); });
    });
  }
  await browser?.close(); browser = null; page = null;
  if (app && app.exitCode === null) { app.kill(); await new Promise(resolve => app.once("exit", resolve)); }
}
try {
  await start();
  const generalRoot = path.join(home, "conversations/general");
  const generalBefore = await invoke("process_list", { projectDir: generalRoot });
  const initial = await invoke("skills_list");
  check(initial.length === 9 && initial.every(skill => skill.source === "builtin" && skill.enabled), "Fresh desktop installation discovers nine enabled built-ins");
  await page.locator("#skills-nav").click();
  await page.waitForFunction(() => document.querySelectorAll(".skill-library-row").length === 9);
  check(await page.locator("body").getAttribute("data-app-scope") === "global", "The native manager opens before any project or conversation exists");
  await page.locator('[data-skill-name="pdf"]').getByRole("switch").uncheck({ force: true });
  await until(async () => !(await invoke("skills_list")).find(skill => skill.name === "pdf").enabled, "global disable");
  check(JSON.parse(await readFile(path.join(home, "skills.json"), "utf8")).disabled.includes("pdf"), "Native toggle writes global preferences");
  await page.locator("#skills-generate-open").click(); await page.locator("#skills-generation-prompt").fill("把周报模板整理为一个可复用技能");
  await page.locator("#skills-generate").click();
  await until(async () => await page.locator("#skill-name").inputValue() === "weekly-review", "generated draft");
  check(requests.length === 1 && requests[0].model === "skills-global-model", "Generation uses the global model through the real provider protocol");
  check(!(await invoke("skills_list")).some(skill => skill.name === "weekly-review"), "Generated drafts are not installed before Save");
  await page.locator("#skills-save").click();
  await until(async () => (await invoke("skills_list")).some(skill => skill.name === "weekly-review"), "saved draft");
  const saved = await invoke("skills_read", { name: "weekly-review" });
  check(saved.editable && saved.instructions.includes("不编造"), "Saving creates a real global SKILL.md with the generated instructions");
  const source = path.join(run, "source", "imported-check");
  await mkdir(path.join(source, "references"), { recursive: true }); await mkdir(path.join(source, "scripts"), { recursive: true });
  await writeFile(path.join(source, "SKILL.md"), "---\nname: imported-check\ndescription: >\n  Imported task workflow\nallowed-tools: [Read, Bash]\nmetadata:\n  owner: test\n---\nRead references/context.md.\n");
  await writeFile(path.join(source, "references/context.md"), "source evidence"); await writeFile(path.join(source, "scripts/check.py"), "print('fixture')\n");
  const imported = await invoke("skills_import", { directory: source });
  check(await readFile(path.join(home, "skills/imported-check/references/context.md"), "utf8") === "source evidence", "Native import retains referenced files");
  await invoke("skills_save", { draft: { name: imported.name, description: "Updated workflow", instructions: "Read references/context.md and run scripts/check.py.", manualOnly: true, userInvocable: true }, expectedHash: imported.revision });
  const updatedText = await readFile(path.join(home, "skills/imported-check/SKILL.md"), "utf8");
  check(updatedText.includes("allowed-tools:") && updatedText.includes("owner: test"), "Editing preserves imported metadata and tool declarations");
  let conflict = false;
  try { await invoke("skills_save", { draft: { name: imported.name, description: "Stale change", instructions: "Wrong body" }, expectedHash: imported.revision }); } catch { conflict = true; }
  check(conflict && (await invoke("skills_read", { name: imported.name })).description === "Updated workflow", "A stale editor cannot overwrite a newer skill");
  const duplicate = await invoke("skills_duplicate", { name: imported.name, newName: "imported-copy" });
  check(duplicate.editable && await readFile(path.join(home, "skills/imported-copy/scripts/check.py"), "utf8") === "print('fixture')\n", "Copying preserves executable resources in the custom skill");
  const conversations = await invoke("process_list", { projectDir: generalRoot });
  check(JSON.stringify(conversations.map(item => item.id)) === JSON.stringify(generalBefore.map(item => item.id)), "Skill generation does not create a general conversation");
  mode = "invalid"; let invalid = false; try { await invoke("skills_generate", { description: "Invalid format test" }); } catch { invalid = true; }
  mode = "truncated"; let truncated = false; try { await invoke("skills_generate", { description: "Truncation test" }); } catch { truncated = true; }
  check(invalid && truncated, "Malformed and truncated model responses are reported without saving skills");
  mode = "valid";
  const active = conversations[0];
  await invoke("run_prompt", { projectDir: generalRoot, processId: active.id, prompt: "请使用$weekly-review", autoAllow: true });
  await until(async () => requests.some(request => JSON.stringify(request.messages).includes("请使用$weekly-review"))
    && !(await invoke("process_list", { projectDir: generalRoot })).some(item => item.running), "explicit global skill invocation");
  const actual = requests.find(request => JSON.stringify(request.messages).includes("请使用$weekly-review"));
  const system = actual.messages.filter(message => message.role === "system").map(message => typeof message.content === "string" ? message.content : JSON.stringify(message.content)).join("\n");
  check(system.includes("不编造未完成的工作"), "A real conversation receives the explicitly invoked global skill body");
  check(!system.includes(path.join(home, "builtin-skills/pdf/SKILL.md")), "Disabled skills stay out of the conversation's discovery context");
  await page.locator("#skills-editor-close").click();
  await page.locator("#skills-refresh").click();
  await page.screenshot({ path: path.join(output, "skills-native.png") });
  await close(); await start();
  const restored = await invoke("skills_list");
  check(!restored.find(skill => skill.name === "pdf").enabled && restored.some(skill => skill.name === "weekly-review"), "Restart restores both disabled built-ins and saved personal skills");
  const latest = await invoke("skills_read", { name: imported.name });
  await invoke("skills_delete", { name: imported.name, expectedHash: latest.revision });
  check(!(await invoke("skills_list")).some(skill => skill.name === imported.name) && (await readdir(path.join(home, "skill-trash"))).some(name => name.includes(imported.name)), "Deleting a personal skill removes it globally and keeps recoverable files");
  let protectedBuiltin = false; try { await invoke("skills_delete", { name: "pdf", expectedHash: "invalid" }); } catch { protectedBuiltin = true; }
  check(protectedBuiltin, "Native management cannot delete a built-in");
  check(errors.length === 0, `No native UI runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "checks.json"), JSON.stringify({ checks, errors, modelRequests: requests.length, executable: exe, boundary: "Actual WebView2 and Rust; model responses served by an isolated local fixture" }, null, 2));
} catch (error) { if (page) await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); console.error({ checks, errors }); throw error; }
finally { await close(); await new Promise(resolve => model.close(resolve)); }
