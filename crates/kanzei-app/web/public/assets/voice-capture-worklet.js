/* global AudioWorkletProcessor, registerProcessor, sampleRate */
class KanzeiVoiceCapture extends AudioWorkletProcessor {
  constructor() { super(); this.phase = 0; this.sum = 0; this.count = 0; this.frame = new Float32Array(320); this.offset = 0; }
  process(inputs) {
    const samples = inputs[0]?.[0];
    if (!samples) return true;
    for (const value of samples) {
      this.sum += value; this.count++; this.phase += 16000;
      if (this.phase < sampleRate) continue;
      this.phase -= sampleRate; this.frame[this.offset++] = this.sum / this.count; this.sum = 0; this.count = 0;
      if (this.offset === 320) {
        this.port.postMessage(this.frame, [this.frame.buffer]); this.frame = new Float32Array(320); this.offset = 0;
      }
    }
    return true;
  }
}
registerProcessor("kanzei-voice-capture", KanzeiVoiceCapture);
