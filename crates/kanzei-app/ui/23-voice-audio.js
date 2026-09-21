import { Pcm16Decoder } from "./23-voice-core.js";

export class VoicePlayer {
  constructor(context, onSignal) {
    this.context = context; this.onSignal = onSignal; this.nodes = new Set(); this.nextTime = 0; this.frame = null; this.lastSample = 0;
    this.analyser = context.createAnalyser(); this.analyser.fftSize = 512;
    this.volume = context.createGain(); this.volume.gain.value = .85;
    this.analyser.connect(this.volume).connect(context.destination);
    this.samples = new Float32Array(512);
  }
  get buffered() { return Math.max(0, this.nextTime - this.context.currentTime); }
  get playing() { const now = this.context.currentTime; return [...this.nodes].some(node => node.startAt <= now && node.endAt > now); }
  push(samples, rate = 24000, caption = "") {
    if (!samples.length) return;
    if (this.buffered + samples.length / rate > 90) throw new Error("Voice playback queue is full");
    const buffer = this.context.createBuffer(1, samples.length, rate); buffer.copyToChannel(samples, 0);
    const source = this.context.createBufferSource(); source.buffer = buffer; source.connect(this.analyser);
    // The first codec packet can contain only 80 ms; leave a small lead so the
    // following packet arrives before playback drains and splits the first word.
    source.startAt = Math.max(this.context.currentTime + (this.buffered > 0 ? .035 : .28), this.nextTime);
    source.endAt = source.startAt + buffer.duration; this.nextTime = source.endAt;
    source.caption = caption;
    this.nodes.add(source);
    source.onended = () => { this.nodes.delete(source); source.disconnect(); if (!this.nodes.size) this.onSignal(false, 0); };
    source.start(source.startAt);
    if (this.frame === null) this.frame = requestAnimationFrame(time => this.tick(time));
  }
  tick(time) {
    this.frame = null;
    if (time - this.lastSample >= 32) {
      this.lastSample = time;
      this.analyser.getFloatTimeDomainData(this.samples);
      let sum = 0; for (const value of this.samples) sum += value * value;
      const current = [...this.nodes].find(node => node.startAt <= this.context.currentTime && node.endAt > this.context.currentTime);
      this.onSignal(Boolean(current), current ? Math.min(1, Math.sqrt(sum / this.samples.length) * 9) : 0, current?.caption);
    }
    if (this.nodes.size) this.frame = requestAnimationFrame(next => this.tick(next));
  }
  clear() {
    for (const node of this.nodes) { node.onended = null; node.stop(); node.disconnect(); }
    this.nodes.clear(); this.nextTime = 0;
    if (this.frame !== null) cancelAnimationFrame(this.frame); this.frame = null;
    this.onSignal(false, 0);
  }
  destroy() { this.clear(); this.analyser.disconnect(); this.volume.disconnect(); }
}

export class VoiceMicrophone {
  constructor(context, onFrame, onEnded) { this.context = context; this.onFrame = onFrame; this.onEnded = onEnded; this.closed = false; this.echoCancellation = false; }
  async start() {
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({audio:{channelCount:1, echoCancellation:true, noiseSuppression:true, autoGainControl:true},video:false});
      if (this.closed) { this.stream.getTracks().forEach(track => track.stop()); return; }
      const track = this.stream.getAudioTracks()[0];
      this.echoCancellation = track.getSettings().echoCancellation === true;
      track.addEventListener("ended", () => { if (!this.closed) this.onEnded(); });
      await this.context.audioWorklet.addModule(new URL("./assets/voice-capture-worklet.js", import.meta.url));
      if (this.closed) return;
      this.source = this.context.createMediaStreamSource(this.stream);
      this.filter = this.context.createBiquadFilter(); this.filter.type = "lowpass"; this.filter.frequency.value = 7200;
      this.node = new AudioWorkletNode(this.context, "kanzei-voice-capture");
      this.silent = this.context.createGain(); this.silent.gain.value = 0;
      this.node.port.onmessage = event => { if (!this.closed) this.onFrame(event.data); };
      this.source.connect(this.filter).connect(this.node).connect(this.silent).connect(this.context.destination);
    } catch (error) { this.destroy(); throw error; }
  }
  destroy() {
    this.closed = true;
    this.stream?.getTracks().forEach(track => track.stop());
    if (this.node) { this.node.port.onmessage = null; this.node.port.close(); }
    for (const node of [this.source, this.filter, this.node, this.silent]) node?.disconnect();
  }
}

export async function streamSpeech({ invoke, Channel, player, sessionId, requestId, text, isCurrent }) {
  const decoder = new Pcm16Decoder();
  let sequence = 0; let failure = null;
  let endStream; let endTimer;
  const completed = new Promise(resolve => { endStream = resolve; });
  const channel = new Channel();
  channel.onmessage = packet => {
    if (!isCurrent() || failure) return;
    try {
      if (packet.requestId !== requestId || packet.sessionId !== sessionId || packet.sequence !== sequence++ || packet.sampleRate !== 24000) throw new Error("Voice stream sequence mismatch");
      if (packet.done) { decoder.finish(); endStream(); return; }
      const bytes = Uint8Array.from(atob(packet.pcm), char => char.charCodeAt(0));
      player.push(decoder.push(bytes), packet.sampleRate, text);
    } catch (error) { failure = error; player.clear(); endStream(); }
  };
  await invoke("voice_speak", {requestId, sessionId, text, onChunk:channel});
  if (!isCurrent()) return;
  try {
    await Promise.race([completed, new Promise((_, reject) => { endTimer = setTimeout(() => reject(new Error("Voice stream completion timed out")), 5000); })]);
  } finally { clearTimeout(endTimer); }
  if (failure) throw failure;
  if (isCurrent()) decoder.finish();
}
