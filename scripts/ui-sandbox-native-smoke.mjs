// Actual native Kanzei runner -> copied inputs -> WSL bubblewrap -> OS cleanup.
/* global window, document */
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, copyFile, writeFile, readFile, readdir } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { decodeExternalSource } from "./fixtures/tool-source.mjs";

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-sandbox-native-smoke.mjs <exe>");
const output = path.resolve("output/playwright/sandbox/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "input.txt"), "NATIVE_EXPLICIT_INPUT_BYTES");
await writeFile(path.join(project, ".env"), "NATIVE_SANDBOX_SECRET_CANARY");
const marker = `native-sandbox-orphan-${randomUUID()}`;
const checks = [], errors = [], results = {}, providerBodies = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
function respond(res, delta) {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.end(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: delta.tool_calls ? "tool_calls" : "stop" }] })}\n\ndata: [DONE]\n\n`);
}
const toolReply = (res, name, args, step) => respond(res, { tool_calls: [{ index: 0, id: `tool-${step}`, type: "function", function: { name, arguments: JSON.stringify(args) } }] });
const model = http.createServer(async (req, res) => {
  try {
    if (req.url === "/v1/models") { res.writeHead(200, { "Content-Type": "application/json" }); res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
    let raw = ""; for await (const chunk of req) raw += chunk;
    const body = JSON.parse(raw); providerBodies.push(body);
    const original = body.messages?.find(message => message.role === "user" && typeof message.content === "string" && message.content.startsWith("NATIVE_SANDBOX_"));
    if (!original) { respond(res, { content: "No memory candidates." }); return; }
    const mode = original.content.split("：")[0], tools = body.messages.filter(message => message.role === "tool");
    if (!tools.length) { toolReply(res, "tool_search", { query: "select:sandbox" }, 0); return; }
    if (tools.length === 1) {
      if (mode.endsWith("HOST")) { toolReply(res, "bash", { command: "[IO.File]::WriteAllText('host-escape.txt','HOST_ESCAPE')" }, 1); return; }
      const command = mode.endsWith("CANCEL") ? `setsid /bin/sh -c 'while :; do sleep 1; done' ${marker} & wait`
        : mode.endsWith("TIMEOUT") ? "sleep 60" : "cat input.txt; printf '\\n'; test ! -e /mnt/c; printf DISPOSABLE > input.txt; cat input.txt";
      toolReply(res, "sandbox", { command, files: [mode.endsWith("SECRET") ? ".env" : "input.txt"], timeout_ms: mode.endsWith("TIMEOUT") ? 200 : 30000 }, 1); return;
    }
    const decoded = decodeExternalSource(tools.at(-1).content); results[mode] = decoded;
    if (!mode.endsWith("SECRET") && !mode.endsWith("HOST")) assert.equal(decoded.source?.kind, "process");
    respond(res, { content: `${mode}：软件实际执行和来源回执已核对。` });
  } catch (error) { errors.push(String(error)); respond(res, { content: `FIXTURE_FAILED: ${error.message}` }); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` } });
let browser, page, database;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(predicate, label, timeout = 60000) { const deadline = Date.now() + timeout; while (Date.now() < deadline) { if (await predicate()) return; await new Promise(resolve => setTimeout(resolve, 100)); } throw new Error(label); }
function children() {
  const script = `import os,pathlib; marker=${JSON.stringify(marker)}.encode(); found=[]\nfor p in pathlib.Path('/proc').iterdir():\n if p.name.isdigit() and int(p.name)!=os.getpid():\n  try:\n   if marker in (p/'cmdline').read_bytes(): found.append(p.name)\n  except (FileNotFoundError, PermissionError): pass\nprint(len(found))`;
  return Number(execFileSync("wsl.exe", ["--exec", "/usr/bin/python3", "-I", "-c", script], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], windowsHide: true }).trim());
}
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); return true; } catch { return false; } }, "Native app starts", 30000);
  await until(() => { page = browser.contexts().flatMap(context => context.pages())[0]; return Boolean(page); }, "Native WebView page is created");
  page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await invoke("projects_init", { path: project, name: "命令隔离原生验收" }); await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator(".workbench-project-name").filter({ hasText: "命令隔离原生验收" }).click();
  let isolatedLine;
  let generalRoot;
  for (const mode of ["NATIVE_SANDBOX_RUN", "NATIVE_SANDBOX_SECRET", "NATIVE_SANDBOX_TIMEOUT", "NATIVE_SANDBOX_CANCEL", "NATIVE_SANDBOX_ISOLATED_RUN", "NATIVE_SANDBOX_ISOLATED_HOST", "NATIVE_SANDBOX_ISOLATED_STALE_HOST", "NATIVE_SANDBOX_ISOLATED_GENERAL_HOST"]) {
    const general = mode.includes("GENERAL");
    if (general) {
      await page.evaluate(async () => (await import("./29-general-chat.js")).openGeneralChat());
      generalRoot = await invoke("general_chat_open");
    }
    const root = general ? generalRoot : project;
    const line = await invoke("process_create", { projectDir: root, profile: "dev", model: "stub:test", subagentMode: "off" });
    await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses({ force: true }); await sessions.switchProcess(id, true); }, line.id);
    if (mode.includes("ISOLATED")) {
      if (!general) isolatedLine = line.id;
      if (!await page.locator("#composer-more-menu").isVisible()) await page.locator("#composer-more").click();
      await page.locator("#profile-select").selectOption("isolated");
      await page.locator("#auto-allow").check();
      if (mode.includes("STALE")) {
        await invoke("run_prompt", { projectDir: root, processId: line.id, prompt: `${mode}：旧发送者试图返回开发模式。`, profile: "dev", agent: "dev-pair", model: "stub:test", autonomous: true, autoAllow: true });
      } else {
        await page.locator("#prompt").fill(`${mode}：在隔离工作区执行指定输入。`);
        await page.locator("#send").click();
      }
    } else await invoke("run_prompt", { projectDir: project, processId: line.id, prompt: `${mode}：在隔离工作区执行指定输入。`, profile: "dev", agent: "dev-pair", model: "stub:test", autonomous: true, autoAllow: true });
    if (mode.endsWith("CANCEL")) {
      await until(() => children() > 0, "Actual Linux command exists before cancellation");
      await invoke("stop_run", { projectDir: project, processId: line.id });
      check(children() === 0, "Native stop awaits cleanup of the actual detached Linux process tree");
    } else {
      await until(async () => { assert.equal(errors.length, 0, errors.join("; ")); const rows = await invoke("collaboration_snapshot", { projectDir: root }); return results[mode] && rows.find(row => row.process_id === line.id)?.status === "completed"; }, `${mode} completes`);
      await invoke("stop_run", { projectDir: root, processId: line.id });
      if (mode.endsWith("HOST")) check(results[mode].content.includes("unknown tool `bash`"), "Actual isolated conversation rejects a provider attempting to call the absent host shell");
      else if (mode.endsWith("SECRET")) check(results[mode].content.includes("permission requires user approval"), "AutoAllow cannot copy .env into the sandbox without exact consent");
      else {
        const receipt = JSON.parse(results[mode].content.replace(/^\[tool error\]\n/, ""));
        check(receipt.isolation_verified && receipt.network === "denied" && receipt.host_writes === "none", `${mode}: real kernel namespace receipt survives the runner`);
        check(receipt.status === (mode.endsWith("TIMEOUT") ? "timeout" : "completed"), `${mode}: actual execution status remains explicit`);
        if (mode.endsWith("RUN")) check(receipt.output.includes("NATIVE_EXPLICIT_INPUT_BYTES") && receipt.output.includes("DISPOSABLE"), "Real native sandbox runs against copied bytes and can modify only its disposable copy");
      }
    }
  }
  check(await readFile(path.join(project, "input.txt"), "utf8") === "NATIVE_EXPLICIT_INPUT_BYTES", "Real project input remains unchanged after sandbox writes");
  check(await readFile(path.join(project, ".env"), "utf8") === "NATIVE_SANDBOX_SECRET_CANARY", "Real sensitive file remains unchanged");
  check(!JSON.stringify(providerBodies).includes("NATIVE_SANDBOX_SECRET_CANARY"), "Sensitive bytes never reach the model through this execution path");
  const isolatedRequests = providerBodies.filter(body => body.messages?.some(message => typeof message.content === "string" && message.content.startsWith("NATIVE_SANDBOX_ISOLATED_")));
  check(isolatedRequests.length > 0 && isolatedRequests.every(body => body.tools?.length && body.tools.every(tool => !["bash", "git", "process", "browser", "task", "write", "edit", "latex", "plot", "websearch", "webfetch"].includes(tool.function.name))), "Actual isolated model requests never advertise host execution, network or mutation tools");
  const { existsSync } = await import("node:fs");
  check(!existsSync(path.join(project, "host-escape.txt")), "Forced host command produces zero project writes in isolated mode");
  database = new DatabaseSync(path.join(project, ".kanzei/state.db"), { readOnly: true });
  const baselines = database.prepare("SELECT payload_json FROM session_events WHERE event_type='run.baseline'").all().map(row => JSON.parse(row.payload_json));
  check(baselines.filter(fact => fact.status === "not_observed_isolated" && fact.head === null).length === 3, "All project isolated starts record unknown origin without running host Git");
  check(!(await readdir(generalRoot, { recursive: true })).some(file => path.basename(file) === "host-escape.txt"), "Projectless isolated conversation cannot execute the forced host command");
  const restoredGeneral = await invoke("general_chat_open");
  check((await invoke("process_list", { projectDir: restoredGeneral })).some(line => line.profile === "isolated"), "Projectless reopening preserves isolated mode");
  await page.locator(".workbench-project-name").filter({ hasText: "命令隔离原生验收" }).click();
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, isolatedLine);
  check(await page.locator("#profile-select").inputValue() === "isolated", "Real UI execution-mode selection survives history reload");
  await page.screenshot({ path: path.join(output, "native-sandbox.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, results, executable: exe }, null, 2));
  console.log(`Sandbox native PASS: ${checks.length} checks`);
} catch (error) {
  if (page && !page.isClosed()) await page.screenshot({ path: path.join(output, "native-failure.png") });
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, errors, results }, null, 2)); throw error;
} finally {
  database?.close();
  if (page && !page.isClosed()) await invoke("runtime_shutdown").catch(error => console.warn(String(error)));
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
