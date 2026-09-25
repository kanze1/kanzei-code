import assert from "node:assert/strict";
import { OcClipDirector } from "../crates/kanzei-app/ui/22-oc-clip-director.js";

const pack={fps:24,transitionMs:180,aliases:{interrupted:"idle",stopping:"idle"},
  states:{idle:{clips:["rest","observe"]},replying:{clips:["enter"]},thinking:{clips:["think"]},complete:{clips:["nod"],once:true}},
  clips:{rest:{end:10},observe:{end:10},think:{end:6},nod:{end:4},
    enter:{end:3,protected:[[.5,3]],next:"hold",exit:"exit",exitAfter:3},
    hold:{end:2.5,next:"exit",exit:"exit"},exit:{end:2,protected:[[0,2]],next:"rest"}}};
const director=new OcClipDirector(pack);
director.advance(10100);
assert.equal(director.sample().clip,"observe");
assert.equal(director.sample().previous.clip,"rest");
assert(director.sample().blend>0&&director.sample().blend<1);
director.setState("replying");director.advance(1600);
director.setState("thinking");director.setState("complete");
assert.equal(director.sample().clip,"enter","elbow motion finishes before retargeting");
director.advance(1400);
assert.equal(director.sample().clip,"exit","queued target takes the authored arm exit");
director.advance(2050);
assert.equal(director.sample().state,"complete","latest target wins without playing superseded states");
director.setState("idle");director.advance(2000);
assert.equal(director.sample().state,"complete","finish one nod when the business indicator expires");
director.advance(2100);
assert.equal(director.sample().state,"idle");
const interrupt=new OcClipDirector(pack);
interrupt.setState("replying");interrupt.advance(3200);
assert.equal(interrupt.sample().clip,"hold");
interrupt.setState("interrupted");
assert.equal(interrupt.sample().clip,"exit","interruption leaves the held hand through a coherent full-frame exit");
assert.equal(interrupt.sample().playbackRate,1.5);
interrupt.advance(1500);
assert.equal(interrupt.sample().state,"idle");
const once=new OcClipDirector(pack);once.setState("complete");once.advance(4500);
for(let i=0;i<10;i++){once.setState("complete");once.advance(500);assert.equal(once.sample().state,"idle");}
for(const bad of [NaN,Infinity,-1]){const before=once.sample();assert.deepEqual(once.advance(bad),before);}
once.setState("unknown");assert.equal(once.sample().requested,"idle");
once.advance(60000);assert(Number.isFinite(once.sample().sourceTime));
const continuing=new OcClipDirector(pack);continuing.setState("replying");continuing.advance(7800);
assert.equal(continuing.sample().clip,"rest","a resting interval separates repeated gestures");
const gated={...pack,clips:{...pack.clips,rest:{end:10,protected:[[.5,6]]}}};
const jump=new OcClipDirector(gated),live=new OcClipDirector(gated);
for(const model of [jump,live]){model.advance(2400);model.setState("thinking");}
jump.advance(4300);for(let i=0;i<430;i++)live.advance(10);
assert.equal(jump.sample().clip,"think","queued state begins after the neutral exit gate");
assert.equal(jump.sample().sourceTime,.7);
assert.deepEqual(jump.sample(),live.sample(),"large export steps and realtime steps obey the same gate");
console.log("Whole-clip director passed: cycles, queued targets, authored elbow recovery, one-shot completion, interruption and invalid time.");
