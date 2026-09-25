// Whole-character clips contain their own breathing, cloth motion and blinking.
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

export class OcClipDirector {
  constructor(pack) {
    this.pack = pack;
    this.group = this.requested = this.lastInput = "idle";
    this.time = this.elapsed = this.cycle = 0;
    this.previous = null;
    this.fade = 0;
    this.serial = 0;
    this.interrupted = false;
    this.clipId = this.choose("idle");
  }
  normalize(value) {
    const name = this.pack.aliases?.[value] || value;
    return Object.hasOwn(this.pack.states, name) ? name : "idle";
  }
  choose(state) {
    const sequence = this.pack.states[state].clips;
    return sequence[this.cycle % sequence.length];
  }
  get clip() { return this.pack.clips[this.clipId]; }
  get duration() { return (this.clip.end - (this.clip.start || 0)) * 1000; }
  get locked() {
    const seconds = this.elapsed / 1000 + (this.clip.start || 0);
    return (this.clip.protected || []).some(([start, end]) => seconds >= start && seconds < end);
  }
  setState(value) {
    if (this.lastInput === value) return;
    this.lastInput = value;
    const next = this.normalize(value);
    this.requested = next;
    this.interrupted = value === "interrupted" || value === "stopping";
    if (next === this.group) return;
    if (this.pack.states[this.group].once && next === "idle" && !this.interrupted) return;
    this.leaveIfReady();
  }
  startClip(id) {
    this.previous = { clipId: this.clipId, elapsed: this.elapsed, serial: this.serial };
    this.fade = 0;
    this.clipId = id;
    this.elapsed = 0;
    this.serial += 1;
  }
  arrive(state, nextVariant = false) {
    this.group = state;
    if (!nextVariant) this.cycle = 0;
    this.startClip(this.choose(state));
    this.interrupted = false;
  }
  leaveIfReady() {
    if (this.requested === this.group || this.locked) return false;
    if (this.pack.states[this.group].once && this.requested === "idle" && !this.interrupted &&
        this.elapsed < this.duration) return false;
    const seconds = this.elapsed / 1000 + (this.clip.start || 0);
    if (this.clip.exit && seconds >= (this.clip.exitAfter || 0)) this.startClip(this.clip.exit);
    else this.arrive(this.requested);
    return true;
  }
  advance(milliseconds) {
    let remaining = Number.isFinite(milliseconds) ? clamp(milliseconds, 0, 60000) : 0;
    this.time += remaining;
    while (remaining > 0) {
      const rate = this.interrupted && this.locked ? 1.5 : 1;
      // Cross every authored gate even when exporting/seeking in one large
      // step, so state changes happen at the same instant as live playback.
      const boundaries = (this.clip.protected || []).flat()
        .map(seconds => (seconds - (this.clip.start || 0)) * 1000)
        .filter(milliseconds => milliseconds > this.elapsed + .001);
      const boundary = Math.min(this.duration, ...boundaries);
      const step = Math.min(remaining, (boundary - this.elapsed) / rate);
      this.elapsed += step * rate;
      remaining -= step;
      this.fade += step;
      if (this.previous) {
        const previousClip = this.pack.clips[this.previous.clipId];
        this.previous.elapsed = Math.min(this.previous.elapsed + step,
          (previousClip.end - (previousClip.start || 0)) * 1000 - 1000 / this.pack.fps);
        if (this.fade >= (this.pack.transitionMs || 180)) this.previous = null;
      }
      if (this.leaveIfReady()) continue;
      if (this.elapsed >= this.duration - .001) {
        if (this.pack.states[this.group].once) {
          this.requested = "idle";
          this.arrive("idle");
        } else if (this.requested !== this.group) {
          this.arrive(this.requested);
        } else if (this.clip.next) {
          this.startClip(this.clip.next);
        } else {
          this.cycle += 1;
          this.arrive(this.group, true);
        }
      }
    }
    return this.sample();
  }
  sample() {
    const seconds = Math.min(this.clip.end - 1 / this.pack.fps,
      (this.clip.start || 0) + this.elapsed / 1000);
    const before = this.previous && this.pack.clips[this.previous.clipId];
    return {
      state: this.group, requested: this.requested,
      phase: this.previous ? "transition" : "loop",
      clip: this.clipId, clipLabel: this.clip.label || this.group,
      serial: this.serial, sourceTime: Math.max(0, seconds),
      blend: this.previous ? clamp(this.fade / (this.pack.transitionMs || 180), 0, 1) : 1,
      previous: this.previous ? { clip: this.previous.clipId, serial: this.previous.serial,
        sourceTime: Math.max(0, Math.min(before.end - 1 / this.pack.fps,
          (before.start || 0) + this.previous.elapsed / 1000)) } : null,
      playbackRate: this.interrupted && this.locked ? 1.5 : 1,
      time: this.time, progress: this.elapsed / this.duration,
      pose: {}, blink: 0, to: [this.group, Math.floor(Math.max(0, seconds) * this.pack.fps)],
    };
  }
}
