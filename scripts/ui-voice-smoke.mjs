import assert from "node:assert/strict";
import { SpeechSegmenter, Pcm16Decoder, SpeechGate, wav16 } from "../crates/kanzei-app/ui/23-voice-core.js";
import { VoiceConversation } from "../crates/kanzei-app/ui/23-voice-controller.js";
import { streamSpeech } from "../crates/kanzei-app/ui/23-voice-audio.js";

const segmenter = new SpeechSegmenter();
const text = "先看这里。\n```js\nsecret_code();\n```\n答案是 **3.14**。 You can continue.\n";
const sentences = [];
for (const char of text) sentences.push(...segmenter.push(char));
sentences.push(...segmenter.push("", true));
assert.deepEqual(sentences, ["先看这里。", "答案是 3.14。", "You can continue."]);
const decoder = new Pcm16Decoder();
assert.deepEqual([...decoder.push(new Uint8Array([0, 128, 255]))], [-1]);
assert.deepEqual([...decoder.push(new Uint8Array([127, 0, 0]))], [32767 / 32768, 0]);
decoder.finish(); decoder.push(new Uint8Array([0])); assert.throws(() => decoder.finish());
let starts = 0; let recording = null;
const gate = new SpeechGate({onStart:() => starts++, onEnd:frames => { recording = frames; }});
const quiet = new Float32Array(320); const speech = new Float32Array(320).fill(.1);
for (let i = 0; i < 100; i++) gate.push(quiet);
for (let i = 0; i < 4; i++) gate.push(speech);
for (let i = 0; i < 10; i++) gate.push(quiet);
assert.equal(starts, 0, "short click/noise must not interrupt");
for (let i = 0; i < 20; i++) gate.push(speech, true);
assert.equal(starts, 1);
for (let i = 0; i < 35; i++) gate.push(quiet);
assert.ok(recording.length > 20, "capture preserves speech before onset confirmation");
const wav = wav16(recording); const header = new DataView(wav.buffer);
assert.equal(header.getUint32(24, true), 16000); assert.equal(header.getUint32(40, true), wav.length - 44);

const tick = () => new Promise(resolve => setImmediate(resolve));
const calls = []; const requests = []; const transcripts = []; const played = []; const errors = [];
let target = {sessionId:"a", project:"project-a", processId:"main", running:false}; let micClosed = 0;
class Channel { onmessage() {} }
const controller = new VoiceConversation({
  getTarget:() => target, Channel,
  invoke:async (name, args) => {
    calls.push(name);
    if (name === "voice_start") return {ready:true};
    if (["voice_speak", "voice_transcribe"].includes(name)) return new Promise((resolve, reject) => requests.push({name,args,resolve,reject}));
  },
  sendText:async text => transcripts.push(text), stopReply:async () => { target.running = false; },
  createContext:() => ({state:"running",resume:async () => {},close:async () => {}}),
  createPlayer:() => ({buffered:0,playing:false,push:samples => played.push([...samples]),clear:() => {},destroy:() => {}}),
  createMicrophone:() => ({echoCancellation:true,start:async () => {},destroy:() => micClosed++}),
  onSignal:() => {},onCaption:() => {},onLevel:() => {},onState:(state,error) => { if (state === "error") errors.push(error); },
});
await controller.start();
controller.handle("kz:turn", {sessionId:"a",step:1});
controller.handle("kz:text", {sessionId:"b",text:"后台不应朗读。"});
controller.handle("kz:text", {sessionId:"a",text:"第一句。第二句。"});
controller.handle("kz:done", {sessionId:"a"});
await tick(); await tick();
assert.equal(requests.length, 1, "speech requests are sequential and session-scoped");
const first = requests.shift();
assert.equal(first.args.text, "第一句。");
controller.interrupt();
first.args.onChunk.onmessage({requestId:first.args.requestId,sessionId:first.args.sessionId,sequence:0,sampleRate:24000,pcm:"/38="});
first.resolve(); await tick();
assert.deepEqual(played, [], "late PCM after interruption is discarded");
assert.equal(requests.length, 0, "interruption discards queued sentences");

const recognition = controller.recognize([new Float32Array(3200)], controller.life);
await tick(); await tick();
const asr = requests.shift(); assert.equal(asr.name, "voice_transcribe");
target = {...target, sessionId:"b",project:"project-b"};
asr.resolve({text:"旧会话的识别结果"}); await recognition;
assert.deepEqual(transcripts, [], "late recognition never sends into a different conversation");
controller.stop(); assert.equal(micClosed, 1);
await controller.start();
const newer = controller.recognize([new Float32Array(3200)], controller.life);
await tick(); await tick();
const late = requests.shift(); controller.interrupt(); late.resolve({text:"打断前的录音"}); await newer;
assert.deepEqual(transcripts, [], "interrupt also invalidates in-flight recognition");
const accepted = controller.recognize([new Float32Array(3200)], controller.life);
await tick(); await tick();
requests.shift().resolve({text:"新的语音输入"}); await accepted;
assert.deepEqual(transcripts, ["新的语音输入"]);
target.running = true;
controller.interrupt(true, true);
const bargeIn = controller.recognize([new Float32Array(3200)], controller.life);
await tick(); await tick();
controller.handle("kz:stopped", {sessionId:target.sessionId});
requests.shift().resolve({text:"插话的新一句"}); await bargeIn;
assert.deepEqual(transcripts, ["新的语音输入", "插话的新一句"], "late stop acknowledgement preserves the new utterance");
assert.deepEqual(errors, []);
controller.stop();

// Finishing model startup after Stop must never acquire the microphone.
let completeStartup; let microphoneStarts = 0;
const originalInvoke = controller.invoke;
controller.invoke = (name, args) => name === "voice_start" ? new Promise(resolve => { completeStartup = resolve; }) : originalInvoke(name, args);
controller.createMicrophone = () => ({echoCancellation:true,start:async () => { microphoneStarts++; },destroy:() => {}});
const starting = controller.start();
assert.equal(controller.starting, true);
controller.stop();
completeStartup({ready:true}); await starting;
assert.equal(microphoneStarts, 0, "cancelled model startup must not open microphone later");
assert.equal(controller.enabled, false);

let audioChannel; let finished = false;
const stream = streamSpeech({
  invoke:async (_name, args) => { audioChannel = args.onChunk; }, Channel,
  player:{push:samples => played.push([...samples]), clear:() => {}}, sessionId:"voice-a",requestId:"request-a",text:"hello",isCurrent:() => true,
}).then(() => { finished = true; });
await tick(); assert.equal(finished, false, "command completion cannot overtake audio channel EOF");
audioChannel.onmessage({requestId:"request-a",sessionId:"voice-a",sequence:0,sampleRate:24000,pcm:"/38=",done:false});
audioChannel.onmessage({requestId:"request-a",sessionId:"voice-a",sequence:1,sampleRate:24000,pcm:"",done:true});
await stream; assert.equal(finished, true); assert.equal(played.at(-1)[0], 32767 / 32768);
console.log("Voice smoke passed: segmentation, PCM framing, VAD, queue ownership, cancellation and stale ASR isolation");
