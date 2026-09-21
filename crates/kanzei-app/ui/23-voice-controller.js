import { SpeechGate, SpeechSegmenter, bytesToBase64, wav16 } from "./23-voice-core.js";
import { VoiceMicrophone, VoicePlayer, streamSpeech } from "./23-voice-audio.js";

const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

export class VoiceConversation {
  constructor(options) {
    Object.assign(this, options);
    this.enabled = false; this.starting = false; this.life = 0; this.epoch = 0; this.queue = []; this.worker = null;
    this.cancelFlight = Promise.resolve(); this.segmenter = new SpeechSegmenter(); this.accepting = false; this.responding = false;
    this.onState("off");
  }
  current(life = this.life) {
    const now = this.getTarget();
    return this.enabled && this.life === life && now.sessionId === this.target?.sessionId && now.project === this.target?.project;
  }
  async start() {
    if (this.enabled || this.starting) return;
    this.target = this.getTarget();
    if (!this.target.sessionId || !this.target.project) throw new Error("voice_no_session");
    const life = ++this.life; this.starting = true; this.onState("connecting");
    this.wireSession = `${this.target.sessionId}::voice::${crypto.randomUUID()}`;
    try {
      const status = await this.invoke("voice_status");
      if (life !== this.life) return;
      if (!status?.ready) throw new Error(status?.detail || "voice_service_unavailable");
      const context = this.createContext(); this.context = context; await context.resume();
      if (life !== this.life) { if (context.state !== "closed") await context.close(); return; }
      const createPlayer = this.createPlayer || ((...args) => new VoicePlayer(...args));
      this.player = createPlayer(context, (playing, level, caption) => {
        if (!this.current(life)) return;
        this.onSignal(this.target.sessionId, playing ? "speaking" : this.responding || this.worker ? null : "listening", level);
        if (playing) { this.onState("speaking"); if (caption) this.onCaption(caption, "assistant"); }
        else if (!this.recognizing && !this.gate?.active) this.onState(this.responding || this.worker ? "thinking" : "listening");
      });
      this.gate = new SpeechGate({onStart:() => {
        this.interrupt(true, true);
        this.onState("hearing"); this.onSignal(this.target.sessionId, "listening", 0);
      }, onEnd:frames => { void this.recognize(frames, life); }});
      this.microphone = this.createMicrophone(this.context, frame => {
        if (!this.current(life) || this.recognizing) return;
        // Without confirmed AEC, avoid recognizing our own speaker output.
        if (this.player.playing && !this.microphone.echoCancellation) { this.gate.reset(); return; }
        this.onLevel(this.gate.push(frame, this.player.playing));
      }, () => this.fail(new Error("voice_microphone_ended")));
      const microphone = this.microphone;
      await microphone.start();
      if (life !== this.life) { microphone.destroy(); return; }
      this.enabled = true; this.starting = false; this.accepting = true;
      this.onState("listening"); this.onSignal(this.target.sessionId, "listening", 0);
    } catch (error) { if (life === this.life) this.fail(error); }
  }
  interrupt(stopReply = true, preserveCapture = false) {
    this.epoch++; this.queue = []; this.worker = null; this.accepting = false; this.responding = false; this.segmenter = new SpeechSegmenter();
    this.player?.clear();
    if (!preserveCapture) this.gate?.reset();
    this.recognizing = false;
    const session = this.wireSession;
    const life = this.life;
    if (session) this.cancelFlight = this.cancelFlight.catch(() => {}).then(() => this.invoke("voice_cancel", {sessionId:session}));
    this.cancelFlight.catch(error => { if (this.enabled && this.life === life) this.fail(error); });
    const stopping = stopReply && this.current() && this.getTarget().running;
    if (stopping) this.expectingStop = true;
    this.stopReplyFlight = stopping ? Promise.resolve(this.stopReply(this.target)) : Promise.resolve();
    this.stopReplyFlight.catch(error => { if (this.enabled && this.life === life) this.fail(error); });
    if (this.enabled) { this.onSignal(this.target.sessionId, "interrupted", 0); this.onState("listening"); }
  }
  stop() {
    const target = this.target;
    this.enabled = false; this.starting = false; this.expectingStop = false; this.life++;
    this.interrupt(false); this.microphone?.destroy(); this.microphone = null;
    this.player?.destroy(); this.player = null; this.gate?.reset(); this.recognizing = false;
    const context = this.context; this.context = null;
    if (context && context.state !== "closed") void context.close().catch(() => {});
    if (target) this.onSignal(target.sessionId, null, 0);
    this.onLevel(0); this.onState("off");
  }
  fail(error) { this.stop(); this.onState("error", String(error?.message || error)); }
  async recognize(frames, life) {
    if (!this.current(life)) return;
    const epoch = this.epoch;
    const current = () => this.current(life) && epoch === this.epoch;
    this.recognizing = true; this.onState("recognizing");
    try {
      await this.stopReplyFlight; await this.cancelFlight;
      if (!current()) return;
      const result = await this.invoke("voice_transcribe", {sessionId:this.wireSession, requestId:crypto.randomUUID(), wav:bytesToBase64(wav16(frames))});
      if (!current()) return;
      const text = String(result?.text || "").trim();
      if (!text) { this.onState("listening"); return; }
      this.onCaption(text, "user");
      // The stop command acknowledges cancellation; wait for its UI projection too.
      const deadline = performance.now() + 5000;
      while (this.getTarget().running && current() && performance.now() < deadline) await delay(40);
      if (!current()) return;
      if (this.getTarget().running) throw new Error("voice_stop_pending");
      this.accepting = true; this.responding = true; this.onState("thinking"); this.onSignal(this.target.sessionId, null, 0);
      await this.sendText(text);
    } catch (error) { if (current()) this.fail(error); }
    finally { if (current()) this.recognizing = false; }
  }
  handle(type, payload) {
    if (!this.current() || payload.sessionId !== this.target.sessionId) return;
    if (type === "kz:turn") {
      if (payload.step === 1) {
        this.expectingStop = false;
        this.interrupt(false); this.accepting = true; this.responding = true; this.onState("thinking");
        this.onSignal(this.target.sessionId, null, 0);
      } else if (this.accepting) this.enqueue(this.segmenter.push("", true));
    } else if (type === "kz:text" && this.accepting) {
      this.responding = true; this.enqueue(this.segmenter.push(String(payload.text || "")));
    } else if (type === "kz:done" && this.accepting) {
      this.responding = false; this.enqueue(this.segmenter.push("", true));
      if (!this.worker && !this.player?.buffered) this.onState("listening");
    } else if (type === "kz:stopped" && this.expectingStop) {
      // Acknowledging the reply we interrupted must not cancel the new utterance.
      this.expectingStop = false;
    } else if (["kz:error", "kz:stopped"].includes(type)) {
      this.interrupt(false);
    }
  }
  enqueue(sentences) {
    if (!sentences.length) return;
    if (this.queue.length + sentences.length > 32) { this.fail(new Error("voice_queue_full")); return; }
    this.queue.push(...sentences); void this.pump();
  }
  async pump() {
    if (this.worker || !this.enabled) return;
    const owner = {}; const epoch = this.epoch; const life = this.life;
    this.worker = owner;
    const current = () => this.current(life) && this.epoch === epoch;
    try {
      await this.cancelFlight;
      while (current() && this.queue.length) {
        while (current() && this.player.buffered > 3) await delay(80);
        if (!current()) return;
        const text = this.queue.shift();
        await streamSpeech({invoke:this.invoke, Channel:this.Channel, player:this.player,
          sessionId:this.wireSession, requestId:crypto.randomUUID(), text, isCurrent:current});
      }
    } catch (error) { if (current()) this.fail(error); }
    finally {
      if (this.worker === owner) {
        this.worker = null;
        if (this.current(life) && !this.responding && !this.player.buffered) this.onState("listening");
      }
    }
  }
}

export function microphoneFor(context, onFrame, onEnded) { return new VoiceMicrophone(context, onFrame, onEnded); }
