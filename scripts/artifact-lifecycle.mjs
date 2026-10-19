// Engineering artifacts are owned by a run; retained evidence is independent of its environment.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { createHash, randomUUID } from "node:crypto";
import { execFile, execFileSync, spawn } from "node:child_process";
import { promisify } from "node:util";
import { once } from "node:events";
import { AsyncLocalStorage } from "node:async_hooks";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const exec = promisify(execFile);
const context = new AsyncLocalStorage();
export const POLICY = Object.freeze({ successfulRunsPerSuite: 3, failedRunDays: 7, localReleases: 2, cacheBudgetHeadroom: 1.5, minimumBuildFreeBytes: 2 * 1073741824 });
const markerName = ".kanzei-artifact-owner.json";
const day = 86400000;
const json = value => JSON.stringify(value, null, 2);
const write = (file, value) => fs.writeFileSync(file, json(value) + "\n");
export function containsPath(parent, child) {
  const relative = path.relative(parent, child);
  return relative === "" || (!path.isAbsolute(relative) && relative !== ".." && !relative.startsWith(".." + path.sep));
}
function canonical(value) { return fs.realpathSync.native(path.resolve(value)); }
function sha(file) { return createHash("sha256").update(fs.readFileSync(file)).digest("hex"); }
function repo(root) {
  const common = execFileSync("git", ["-C", root, "rev-parse", "--path-format=absolute", "--git-common-dir"], { encoding: "utf8" }).trim();
  return canonical(path.dirname(common));
}
export async function processSnapshot() {
  if (process.platform === "win32") {
    const command = "$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId;parent=$_.ParentProcessId;started=[math]::Floor($_.CreationDate.ToUniversalTime().Ticks/10000).ToString('F0');exe=$_.ExecutablePath;command=$_.CommandLine} }) | ConvertTo-Json -Compress";
    const { stdout } = await exec("powershell.exe", ["-NoProfile", "-Command", command], { windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
    return JSON.parse(stdout || "[]");
  }
  if (process.platform !== "linux") throw new Error("Artifact process identity supports Windows and Linux");
  const result = [];
  for (const name of fs.readdirSync("/proc")) {
    if (!/^\d+$/.test(name)) continue;
    try {
      const stat = fs.readFileSync("/proc/" + name + "/stat", "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      result.push({ pid: Number(name), parent: Number(fields[1]), started: fields[19], exe: fs.readlinkSync("/proc/" + name + "/exe"), command: fs.readFileSync("/proc/" + name + "/cmdline", "utf8").replaceAll("\0", " ") });
    } catch (error) { if (!["ENOENT", "ESRCH", "EACCES"].includes(error.code)) throw error; }
  }
  return result;
}
const identity = row => row && ({ pid: row.pid, started: row.started, exe: row.exe });
export function sameProcess(expected, actual) {
  return Boolean(expected && actual && expected.pid === actual.pid && expected.started === actual.started && expected.exe === actual.exe);
}
function alive(owner, snapshot) { return sameProcess(owner, snapshot.find(row => row.pid === owner?.pid)); }
function pathUsers(directory, snapshot) {
  const normalized = directory.toLowerCase();
  return snapshot.filter(row => (row.exe && containsPath(directory, row.exe)) || row.command?.toLowerCase().includes(normalized));
}
export function directorySize(directory) {
  let bytes = 0, files = 0;
  const walk = current => {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const file = path.join(current, entry.name), stat = fs.lstatSync(file);
      if (stat.isSymbolicLink()) continue;
      if (entry.isDirectory()) walk(file); else { bytes += stat.size; files++; }
    }
  };
  if (fs.existsSync(directory)) walk(directory);
  return { bytes, files };
}
const cargoLocks = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"];
export async function withCacheLocks(directory, action) {
  directory = canonical(directory);
  const profile = path.join(directory, "debug");
  fs.mkdirSync(profile, { recursive: true });
  if (canonical(profile) !== profile) throw new Error("Cargo cache profile traverses a filesystem link");
  const program = process.platform === "win32" ? "powershell.exe" : "flock";
  const arguments_ = process.platform === "win32"
    ? ["-NoProfile", "-File", fileURLToPath(new URL("./cache-lock.ps1", import.meta.url)), "-Directory", profile]
    : cargoLocks.flatMap((name,index) => ["--exclusive", "--nonblock", path.join(profile,name), ...(index < cargoLocks.length-1 ? ["flock"] : [process.execPath, "--input-type=module", "-e", "process.stdout.write('locked\\n');process.stdin.resume();process.stdin.on('end',()=>process.exit(0));"])]);
  const child = spawn(program, arguments_, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const closed = once(child, "close");
  let diagnostic = "";
  child.stderr.on("data", data => { diagnostic += data; });
  try {
    await new Promise((resolve,reject) => {
      let output="";
      child.stdout.on("data",data=>{output+=data;if(output.includes("locked\n")||output.includes("locked\r\n"))resolve();});
      child.once("error",reject);
      child.once("close",()=>reject(new Error("Cargo cache lock is held or unavailable: "+diagnostic)));
    });
    return await action();
  } finally {
    if(!child.stdin.destroyed)child.stdin.end();
    await closed;
  }
}
export class ArtifactStore {
  constructor(root = process.cwd()) {
    this.worktree = canonical(root);
    this.project = repo(this.worktree);
    this.directory = path.join(this.project, "output", ".lifecycle");
    this.evidence = path.join(this.project, "output", "evidence");
    fs.mkdirSync(this.directory, { recursive: true });
    fs.mkdirSync(this.evidence, { recursive: true });
    this.db = new DatabaseSync(path.join(this.directory, "registry.sqlite"));
    this.db.exec("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS artifacts(id TEXT PRIMARY KEY, kind TEXT NOT NULL, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS pins(reference TEXT NOT NULL, artifact TEXT NOT NULL, PRIMARY KEY(reference,artifact));");
    this.environments = this.transaction(() => {
      const registered = this.all("environment-root").find(root => root.active);
      if (registered && fs.existsSync(registered.path)) return registered.path;
      if (registered) { registered.active = false; this.put(registered); }
      const temporary = canonical(os.tmpdir());
      for (let parent = temporary; ; parent = path.dirname(parent)) {
        if (fs.existsSync(path.join(parent, ".git"))) throw new Error("Temporary environments must live outside Git working trees");
        if (parent === path.dirname(parent)) break;
      }
      const directory = fs.mkdtempSync(path.join(temporary, "kanzei-runs-"));
      const root = { id: "environment-root:" + directory, record_type: "environment-root", path: directory, project: this.project, active: true, created_at: Date.now(), reason: "Reusable project namespace for task-owned temporary environments outside Git discovery" };
      write(path.join(directory, markerName), { id: root.id, project: root.project, type: "environment-root" });
      this.put(root);
      return directory;
    });
  }
  close() { this.db.close(); }
  get(id) { const row = this.db.prepare("SELECT data FROM artifacts WHERE id=?").get(id); return row && JSON.parse(row.data); }
  all(kind) { return this.db.prepare("SELECT data FROM artifacts WHERE kind=?").all(kind).map(row => JSON.parse(row.data)); }
  put(record) { this.db.prepare("INSERT INTO artifacts VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data").run(record.id, record.record_type, JSON.stringify(record)); }
  transaction(action) {
    this.db.exec("BEGIN IMMEDIATE");
    try { const result = action(); this.db.exec("COMMIT"); return result; }
    catch (error) { this.db.exec("ROLLBACK"); throw error; }
  }
  pin(reference, ids) {
    this.transaction(() => this.replacePins(reference, ids));
  }
  replacePins(reference, ids) {
    for (const id of ids) { const run = this.get(id); if (!run || run.evidence_state !== "retained") throw new Error("Cannot pin missing or retiring evidence: " + id); }
    this.db.prepare("DELETE FROM pins WHERE reference=?").run(reference);
    for (const id of ids) this.db.prepare("INSERT INTO pins VALUES(?,?)").run(reference, id);
  }
  isPinned(id) { return Boolean(this.db.prepare("SELECT 1 FROM pins WHERE artifact=? LIMIT 1").get(id)); }
  assertOwned(run, directory, type) {
    const parent = type === "environment" ? path.dirname(run.environment) : run.evidence_root;
    if (type === "environment") {
      const root = this.all("environment-root").find(root => root.path === parent && root.project === this.project);
      if (!root) throw new Error("Environment root is not registered to this project");
      const marker = JSON.parse(fs.readFileSync(path.join(parent, markerName), "utf8"));
      if (marker.id !== root.id || marker.project !== this.project || marker.type !== "environment-root") throw new Error("Environment root ownership marker mismatch");
    }
    if (type === "evidence" && ![path.join(this.project, "output"), path.join(this.project, "dist")].some(base => containsPath(base, parent) && base !== parent)) throw new Error("Evidence root is outside the project's artifact namespaces");
    if (!containsPath(parent, directory) || directory === parent) throw new Error("Artifact path escapes owned root: " + directory);
    let current = directory;
    while (true) {
      if (fs.lstatSync(current).isSymbolicLink()) throw new Error("Artifact path traverses a link: " + current);
      if (current === path.dirname(current)) break;
      current = path.dirname(current);
    }
    const marker = JSON.parse(fs.readFileSync(path.join(directory, markerName), "utf8"));
    if (marker.run_id !== run.id || marker.project !== this.project || marker.type !== type) throw new Error("Artifact ownership marker mismatch: " + directory);
  }
  removeOwned(run, directory, type) {
    if (!fs.existsSync(directory)) return;
    this.assertOwned(run, directory, type);
    fs.rmSync(directory, { recursive: true });
  }
  assertCachePath(cache) {
    if (cache.project !== this.project || path.basename(cache.path) !== "target") throw new Error("Cache is not registered to this project");
    const roots = [this.project, cache.worktree].filter(root => fs.existsSync(root));
    if (!roots.some(root => repo(root) === this.project && containsPath(root, cache.path) && canonical(path.join(root, "target")) === cache.path)) throw new Error("Cache is outside its owning worktree");
  }
  export(run) {
    if (run.exported) return;
    this.assertOwned(run, run.environment, "environment");
    const files = [], omitted = [], excluded = new Set(["profile", "webview", "node_modules", "target", ".git", "tmp", "retired"]);
    if(run.state!=="success")excluded.delete("tmp");
    const copy = (source, relative) => {
      const stat = fs.lstatSync(source);
      if (stat.isSymbolicLink()) { omitted.push({ path: relative, reason: "link" }); return; }
      if (stat.isDirectory()) {
        if (excluded.has(path.basename(source))) { omitted.push({ path: relative, reason: "runtime-cache" }); return; }
        fs.mkdirSync(path.join(run.evidence, "raw", relative), { recursive: true });
        for (const name of fs.readdirSync(source)) copy(path.join(source, name), path.join(relative, name));
      } else {
        if (/^(kzapp|kz)\.exe$/i.test(path.basename(source)) || run.program_copies?.includes(source)) { omitted.push({ path: relative, reason: "program-copy" }); return; }
        const destination = path.join(run.evidence, "raw", relative);
        fs.mkdirSync(path.dirname(destination), { recursive: true });
        fs.copyFileSync(source, destination);
        files.push({ path: path.join("raw", relative), bytes: stat.size, sha256: sha(destination) });
      }
    };
    for (const name of fs.readdirSync(run.environment)) if (name !== markerName) copy(path.join(run.environment, name), name);
    write(path.join(run.evidence, "environment-manifest.json"), { source: run.environment, files, omitted });
    run.exported = true;
  }
  async begin({ kind, suite, ownerPid = process.pid, caches = [], evidenceNamespace = null }) {
    if (!/^[a-z0-9][a-z0-9._-]*$/i.test(suite)) throw new Error("Invalid artifact suite: " + suite);
    if (caches.length && process.env.CARGO_TARGET_DIR && !caches.map(canonical).includes(canonical(process.env.CARGO_TARGET_DIR))) throw new Error("Shared development commands require the registered target; use dev.ps1 -Isolated for a temporary target");
    const snapshot = await processSnapshot();
    await this.reap({ snapshot });
    for (const directory of caches.map(canonical)) {
      const cache = this.get("cache:" + directory);
      if (cache?.budget_bytes && directorySize(directory).bytes > cache.budget_bytes) await this.pruneIncrementalCache(directory);
    }
    const owner = identity(snapshot.find(row => row.pid === Number(ownerPid)));
    if (!owner?.exe) throw new Error("Cannot establish run owner identity: " + ownerPid);
    const id = Date.now() + "-" + randomUUID();
    const volume=fs.statfsSync(this.directory),available=volume.bavail*volume.bsize;
    const projectedGrowth=caches.map(canonical).reduce((total,directory)=>total+Math.max(0,(this.get("cache:"+directory)?.budget_bytes??0)-directorySize(directory).bytes),0);
    const required=caches.length?Math.max(POLICY.minimumBuildFreeBytes,projectedGrowth):0;
    if(available<required)throw new Error(`Build space check failed: ${available} bytes available, ${required} bytes reserved for projected cache growth`);
    const evidenceRoot = evidenceNamespace ? path.resolve(this.project, evidenceNamespace) : path.join(this.evidence, suite);
    if (![path.join(this.project, "output"), path.join(this.project, "dist")].some(base => containsPath(base, evidenceRoot) && base !== evidenceRoot)) throw new Error("Invalid evidence namespace");
    const run = { id, record_type: "run", kind, suite, project: this.project, worktree: this.worktree, owner, created_at: Date.now(), ended_at: null, state: "running", evidence_state: "retained", cleanup_state: "pending", exported: false, environment: fs.mkdtempSync(path.join(this.environments, "run-")), evidence_root: evidenceRoot, evidence: path.join(evidenceRoot, id), caches: caches.map(canonical), processes: [], inputs: [], commands: [] };
    run.commit = execFileSync("git", ["-C", this.worktree, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
    run.owner_command = snapshot.find(row=>row.pid===Number(ownerPid)).command;
    if(Number(ownerPid)===process.pid)run.commands.push(process.argv);
    run.configuration = { cargo_target_dir: process.env.CARGO_TARGET_DIR ?? path.join(this.worktree, "target"), rustflags: process.env.RUSTFLAGS ?? "", build_info: process.env.KANZEI_BUILD_INFO ?? "" };
    run.disk_check={available_bytes:available,required_bytes:required};
    this.transaction(() => {
      for (const directory of run.caches) {
        const id = "cache:" + directory, cache = this.get(id);
        if (cache?.state === "reclaiming") throw new Error("Build cache is being reclaimed: " + directory);
        this.put(cache ? { ...cache, last_used_at: Date.now() } : { id, record_type: "cache", path: directory, worktree: this.worktree, project: this.project, created_at: Date.now(), last_used_at: Date.now(), reason: "Reusable development build cache", state: "ready", budget_bytes: null });
      }
      this.put(run);
    });
    for (const [directory, type] of [[run.environment, "environment"], [run.evidence, "evidence"]]) {
      fs.mkdirSync(directory, { recursive: true });
      write(path.join(directory, markerName), { project: this.project, run_id: id, type, owner, created_at: run.created_at });
    }
    fs.mkdirSync(path.join(run.environment, "tmp"));
    write(path.join(run.evidence, "run.json"), run);
    return run;
  }
  async finish(id, state) {
    if (!["success", "failure", "interrupted"].includes(state)) throw new Error("Invalid terminal run state");
    const run = this.get(id);
    if (!run || run.state !== "running") throw new Error("Run is not active: " + id);
    const snapshot = await processSnapshot();
    const users = pathUsers(run.environment, snapshot).filter(row => row.pid !== process.pid && !sameProcess(run.owner, row));
    run.ended_at = Date.now();
    run.state = state;
    run.final_configuration = { cargo_target_dir: process.env.CARGO_TARGET_DIR ?? path.join(run.worktree,"target"), build_info:process.env.KANZEI_BUILD_INFO??"" };
    if (users.length) {
      run.state = "failure";
      run.cleanup_state = "blocked";
      run.cleanup_error = "Environment still has live process identities";
      run.blockers = users.map(identity);
      this.put(run); write(path.join(run.evidence, "run.json"), run);
      throw new Error("Environment retained because owned processes are still live: " + users.map(row => row.pid).join(","));
    }
    try {
      this.export(run);
      this.removeOwned(run, run.environment, "environment");
      run.cleanup_state = "removed";
      for (const directory of run.caches) {
        const cache = this.get("cache:" + directory), size = directorySize(directory);
        cache.observed_high_water_bytes = Math.max(cache.observed_high_water_bytes ?? 0, size.bytes);
        if (run.kind === "verification" && run.suite === "verify-full" && state === "success" && !cache.budget_bytes) {
          cache.budget_bytes = Math.ceil(cache.observed_high_water_bytes * POLICY.cacheBudgetHeadroom / 1073741824) * 1073741824;
          cache.calibration = { run_id: run.id, observed_high_water_bytes: cache.observed_high_water_bytes, headroom: POLICY.cacheBudgetHeadroom };
        }
        cache.last_size = size; this.put(cache);
      }
    } catch (error) { run.cleanup_state = "failed"; run.cleanup_error = String(error); run.state = "failure"; throw error; }
    finally { this.put(run); write(path.join(run.evidence, "run.json"), run); }
    return run;
  }
  async reap({ snapshot = null, now = Date.now() } = {}) {
    snapshot ??= await processSnapshot();
    const reclaimer = identity(snapshot.find(row => row.pid === process.pid));
    const removed = [], retained = [];
    this.transaction(() => {
      for (const cache of this.all("cache")) if (cache.state === "reclaiming" && !alive(cache.maintenance_owner,snapshot)) {
        cache.state="ready"; cache.interrupted_maintenance_at=now; delete cache.maintenance_owner; this.put(cache);
      }
    });
    for(const run of this.all("run"))if(run.evidence_state==="reclaiming"&&!alive(run.evidence_cleanup_owner,snapshot)){
      if(this.isPinned(run.id))throw new Error("Retiring evidence acquired a conflicting reference: "+run.id);
      this.removeOwned(run,run.evidence,"evidence");run.evidence_state="removed";delete run.evidence_cleanup_owner;this.put(run);
    }
    for (const original of this.all("run")) {
      if (original.cleanup_state === "removed" || alive(original.owner, snapshot) || alive(original.cleanup_owner, snapshot)) continue;
      const users = pathUsers(original.environment, snapshot);
      if (users.length) { retained.push({ id: original.id, reason: "live-process", processes: users.map(identity) }); continue; }
      const run = this.transaction(() => {
        const current = this.get(original.id);
        if (current.cleanup_state === "removed" || alive(current.cleanup_owner, snapshot)) return null;
        current.cleanup_owner = reclaimer; current.cleanup_state = "reclaiming"; this.put(current); return current;
      });
      if (!run) continue;
      try {
        if (run.state === "running") { run.state = "interrupted"; run.ended_at = now; }
        if (fs.existsSync(run.environment)) { this.export(run); this.removeOwned(run, run.environment, "environment"); }
        run.cleanup_state = "removed"; delete run.cleanup_owner; delete run.cleanup_error;
        this.put(run); write(path.join(run.evidence, "run.json"), run); removed.push(run.id);
      } catch (error) { run.cleanup_state = "failed"; run.cleanup_error = String(error); delete run.cleanup_owner; this.put(run); throw error; }
    }
    const groups = Map.groupBy(this.all("run").filter(run => run.state === "success"), run => run.suite);
    const keep = new Set([...groups.values()].flatMap(runs => runs.sort((a,b) => b.ended_at - a.ended_at).slice(0, POLICY.successfulRunsPerSuite).map(run => run.id)));
    for (const candidate of this.all("run")) {
      if (candidate.evidence_state !== "retained" || candidate.cleanup_state !== "removed" || candidate.state === "running" || alive(candidate.owner, snapshot) || keep.has(candidate.id)) continue;
      if (candidate.state !== "success" && now - candidate.ended_at <= POLICY.failedRunDays * day) continue;
      const run = this.transaction(() => {
        const current = this.get(candidate.id);
        if (this.isPinned(current.id) || current.evidence_state !== "retained") return null;
        current.evidence_state = "reclaiming"; current.evidence_cleanup_owner=reclaimer; this.put(current); return current;
      });
      if (!run) continue;
      this.removeOwned(run, run.evidence, "evidence");
      run.evidence_state = "removed"; delete run.evidence_cleanup_owner; this.put(run);
    }
    return { removed_environments: removed, retained };
  }
  async resetDevelopmentProfile(directory) {
    directory = canonical(directory);
    const cacheId = "cache:" + directory, snapshot = await processSnapshot();
    await this.reap({ snapshot });
    const cache = this.transaction(() => {
      const record = this.get(cacheId);
      if (!record || record.project !== this.project || path.basename(record.path) !== "target") throw new Error("Cache must be explicitly registered for this project");
      if (record.state !== "ready" || this.all("run").some(run => run.state === "running" && run.caches.includes(directory) && alive(run.owner, snapshot))) throw new Error("Build cache has active leases");
      const users = pathUsers(directory, snapshot).filter(row=>![process.pid,process.ppid].includes(row.pid));
      if (users.length) throw new Error("Build cache is in use by process " + users[0].pid);
      record.state = "reclaiming"; record.maintenance_owner=identity(snapshot.find(row=>row.pid===process.pid)); this.put(record); return record;
    });
    const debug = path.join(directory, "debug");
    try {
      this.assertCachePath(cache);
      if (fs.lstatSync(directory).isSymbolicLink() || (fs.existsSync(debug) && fs.lstatSync(debug).isSymbolicLink())) throw new Error("Cache deletion target is not a real project target directory");
      await withCacheLocks(directory, async () => {
        const before = directorySize(debug);
        for (const entry of fs.readdirSync(debug)) if (!cargoLocks.includes(entry)) fs.rmSync(path.join(debug,entry), { recursive: true });
        cache.last_reset = { at: Date.now(), directory: debug, before, after: directorySize(debug), reason: "Retire legacy development profile while preserving Cargo's lock identities" };
      });
      cache.observed_high_water_bytes = 0;
      cache.budget_bytes = null;
      return cache.last_reset;
    } finally { cache.state = "ready"; delete cache.maintenance_owner; this.put(cache); }
  }
  sample(id, phase) {
    const run = this.get(id);
    if (!run || run.state !== "running") throw new Error("Cannot sample an inactive task");
    const samples = run.caches.map(directory => {
      const cache = this.get("cache:" + directory), size = directorySize(directory);
      cache.observed_high_water_bytes = Math.max(cache.observed_high_water_bytes ?? 0, size.bytes); this.put(cache);
      return { phase, at: Date.now(), directory, ...size };
    });
    run.cache_samples = [...(run.cache_samples ?? []), ...samples]; this.put(run); return samples;
  }
  async pruneIncrementalCache(directory) {
    const snapshot = await processSnapshot(), id = "cache:" + directory;
    const cache = this.transaction(() => {
      const cache = this.get(id);
      if (!cache || cache.state !== "ready" || this.all("run").some(run => run.state === "running" && run.caches.includes(directory) && alive(run.owner, snapshot))) throw new Error("Over-budget cache has active leases; maintenance deferred");
      if (pathUsers(directory,snapshot).some(row=>![process.pid,process.ppid].includes(row.pid))) throw new Error("Over-budget cache is currently in use");
      cache.state = "reclaiming"; cache.maintenance_owner=identity(snapshot.find(row=>row.pid===process.pid)); this.put(cache); return cache;
    });
    try {
      const incremental = path.join(directory,"debug","incremental");
      this.assertCachePath(cache);
      if (fs.lstatSync(directory).isSymbolicLink() || (fs.existsSync(incremental) && fs.lstatSync(incremental).isSymbolicLink())) throw new Error("Incremental cache path is not an owned real directory");
      await withCacheLocks(directory, async () => {
        const before = directorySize(incremental);
        if (fs.existsSync(incremental)) fs.rmSync(incremental,{recursive:true});
        cache.last_prune = { at:Date.now(), before, after:directorySize(directory), budget_bytes:cache.budget_bytes };
      });
      if(cache.last_prune.after.bytes>cache.budget_bytes) throw new Error("Cache remains above its calibrated budget after incremental cleanup; retained dependencies require explicit cache retirement");
      return cache.last_prune;
    } finally { cache.state="ready";delete cache.maintenance_owner;this.put(cache); }
  }
  recordRelease(commit, installer, stage, verificationId = null, packageId = null) {
    if (!["packaged", "published", "installed"].includes(stage)) throw new Error("Invalid release stage");
    const record = this.transaction(() => {
      const id = "release:" + commit, previous = this.get(id);
      const sequence = previous?.sequence ?? Math.max(0, ...this.all("release").map(row => row.sequence ?? 0)) + 1;
      const record = { ...(previous ?? { id, record_type: "release", commit, created_at: Date.now(), run_ids: [] }), sequence, installer: path.resolve(installer), sha256: sha(installer) };
      if (previous && previous.sha256 !== record.sha256 && stage !== "packaged") throw new Error("Release installer differs from the packaged artifact");
      record[stage + "_at"] ??= Date.now();
      if(previous && previous.sha256===record.sha256 && fs.existsSync(previous.installer)) record.installer=previous.installer;
      for(const runId of [verificationId,packageId]) if (runId && !record.run_ids.includes(runId)) record.run_ids.push(runId);
      this.replacePins(id, record.run_ids);
      this.put(record);
      return record;
    });
    if (record.published_at && record.installed_at) {
      const releases = this.all("release").filter(row => row.published_at && row.sequence <= record.sequence).sort((a,b) => b.sequence - a.sequence);
      for (const old of releases.slice(POLICY.localReleases)) {
        if (!containsPath(this.project, old.installer)) throw new Error("Old installer is outside this project's managed artifact root");
        if (fs.existsSync(old.installer)) { if (sha(old.installer) !== old.sha256) throw new Error("Installer changed since registration: " + old.installer); fs.unlinkSync(old.installer); }
        this.pin(old.id, []); old.installer_removed_at = Date.now(); this.put(old);
      }
    }
    return record;
  }
}
export function currentTask() { return context.getStore(); }
async function stopPrivateProcesses(rows) {
  if(!rows.length)return;
  if(process.platform==="win32"){
    const payload=Buffer.from(JSON.stringify(rows.map(identity))).toString("base64");
    const command="$ErrorActionPreference='Stop'; $owned=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+payload+"')) | ConvertFrom-Json; foreach($item in $owned){ try{$ownedProcess=[Diagnostics.Process]::GetProcessById($item.pid)}catch [ArgumentException]{continue}; try{if($ownedProcess.HasExited){continue}; $started=[math]::Floor($ownedProcess.StartTime.ToUniversalTime().Ticks/10000).ToString('F0'); if($started -ne $item.started -or $ownedProcess.MainModule.FileName -ne $item.exe){throw 'Owned process identity changed'}; $ownedProcess.Kill(); $ownedProcess.WaitForExit()}finally{$ownedProcess.Dispose()} }";
    await exec("powershell.exe",["-NoProfile","-Command",command],{windowsHide:true});
  }else{
    const snapshot=await processSnapshot();
    for(const row of rows)if(alive(row,snapshot))process.kill(row.pid,"SIGTERM");
  }
}
export function spawnOwned(program, args, options = {}) {
  const task = currentTask();
  if(!task) throw new Error("spawnOwned requires a task owner");
  const env = { ...process.env, ...options.env, TEMP: path.join(task.environment, "tmp"), TMP: path.join(task.environment, "tmp"), TMPDIR: path.join(task.environment, "tmp") };
  const child = spawn(program, args, { ...options, env });
  task.children.push(child);
  return child;
}
export async function withTask({ suite, kind = "ui-test", root = process.cwd(), caches = [] }, action) {
  const store = new ArtifactStore(root), children = [], servers = new Set(), browsers = new Set();
  let run;
  try { run = await store.begin({ kind, suite, caches }); }
  catch (error) { store.close(); throw error; }
  const task = { ...run, children,
    server(server) { servers.add(server); return server; },
    async browser(promise) { const browser=await promise; browsers.add(browser); return browser; },
    async copyInput(source,destination) {
      task.input(source); await fs.promises.copyFile(source,destination);
      if(/\.(exe|dll)$/i.test(source)){run.program_copies=[...(run.program_copies??[]),path.resolve(destination)];store.put(run);}
    },
    input(file) { const entry = { path: canonical(file), bytes: fs.statSync(file).size, sha256: sha(file) }; run.inputs.push(entry); store.put(run); return entry; }
  };
  let failure;
  try { return await context.run(task, () => action(task)); }
  catch (error) { failure = error; throw error; }
  finally {
    const cleanups = await Promise.allSettled([
      ...[...browsers].map(browser=>browser.close()),
      ...[...servers].map(async server=>{ if(!server.listening)return;server.closeAllConnections?.();await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve())); }),
      ...children.map(async child => {
      if (!child.pid || child.exitCode !== null || child.signalCode !== null) return;
      const exited = once(child, "exit"); child.kill(); await exited;
    })]);
    const errors = cleanups.filter(item => item.status === "rejected").map(item => item.reason);
    try {
      const snapshot=await processSnapshot();
      const privateProcesses=snapshot.filter(row=>row.exe&&containsPath(run.environment,row.exe));
      run.processes=privateProcesses.map(identity);store.put(run);
      await stopPrivateProcesses(privateProcesses);
    } catch(error) { errors.push(error); }
    try { await store.finish(run.id, failure || errors.length ? "failure" : "success"); }
    catch (error) { errors.push(error); }
    store.close();
    if (errors.length) throw new AggregateError([...(failure ? [failure] : []), ...errors], "Task resource cleanup failed; evidence retained");
  }
}
async function main(args) {
  const [command, ...values] = args, store = new ArtifactStore();
  try {
    if (command === "begin") return await store.begin({ kind: values[0], suite: values[1], ownerPid: Number(values[2]), caches: values[3] && values[3]!=="-" ? [values[3]] : [], evidenceNamespace: values[4] || null });
    if (command === "finish") return await store.finish(values[0], values[1]);
    if (command === "reap") return await store.reap();
    if (command === "pin") { store.pin(values[0], values.slice(1)); return { reference: values[0], runs: values.slice(1) }; }
    if (command === "inspect") return { project: store.project, runs: store.all("run"), caches: store.all("cache"), releases: store.all("release") };
    if (command === "sample") return store.sample(values[0], values[1]);
    if (command === "reset-development-profile") return await store.resetDevelopmentProfile(values[0]);
    if (command === "release") return store.recordRelease(...values);
    if (command === "exec") {
      const [kind, suite, separator, program, ...arguments_] = values;
      if (separator !== "--" || !program) throw new Error("Usage: artifact-lifecycle exec <kind> <suite> -- <program> [arguments]");
      return await withTask({ kind, suite }, async task => {
        const child = spawnOwned(program, arguments_, { windowsHide: true, stdio: "inherit", env: { ...process.env, KANZEI_RUN_ENVIRONMENT: task.environment, KANZEI_RUN_EVIDENCE: task.evidence, TEMP: path.join(task.environment, "tmp"), TMP: path.join(task.environment, "tmp"), TMPDIR: path.join(task.environment, "tmp") } });
        const [code, signal] = await once(child, "exit");
        if (code !== 0) throw new Error("Command exited " + code + (signal ? " (" + signal + ")" : ""));
        return { run_id: task.id, evidence: task.evidence };
      });
    }
    throw new Error("Unknown lifecycle command: " + command);
  } finally { store.close(); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(result => process.stdout.write(json(result) + "\n"), error => { console.error(error); process.exitCode = 1; });
}
