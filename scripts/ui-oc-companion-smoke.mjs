import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { createOcStateStore } from "../crates/kanzei-app/ui/22-oc-companion.js";
import { createOcDirector, ocMouthFrame } from "../crates/kanzei-app/ui/22-oc-director.js";
import { ocPerformanceMarkup } from "../crates/kanzei-app/ui/22-oc-performance.js";
import { OC_REFERENCE_SRC, OC_CHARACTER_PACK_SRC } from "../crates/kanzei-app/ui/22-oc-config.js";

let active = "a";
let clock = 1000;
const runtimes = new Map([["a", { phase: "idle", running: false }]]);
const store = createOcStateStore({
  getSessionId: () => active,
  getRuntime: (id) => runtimes.get(id),
  now: () => clock,
});
const emit = (type, id = "a", detail = {}) => store.emit(type, { session_id: id, ...detail });

assert.equal(store.current(), "idle");
runtimes.set("a", { phase: "running", running: true });
assert.equal(store.current(), "thinking", "恢复运行中的会话应有思考反馈，即使还没收到新动画事件");
emit("run_started");
emit("tool_started", "a", { tool_call_id: "read" });
emit("tool_started", "a", { tool_call_id: "search" });
emit("tool_completed", "a", { tool_call_id: "read" });
assert.equal(store.current(), "executing", "并行工具尚未全部结束时，光点应继续流动");
emit("assistant_streaming");
assert.equal(store.current(), "executing", "文本流不应掩盖仍在执行的工具");
emit("tool_completed", "a", { tool_call_id: "search" });
assert.equal(store.current(), "thinking");
emit("assistant_streaming");
assert.equal(store.current(), "replying");

runtimes.set("b", { phase: "running", running: true });
emit("tool_started", "b", { tool_call_id: "build" });
assert.equal(store.current(), "replying", "后台会话不得改变当前人物状态");
active = "b";
assert.equal(store.current(), "executing", "切回后台会话应恢复它自己的执行光效");
runtimes.set("b", { phase: "idle", running: false, converged: true });
assert.equal(store.current(), "idle", "即使没有动画终态事件，运行真源也应让人物收住");
emit("tool_started", "b", { tool_call_id: "late" });
assert.equal(store.current(), "idle", "迟到进度不得让终态人物重新活动");

active = "a";
emit("run_completed");
runtimes.set("a", { phase: "auto_pending", running: false, converged: true });
assert.equal(store.current(), "complete");
clock += 1801;
assert.equal(store.current(), "idle", "轮间等待必须在收尾后恢复待机");
runtimes.set("a", { phase: "running", running: true });
emit("run_started");
emit("reasoning_active");
assert.equal(store.current(), "thinking");
runtimes.set("a", { phase: "stopping", running: true });
assert.equal(store.current(), "idle", "发出停止意图后立即收住动画");
emit("assistant_streaming");
assert.equal(store.current(), "idle");
runtimes.set("a", { phase: "failed", running: false, converged: true });
assert.equal(store.current(), "blocked");
runtimes.set("a", { phase: "running", running: true });
emit("run_started");
assert.equal(store.current(), "thinking", "新一轮任务必须清除上一轮失败表现");
active = null;
assert.equal(store.current(), "idle");
assert.deepEqual(runtimes.get("a"), { phase: "running", running: true }, "动画不得改写业务状态");
const uiUrl = new URL("../crates/kanzei-app/ui/", import.meta.url);
const reference = await readFile(new URL(OC_REFERENCE_SRC, uiUrl));
assert(ocPerformanceMarkup().includes(`src="${OC_REFERENCE_SRC}"`));
const packUrl = new URL(OC_CHARACTER_PACK_SRC, uiUrl);
const pack = JSON.parse(await readFile(packUrl, "utf8"));
assert.equal(pack.format, "kanzei.character-pack.v3");
assert.equal(createHash("sha256").update(reference).digest("hex"), pack.posterSha256);
assert.deepEqual([reference.readUInt32BE(16),reference.readUInt32BE(20)],[1024,1536]);
for(const name of ["idle","listening","thinking","replying","executing","blocked","complete","aside","warm"]){
  assert(pack.states[name]?.clips.length,name+" has authored clips");
  for(const id of pack.states[name].clips)assert(pack.clips[id]);
}
for(const [id,clip] of Object.entries(pack.clips)){
  const movie=await readFile(new URL(clip.file,packUrl));
  assert.equal(movie.toString("ascii",4,8),"ftyp",id+" is an MP4");
  assert.equal(createHash("sha256").update(movie).digest("hex"),clip.sha256);
  const trackBytes=await readFile(new URL(clip.tracking,packUrl));
  assert.equal(createHash("sha256").update(trackBytes).digest("hex"),clip.trackingSha256);
  const tracking=JSON.parse(trackBytes);
  assert.equal(tracking.fps,24);assert.equal(tracking.frames,clip.frames);
  assert.equal(tracking.mouth.length,clip.frames);
  assert(clip.start>=0&&clip.end>clip.start&&clip.end<=clip.frames/24);
  assert(tracking.mouth.every(row=>row.length===5&&row.every(Number.isFinite)&&row[0]>0&&row[0]<1&&row[1]>0&&row[1]<1));
  for(const edge of [clip.next,clip.exit].filter(Boolean))assert(pack.clips[edge]);
  for(const [a,b] of clip.protected||[])assert(a>=clip.start&&b<=clip.end&&b>a);
}
const director=createOcDirector(pack);
director.setState("replying");director.advance(1400);
director.setState("thinking");director.setState("complete");
assert.equal(director.sample().clip,"reply-enter");
const seen=new Set();
for(let t=0;t<8000;t+=20)seen.add(director.advance(20).clip);
assert(seen.has("reply-exit"),"change state through the authored elbow recovery");
assert(seen.has("complete"));assert(!seen.has("thinking"),"latest request wins");
const idle=createOcDirector(pack),variants=new Set();
for(let t=0;t<31000;t+=50)variants.add(idle.advance(50).clip);
assert.equal(variants.size,3,"three idle variants alternate without an extra procedural breath");
for(const bad of [NaN,-1,Infinity])assert.equal(ocMouthFrame(bad),0);
assert.equal(ocMouthFrame(1),3);
assert.equal(pack.demo.voice,"Kanzei OC CN C");
for(let i=0;i<pack.demo.cues.length;i++){
  const cue=pack.demo.cues[i];assert(cue.duration>0&&cue.at>=0&&cue.at+cue.duration<=pack.demo.duration);
  if(i)assert(pack.demo.cues[i-1].at+pack.demo.cues[i-1].duration<=cue.at);
}
console.log("OC passed: session isolation, complete-frame pack integrity, three idle variants, coherent elbow exit, C voice timing and mouth controls.");
