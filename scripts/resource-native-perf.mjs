// Real native transport, unchanged production resource modules, isolated fixture data.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";

const config = JSON.parse(await fs.readFile(process.argv[2], "utf8"));
const endpoint = JSON.parse(await fs.readFile(config.endpointPath, "utf8"));
const result = { passed: false, checks: [], phases: {}, samples: {} };
let calls = 0, holdNext = null;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const check = (value, label) => { assert(value, label); result.checks.push(label); };
const metrics = async () => (await fetch(config.metrics)).json();
function rpc(command, args) {
  calls += 1;
  const request = Buffer.from(JSON.stringify({ action: "invoke", token: endpoint.token, command, args }));
  const prefix = Buffer.alloc(4); prefix.writeUInt32BE(request.length);
  const [host, port] = endpoint.addr.split(":");
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host, port: Number(port) });
    let buffer = Buffer.alloc(0), complete = false;
    socket.setTimeout(60000, () => socket.destroy(new Error("Native response timeout")));
    socket.on("connect", () => socket.write(Buffer.concat([prefix, request])));
    socket.on("error", reject);
    socket.on("end", () => { if (!complete) reject(new Error("Truncated native response")); });
    socket.on("data", chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      if (buffer.length < 4 || buffer.length < 4 + buffer.readUInt32BE(0)) return;
      complete = true; socket.end();
      try {
        const reply = JSON.parse(buffer.subarray(4, 4 + buffer.readUInt32BE(0)).toString());
        if (reply.ok) resolve(reply.value); else reject(new Error(String(reply.value)));
      } catch (error) { reject(error); }
    });
  });
}
globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
  const gate = holdNext; holdNext = null;
  const value = await rpc(command, args);
  if (gate) { gate.arrived(value); await gate.promise; }
  return value;
} }, event: { listen: async () => () => {} } } };
globalThis.document = { hidden: false };
const modulePath = name => path.join(config.repository, "crates/kanzei-app/ui", name);
const resources = await import(pathToFileURL(modulePath("01-project-resources.js")));
const { readProjectDocuments: read, invalidateProjectDocuments: invalidate, observeProjectDocuments: observe } = resources;
const project = config.project, resource = resources.projectDocuments(project);
const conventions = path.join(project, ".kanzei/project/conventions.md");
const calibrationMs = Number((await fs.readFile(modulePath("01-project-resources.js"), "utf8")).match(/interval:\s*(\d+)/)[1]);
const tickMs = Number((await fs.readFile(modulePath("00-refresh.js"), "utf8")).match(/},\s*(\d+)\);/)[1]);
const idleWindowMs = calibrationMs + 2 * tickMs;
result.configuration = { forcedSamples: 200, cacheSamples: 200, coalescingBatches: 40, callersPerBatch: 8,
  invalidationSamples: 20, calibrationMs, tickMs, idleWindowMs, percentile: "Nearest rank ceil(p*n), all samples retained" };
function distribution(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  return { n: samples.length, minMs: sorted[0], p50Ms: sorted[Math.ceil(sorted.length * 0.5) - 1],
    p95Ms: sorted[Math.ceil(sorted.length * 0.95) - 1], maxMs: sorted.at(-1),
    meanMs: samples.reduce((a, b) => a + b, 0) / samples.length };
}
async function phase(name, body) {
  console.log(`Starting ${name}`);
  const before = await metrics(), nativeBefore = calls, jsBefore = process.cpuUsage(), start = performance.now();
  const samples = await body();
  const wallMs = performance.now() - start, js = process.cpuUsage(jsBefore), after = await metrics();
  const native = Object.fromEntries(Object.keys(before).map(key => [key, after[key] - before[key]]));
  const cpuMs = native.cpuKernelMs + native.cpuUserMs;
  const summary = { wallMs, nativeCalls: calls - nativeBefore, native: { ...native, cpuMs,
    cpuPercentOneCore: 100 * cpuMs / wallMs }, harnessCpuMs: (js.user + js.system) / 1000 };
  if (samples?.length) { summary.latency = distribution(samples); result.samples[name] = samples; }
  result.phases[name] = summary;
  console.log(JSON.stringify({ phase: name, ...summary }));
  return summary;
}
async function timedReads(count, operation) {
  const samples = [];
  for (let index = 0; index < count; index += 1) {
    const start = performance.now(); await operation(index); samples.push(performance.now() - start);
  }
  return samples;
}
const hasMarker = (snapshot, marker) => snapshot?.conventions?.headings?.includes(marker);
async function mark(label) {
  const marker = `UI10-${label}-${Date.now()}`;
  await fs.appendFile(conventions, `\n# ${marker}\n`);
  return marker;
}
async function waitUntil(predicate, timeout) {
  const start = performance.now();
  while (!predicate()) {
    assert(performance.now() - start < timeout, "Observed freshness exceeded its measurement deadline");
    await sleep(20);
  }
  return performance.now() - start;
}
const disposers = [];
try {
  if (config.phase !== "events") {
  const first = await read(project);
  result.snapshot = { requirements: first.requirements.length, defects: first.defects.length, ideas: first.ideas.length,
    archived: first.archived, bytes: Buffer.byteLength(JSON.stringify(first)), warnings: first.warnings };
  check(first.requirements.length > 0 && first.archived.defect > 0, "Real project documents produce non-empty active and archive projections");
  for (let i = 0; i < 5; i += 1) await read(project, { force: true });
  await phase("forcedNative", () => timedReads(200, () => read(project, { force: true })));
  check(result.phases.forcedNative.nativeCalls === 200, "200 forced reads each reach native docs_snapshot");
  const current = resource.data;
  await phase("cached", () => timedReads(200, async () => assert.equal(await read(project), current)));
  check(result.phases.cached.nativeCalls === 0, "200 cache hits preserve the same snapshot object with zero IPC");
  await phase("coalesced", () => timedReads(40, async () => {
    invalidate(project);
    const batch = await Promise.all(Array.from({ length: 8 }, () => read(project)));
    assert(batch.every(item => item === batch[0]), "Concurrent subscribers share one snapshot object");
  }));
  check(result.phases.coalesced.nativeCalls === 40, "320 concurrent resource consumers coalesce into 40 actual native reads");
  await phase("explicitInvalidation", () => timedReads(20, async index => {
    const marker = await mark(`invalidate-${index}`);
    invalidate(project);
    assert(hasMarker(await read(project), marker), "Explicit invalidation must publish changed native bytes");
  }));
  check(result.phases.explicitInvalidation.nativeCalls === 20, "Each explicit invalidation returns the new document version");

  // Hold only delivery of an already completed real native response. No payload is fabricated.
  let release, arrived;
  const responseArrived = new Promise(resolve => { arrived = resolve; });
  holdNext = { arrived, promise: new Promise(resolve => { release = resolve; }) };
  invalidate(project); const beforeFence = calls, pending = read(project);
  await responseArrived;
  const fencedMarker = await mark("inflight"); invalidate(project); release();
  check(hasMarker(await pending, fencedMarker) && calls - beforeFence === 2,
    "Invalidating a real response in flight discards that revision and issues one latest native read");

  let active = true;
  const publications = [];
  const onChange = state => { if (state.status === "ready") publications.push({ at: performance.now(), data: state.data }); };
  const a = observe(project, onChange, () => active), b = observe(project, state => onChange(state), () => active);
  disposers.push(a, b);
  const calibrationMarker = await mark("missed-event");
  const staleStart = performance.now();
  await phase("visibleCalibration", async () => {
    await waitUntil(() => hasMarker(resource.data, calibrationMarker), calibrationMs + 5 * tickMs + result.phases.forcedNative.latency.maxMs);
  });
  result.calibration = { staleMs: performance.now() - staleStart, configuredIntervalMs: calibrationMs,
    clockGranularityMs: tickMs, expectedUncontendedBudgetMs: calibrationMs + tickMs + result.phases.forcedNative.latency.maxMs,
    scope: "Missed event; two visible observers; actual wall-clock timer. Budget is interval + timer granularity + observed max native read, not a guarantee under arbitrarily stalled event loops." };
  check(result.calibration.staleMs <= result.calibration.expectedUncontendedBudgetMs, "Missed-event freshness stays inside measured interval + tick + read budget");
  check(result.phases.visibleCalibration.nativeCalls === 1, "Two visible subscriptions share one calibration read");
  check(publications.length >= 4 && publications.at(-1).data === publications.at(-2).data,
    "Both subscribers receive the same refreshed snapshot object");
  active = false;
  await phase("inactiveIdle", () => sleep(idleWindowMs));
  check(result.phases.inactiveIdle.nativeCalls === 0, "Inactive subscriptions issue zero IPC for longer than one calibration interval");
  active = true; document.hidden = true;
  await phase("hiddenIdle", () => sleep(idleWindowMs));
  check(result.phases.hiddenIdle.nativeCalls === 0, "Hidden document issues zero IPC for longer than one calibration interval");
  const resumeMarker = await mark("resume"); const resumeStart = performance.now(); document.hidden = false;
  await waitUntil(() => hasMarker(resource.data, resumeMarker), tickMs * 5 + result.phases.forcedNative.latency.maxMs);
  result.resumeMs = performance.now() - resumeStart;
  check(hasMarker(resource.data, resumeMarker), "A visible owner reconciles overdue data after returning from hidden state");
  a(); b(); disposers.length = 0;
  await phase("unsubscribedIdle", () => sleep(idleWindowMs));
  check(result.phases.unsubscribedIdle.nativeCalls === 0, "Releasing the last owner stops calibration and idle IPC");
  }
  if (config.phase !== "core") {
  const { measureDocumentEvents } = await import("./resource-native-browser.mjs");
  await phase("browserToolEvent", async () => {
    result.browserEvent = await measureDocumentEvents({ project, rpc, mark });
    return result.browserEvent.samples;
  });
  check(result.browserEvent.samples.length === 20 && !result.browserEvent.errors.length,
    "20 production browser tool completion events load and render real changed native document snapshots");
  }
  result.totalNativeCalls = calls;
  result.passed = true;
  const summaries = [];
  if (result.phases.forcedNative) summaries.push(`Native docs_snapshot n=200 P50=${result.phases.forcedNative.latency.p50Ms.toFixed(2)}ms P95=${result.phases.forcedNative.latency.p95Ms.toFixed(2)}ms; cached 200/0 IPC; 320 concurrent consumers/40 IPC; explicit invalidation P95=${result.phases.explicitInvalidation.latency.p95Ms.toFixed(2)}ms; missed-event stale=${result.calibration.staleMs.toFixed(2)}ms; inactive/hidden/unsubscribed 22s each/0 IPC`);
  if (result.phases.browserToolEvent) summaries.push(`Production browser tool event n=20 P95=${result.phases.browserToolEvent.latency.p95Ms.toFixed(2)}ms`);
  result.summary = summaries.join("; ") + ".";
} catch (error) {
  result.error = String(error.stack || error); result.summary = "Real native resource acceptance failed"; throw error;
} finally {
  for (const dispose of disposers) dispose();
  await fs.writeFile(config.output, JSON.stringify(result, null, 2) + "\n");
}
