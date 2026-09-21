// Pure voice primitives: incremental text, PCM framing and microphone endpointing.
export function speechText(text) {
  return text.replace(/`[^`]*`/g, " ").replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/https?:\/\/\S+/g, " ")
    .replace(/^[\s#>*+-]+/gm, "").replace(/[*_`~|]/g, "").replace(/\s+/g, " ").trim();
}

export class SpeechSegmenter {
  constructor() { this.pending = ""; this.text = ""; this.fence = false; }
  push(delta, final = false) {
    this.pending += delta;
    const result = [];
    const flush = () => { const text = speechText(this.text); this.text = ""; if (/[\p{L}\p{N}]/u.test(text)) result.push(text); };
    while (this.pending.length && (final || this.pending.length >= 3)) {
      if (this.pending.startsWith("```") || this.pending.startsWith("~~~")) {
        this.fence = !this.fence;
        this.pending = this.pending.slice(3);
        continue;
      }
      const char = this.pending[0];
      this.pending = this.pending.slice(1);
      if (this.fence) continue;
      this.text += char;
      const end = /[。！？!?；;\n]/.test(char) || (char === "." && (!this.pending || /^\s/.test(this.pending)));
      if (end || (this.text.length >= 140 && /[，,、\s]/.test(char)) || this.text.length >= 240) flush();
    }
    if (final) flush();
    return result;
  }
}

export class Pcm16Decoder {
  constructor() { this.carry = null; }
  push(bytes) {
    let input = bytes;
    if (this.carry !== null) { input = new Uint8Array(bytes.length + 1); input[0] = this.carry; input.set(bytes, 1); }
    const count = Math.floor(input.length / 2);
    const values = new Float32Array(count);
    for (let i = 0; i < count; i++) {
      const value = input[i * 2] | (input[i * 2 + 1] << 8);
      values[i] = (value >= 32768 ? value - 65536 : value) / 32768;
    }
    this.carry = input.length % 2 ? input[input.length - 1] : null;
    return values;
  }
  finish() { if (this.carry !== null) throw new Error("Incomplete PCM sample"); }
}

export function wav16(frames) {
  const length = frames.reduce((sum, frame) => sum + frame.length, 0);
  const buffer = new ArrayBuffer(44 + length * 2);
  const view = new DataView(buffer);
  const label = (offset, text) => { for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i)); };
  label(0, "RIFF"); view.setUint32(4, 36 + length * 2, true); label(8, "WAVE"); label(12, "fmt ");
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, 16000, true); view.setUint32(28, 32000, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  label(36, "data"); view.setUint32(40, length * 2, true);
  let index = 44;
  for (const frame of frames) for (const sample of frame) {
    const value = Number.isFinite(sample) ? Math.max(-1, Math.min(1, sample)) : 0;
    view.setInt16(index, Math.round(value * (value < 0 ? 32768 : 32767)), true); index += 2;
  }
  return new Uint8Array(buffer);
}

export function bytesToBase64(bytes) {
  let result = "";
  for (let i = 0; i < bytes.length; i += 8192) result += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(result);
}

export class SpeechGate {
  constructor({ onStart, onEnd }) { this.onStart = onStart; this.onEnd = onEnd; this.noise = .003; this.reset(); }
  reset() { this.frames = []; this.pre = []; this.voiced = 0; this.silence = 0; this.duration = 0; this.active = false; }
  push(frame, playback = false) {
    const ms = frame.length / 16;
    let sum = 0;
    for (const value of frame) sum += value * value;
    const rms = Math.sqrt(sum / Math.max(1, frame.length));
    const threshold = Math.max(playback ? .025 : .012, this.noise * (playback ? 5 : 3));
    const voice = rms > threshold;
    if (!this.active) {
      this.pre.push(frame);
      if (this.pre.length > 16) this.pre.shift();
      if (!voice && rms < .02) this.noise = this.noise * .98 + rms * .02;
      this.voiced = voice ? this.voiced + ms : 0;
      if (this.voiced < (playback ? 260 : 220)) return rms;
      this.active = true; this.frames = this.pre; this.pre = []; this.duration = this.frames.reduce((n, f) => n + f.length / 16, 0);
      this.onStart();
      return rms;
    }
    this.frames.push(frame); this.duration += ms;
    this.silence = voice ? 0 : this.silence + ms;
    if (this.silence >= 700 || this.duration >= 29900) {
      const frames = this.frames; this.reset(); this.onEnd(frames);
    }
    return rms;
  }
}
