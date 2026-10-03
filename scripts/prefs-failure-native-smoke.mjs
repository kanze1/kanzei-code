// Isolated native IPC regression. Supply the executable's recorded SHA256 explicitly.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, readdir, stat, writeFile, rm } from "node:fs/promises";
import net from "node:net";
import path from "node:path";

const [binary, output, expectedSha, provenance = "caller-supplied binary; no build claim"] = process.argv.slice(2);
assert(binary && output && /^[a-f0-9]{64}$/i.test(expectedSha ?? ""),
  "usage: node scripts/prefs-failure-native-smoke.mjs BINARY OUTPUT RECORDED_SHA256 [PROVENANCE]");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const evidence = path.resolve(output), run = path.join(evidence, `native-${Date.now()}`);
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei");
const project = path.join(run, "project-A"), otherProject = path.join(run, "project-B");
const prefsFile = path.join(home, "app.json"), exe = path.join(run, "kzapp.exe");
const sourceExe = await readFile(path.resolve(binary)), sourceStat = await stat(path.resolve(binary));
assert.equal(hash(sourceExe), expectedSha.toLowerCase(), "input binary matches its recorded identity");
for (const dir of [home, project, otherProject, path.join(profile, ".codex"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) {
  await mkdir(dir, { recursive: true });
}
await copyFile(path.resolve(binary), exe);
assert.equal(hash(await readFile(exe)), hash(sourceExe), "private executable copy matches input bytes");
await writeFile(path.join(home, "kanzei.toml"), "[models]\nprimary = 'fixture:unused'\nfast = 'fixture:unused'\nscout = 'fixture:unused'\ncompact = 'fixture:unused'\n");
const good = { projects: [project, otherProject], current: project,
  names: { [project]: "Existing A", [otherProject]: "Retained B" }, theme: "dark",
  open_tools: [{ id: "kept-tool", label: "Retained tool", command: "fixture-never-launched", args: ["{path}"] }],
  workspace_state: { [project]: { space: "dev" }, [otherProject]: { space: "research" } } };
const goodBytes = Buffer.from(JSON.stringify(good));
const damaged = [
  ["invalid_utf8", Buffer.concat([goodBytes, Buffer.from([0xff])])],
  ["invalid_json", Buffer.from(JSON.stringify(good).slice(0, -1))],
  ["invalid_typed_field", Buffer.from(JSON.stringify({ ...good, auto_max: "not-a-u32" }))],
];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
const env = { ...process.env, KANZEI_HOME: home, USERPROFILE: profile, HOME: profile,
  CODEX_HOME: path.join(profile, ".codex"), LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), KANZEI_SCHEDULER: "off" };
for (const key of ["KANZEI_EMBEDDED_RUNTIME", "KANZEI_E2E_CDP", "WEBVIEW2_USER_DATA_FOLDER", "KANZEI_PROJECT_ROOT"]) delete env[key];

function rpc(endpoint, input) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port: Number(endpoint.addr.split(":").at(-1)) });
    let bytes = Buffer.alloc(0);
    socket.setTimeout(10000, () => socket.destroy(new Error("fixture native IPC timeout")));
    socket.on("error", reject);
    socket.on("connect", () => {
      const payload = Buffer.from(JSON.stringify({ ...input, token: endpoint.token })), header = Buffer.alloc(4);
      header.writeUInt32BE(payload.length); socket.write(Buffer.concat([header, payload]));
    });
    socket.on("data", chunk => {
      bytes = Buffer.concat([bytes, chunk]);
      if (bytes.length < 4 || bytes.length < 4 + bytes.readUInt32BE(0)) return;
      try {
        const result = JSON.parse(bytes.subarray(4, 4 + bytes.readUInt32BE(0)));
        socket.destroy(); resolve(result);
      } catch (error) { socket.destroy(); reject(error); }
    });
  });
}
const invoke = (endpoint, command, args = {}) => rpc(endpoint, { action: "invoke", command, args });
async function endpointForOwnProcess(pid) {
  for (const dir of await readdir(path.join(home, "runtime"), { withFileTypes: true }).catch(() => [])) {
    if (!dir.isDirectory()) continue;
    const endpoint = JSON.parse(await readFile(path.join(home, "runtime", dir.name, "endpoint.json"), "utf8").catch(() => "null"));
    if (endpoint?.pid === pid && path.resolve(endpoint.executable).toLowerCase() === exe.toLowerCase()) return endpoint;
  }
  return null;
}
function ownProcessTree(pid) {
  const command = `$taskRows=@(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name);$taskOwned=@(${pid});do{$taskBefore=$taskOwned.Count;$taskOwned+=@($taskRows | Where-Object {$taskOwned -contains $_.ParentProcessId -and $taskOwned -notcontains $_.ProcessId} | ForEach-Object {$_.ProcessId});}while($taskOwned.Count -gt $taskBefore);@($taskRows | Where-Object {$taskOwned -contains $_.ProcessId} | Select-Object ProcessId,ParentProcessId,Name) | ConvertTo-Json -Compress`;
  const rows = JSON.parse(execFileSync("powershell", ["-NoProfile", "-Command", command], { windowsHide: true, encoding: "utf8" }) || "[]");
  return Array.isArray(rows) ? rows : [rows];
}
const rows = [], errors = [], failures = [];
async function test(name, check) {
  const row = { name };
  rows.push(row);
  try { await check(row); row.assertion = "PASS"; console.log(`PASS ${name}`); }
  catch (error) {
    row.assertion = "FAIL"; row.assertion_error = String(error); failures.push(name);
    if (error?.code !== "ERR_ASSERTION") errors.push(`${name}: ${String(error)}`);
    console.log(`FAIL ${name}: ${error.stack}`);
  }
}
const jsonPrefs = async () => JSON.parse(await readFile(prefsFile, "utf8"));
const maybeRead = file => readFile(file).catch(error => { if (error.code === "ENOENT") return null; throw error; });
let child, endpoint, owned = [], hello, stdout = "", stderr = "", shutdown, exit;
try {
  child = spawn(exe, ["--runtime-service"], { cwd: project, env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  child.stdout.on("data", bytes => { stdout += bytes; }); child.stderr.on("data", bytes => { stderr += bytes; });
  child.on("error", error => errors.push(String(error)));
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    endpoint = await endpointForOwnProcess(child.pid);
    if (endpoint) {
      const ping = await rpc(endpoint, { action: "ping" }).catch(() => null);
      const ready = await invoke(endpoint, "ui_prefs_get").catch(() => null);
      if (ping?.ok && ready?.ok) { hello = ping.value; break; }
    }
    if (child.exitCode !== null) throw new Error(`fixture exited before IPC ready: ${child.exitCode}`);
    await delay(150);
  }
  assert(hello && endpoint?.pid === child.pid, "self fixture PID and native service IPC ready");
  owned = ownProcessTree(child.pid);
  console.log(`BINARY SHA256=${hash(sourceExe)} PID=${child.pid} native_build=${hello.build} provenance=${provenance}`);
  await test("normal_theme_keeps_unrelated_fields", async row => {
    await writeFile(prefsFile, goodBytes);
    row.reply = await invoke(endpoint, "ui_prefs_set", { theme: "light" });
    const after = await jsonPrefs();
    assert(row.reply.ok); assert.equal(after.theme, "light");
    for (const field of ["projects", "names", "open_tools", "workspace_state"]) assert.deepEqual(after[field], good[field]);
  });
  const writers = [
    ["ui_prefs_set", { theme: "light" }],
    ["projects_remove", { path: project }],
  ];
  for (const [command, args] of writers) {
    for (const [damage, bytes] of damaged) {
      await test(`${command}_${damage}_keeps_original_bytes`, async row => {
        await writeFile(prefsFile, bytes);
        const getter = await invoke(endpoint, "projects_get");
        row.getter_ok = getter.ok; row.getter_projects = getter.value?.projects;
        row.reply = await invoke(endpoint, command, args);
        const after = await readFile(prefsFile);
        row.before_sha256 = hash(bytes); row.after_sha256 = hash(after);
        await writeFile(path.join(run, `${command}_${damage}.before`), bytes);
        await writeFile(path.join(run, `${command}_${damage}.after`), after);
        assert.deepEqual(after, bytes, "failed original read must not publish default preferences");
        assert.equal(row.reply.ok, false, "read failure must return an explicit rejection");
        assert.match(row.reply.value, /(?:读取|解析)偏好文件.*app\.json/);
        assert.equal(getter.ok, true); assert.deepEqual(getter.value.projects, []);
      });
    }
  }
  await test("not_found_initializes_preferences", async row => {
    await rm(prefsFile, { force: true });
    row.reply = await invoke(endpoint, "ui_prefs_set", { theme: "light" });
    const after = await jsonPrefs();
    assert(row.reply.ok); assert.equal(after.theme, "light"); assert.deepEqual(after.projects, []);
  });
  await test("normal_project_remove_preserves_other_project_and_files", async row => {
    await writeFile(prefsFile, goodBytes);
    await writeFile(path.join(project, "owned-by-user.txt"), "keep user file");
    row.reply = await invoke(endpoint, "projects_remove", { path: project });
    const after = await jsonPrefs();
    assert(row.reply.ok); assert.deepEqual(after.projects, [otherProject]); assert.equal(after.current, otherProject);
    assert.deepEqual(after.names, { [otherProject]: good.names[otherProject] });
    assert.deepEqual(after.workspace_state, { [otherProject]: good.workspace_state[otherProject] });
    assert.deepEqual(after.open_tools, good.open_tools);
    assert.equal(await readFile(path.join(project, "owned-by-user.txt"), "utf8"), "keep user file");
  });
  const exportRoot = path.join(run, "exports"), relative = ".kanzei/project/requirements.md";
  await mkdir(path.dirname(path.join(project, relative)), { recursive: true });
  await writeFile(path.join(project, relative), "FIRST_SNAPSHOT");
  let camelExport;
  await test("actual_ui_camel_export_payload", async row => {
    row.reply = await invoke(endpoint, "export_project_data", { options: {
      projectDir: project, outputDir: exportRoot, includeMemory: false, includeRequirements: true, includeDefects: false, includeConfig: false,
    } });
    assert(row.reply.ok, `real UI payload must deserialize: ${JSON.stringify(row.reply)}`);
    camelExport = row.reply.value.path;
    assert.deepEqual(row.reply.value.files, [relative]);
    assert.equal(await readFile(path.join(camelExport, relative), "utf8"), "FIRST_SNAPSHOT");
  });
  await test("later_and_empty_exports_preserve_each_successful_bundle", async row => {
    // Separate snake payloads keep this ownership test independent of camel compatibility.
    const snake = { project_dir: project, output_dir: exportRoot, include_memory: false, include_requirements: true,
      include_defects: false, include_config: false };
    await writeFile(path.join(project, relative), "SNAKE_FIRST");
    const first = await invoke(endpoint, "export_project_data", { options: snake });
    assert(first.ok);
    await writeFile(path.join(project, relative), "SNAKE_SECOND");
    const second = await invoke(endpoint, "export_project_data", { options: snake });
    assert(second.ok);
    const empty = await invoke(endpoint, "export_project_data", { options: { ...snake, include_requirements: false } });
    row.replies = { first, second, empty };
    const firstBytes = await maybeRead(path.join(first.value.path, relative));
    const secondBytes = await maybeRead(path.join(second.value.path, relative));
    row.first_bytes = firstBytes?.toString() ?? null; row.second_bytes = secondBytes?.toString() ?? null;
    assert.notEqual(first.value.path, second.value.path, "successful exports must reserve distinct directories");
    assert.equal(empty.ok, false); assert.match(empty.value, /没有可导出的工作资料/);
    assert.equal(firstBytes?.toString(), "SNAKE_FIRST"); assert.equal(secondBytes?.toString(), "SNAKE_SECOND");
    if (camelExport) assert.equal(await readFile(path.join(camelExport, relative), "utf8"), "FIRST_SNAPSHOT");
    assert.equal((await readdir(exportRoot)).length, camelExport ? 3 : 2);
  });
} catch (error) {
  errors.push(String(error)); console.log(`EXECUTION_FAULT ${error.stack}`);
} finally {
  if (endpoint) shutdown = await rpc(endpoint, { action: "shutdown" }).catch(error => ({ error: String(error) }));
  if (child) {
    const deadline = Date.now() + 20000;
    while (child.exitCode === null && Date.now() < deadline) await delay(100);
    if (child.exitCode === null) { errors.push("graceful own service shutdown timeout; child killed"); child.kill(); await delay(500); }
    exit = child.exitCode;
  }
  const deadline = Date.now() + 10000;
  while (owned.some(row => alive(row.ProcessId)) && Date.now() < deadline) await delay(100);
  const remaining = owned.filter(row => alive(row.ProcessId));
  if (remaining.length) errors.push(`own descendants remain: ${JSON.stringify(remaining)}`);
  if (shutdown?.ok !== true || exit !== 0) errors.push(`native shutdown failed: ${JSON.stringify(shutdown)}, exit=${exit}`);
  await writeFile(path.join(run, "stdout.log"), stdout); await writeFile(path.join(run, "stderr.log"), stderr);
  const result = { fixture_dir: run, binary: { source: path.resolve(binary), sha256: hash(sourceExe), bytes: sourceExe.length,
    last_write_time_utc: sourceStat.mtime.toISOString(), copy: exe, copy_sha256: hash(await readFile(exe)), provenance },
    service: { pid: child?.pid, hello, shutdown, exit, owned_processes: owned, remaining_processes: remaining },
    rows, failures, errors, pass: rows.filter(row => row.assertion === "PASS").length,
    fail: rows.filter(row => row.assertion === "FAIL").length };
  await writeFile(path.join(evidence, "native-result.json"), JSON.stringify(result, null, 2));
  console.log(`RESULT pass=${result.pass} fail=${result.fail} execution_errors=${errors.length} app_exit=${exit} remaining=${remaining.length}`);
  process.exitCode = errors.length ? 2 : failures.length ? 1 : 0;
}
