import { createOcDirector, ocMouthFrame } from "./22-oc-director.js";
import { loadOcResources, createOcRenderer } from "./22-oc-renderer.js";
import { OC_REFERENCE_SRC, OC_CHARACTER_PACK_SRC } from "./22-oc-config.js";
import { readOcSettings } from "./22-oc-preference.js";
import { initOcLayout } from "./22-oc-layout.js";

export { ocMouthFrame };

export function ocPerformanceMarkup() {
  return `<div class="oc-figure"><div class="oc-stage"><img class="oc-poster" data-oc-poster="${OC_REFERENCE_SRC}" alt="" aria-hidden="true" /></div></div>`;
}

export function initOcPerformance(root) {
  if (!root) return null;
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  let state = "idle"; let speaking = false; let mouthLevel = 0;
  let paused = false; let destroyed = false; let frame = null;
  let director = null; let renderer = null; let loading = null;
  let activeHost = null; let lastTime = null; let lastPaint = 0;
  let smoothMouth = 0; let renderedFrames = 0; let failure = null;
  let enabled = document.documentElement.dataset.ocEnabled !== "false";
  let generation = 0;
  let motionMode = readOcSettings().motion;
  const reduced = () => media.matches || motionMode === "still";
  const bodyState = () => motionMode === "idle" ? "idle" : state;
  const layout = initOcLayout(root, () => {
    motionMode = readOcSettings().motion;
    director?.setState(bodyState());
    refresh();
    if (renderer && !paused && !document.hidden) paint();
  });

  function visibleHost() {
    return Array.from(root.querySelectorAll(".oc-stage")).find(host => {
      if (!host.isConnected || !host.getClientRects().length) return false;
      const rect = host.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    }) || null;
  }

  function stopClock() {
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null; lastTime = null;
    renderer?.pause?.();
  }

  function paint() {
    if (!renderer || !director || !activeHost || destroyed || !enabled) return;
    const sample = director.sample();
    const painted = renderer.render(sample, { speaking, level: smoothMouth, reduced: reduced() });
    if (painted === false) return;
    root.dataset.ocPhase = sample.phase;
    root.dataset.ocFrame = sample.to[0] + ":" + sample.to[1];
    root.dataset.ocMouth = String(speaking ? ocMouthFrame(smoothMouth) : 0);
    renderedFrames += 1;
  }

  function fail(error) {
    failure = error instanceof Error ? error.message : String(error);
    stopClock();
    renderer?.destroy(); renderer = null;
    root.dataset.ocRenderer = "poster";
    root.dataset.ocError = failure;
    console.warn("OC renderer unavailable; keeping the character poster:", failure);
  }

  function tick(time) {
    frame = null;
    if (destroyed || !enabled || paused || document.hidden || !activeHost?.isConnected) return;
    if (!activeHost.getClientRects().length) { refresh(); return; }
    const delta = lastTime === null ? 0 : Math.min(100, Math.max(0, time - lastTime));
    lastTime = time;
    if (!renderer.isBuffering?.()) director.advance(delta);
    smoothMouth = speaking ? smoothMouth + (mouthLevel - smoothMouth) * (1 - Math.exp(-delta / 28)) : 0;
    if (time - lastPaint >= 1000 / director.pack.fps - 1) {
      const interval = 1000 / director.pack.fps;
      lastPaint = time - ((time - lastPaint) % interval);
      try { paint(); } catch (error) { fail(error); return; }
    }
    frame = requestAnimationFrame(tick);
  }

  function resumeClock() {
    if (!renderer || !enabled || paused || document.hidden || !activeHost || destroyed) { stopClock(); return; }
    if (reduced() && !speaking) { stopClock(); smoothMouth = 0; paint(); return; }
    renderer?.resume?.();
    if (frame === null) frame = requestAnimationFrame(tick);
  }

  async function prepare() {
    if (!OC_CHARACTER_PACK_SRC || !enabled) return null;
    if (loading || renderer || destroyed || failure || typeof HTMLCanvasElement === "undefined") return loading;
    if (!visibleHost()) return null;
    const requestGeneration = generation;
    loading = loadOcResources(OC_CHARACTER_PACK_SRC).then(resources => {
      if (destroyed || !enabled || requestGeneration !== generation) return;
      director = createOcDirector(resources.pack);
      director.setState(bodyState());
      activeHost = visibleHost();
      if (!activeHost) return;
      layout.refresh(activeHost);
      const poster = activeHost.querySelector(".oc-poster");
      if (poster && !poster.src) poster.src = OC_REFERENCE_SRC;
      renderer = createOcRenderer(activeHost, resources);
      renderer.onFrame?.(() => {
        if (enabled && !paused && !document.hidden) {
          try { paint(); } catch (error) { fail(error); }
        }
      });
      root.dataset.ocRenderer = "pixi";
      delete root.dataset.ocError;
      paint(); resumeClock();
    }).catch(fail).finally(() => {
      loading = null;
      if (enabled && !destroyed && !renderer && !failure && visibleHost()) refresh();
    });
    return loading;
  }

  function refresh() {
    if (destroyed) return;
    const nextHost = !enabled || paused || document.hidden ? null : visibleHost();
    layout.refresh(nextHost);
    root.dataset.ocPaused = String(!nextHost);
    if (!nextHost) { activeHost = null; stopClock(); return; }
    if (nextHost !== activeHost) {
      activeHost = nextHost;
      renderer?.moveTo(nextHost);
      if (renderer) paint();
    }
    if (!renderer) void prepare();
    else resumeClock();
  }

  document.addEventListener("visibilitychange", refresh);
  media.addEventListener("change", refresh);
  window.addEventListener("resize", refresh);
  root.dataset.ocState = state;
  root.dataset.ocSpeaking = "false";
  root.dataset.ocMouth = "0";
  root.dataset.ocRenderer = "poster";
  refresh();
  return {
    ready: () => prepare(),
    setEnabled(value) {
      const next = Boolean(value);
      if (enabled === next) return;
      enabled = next; generation += 1;
      if (!enabled) {
        stopClock(); renderer?.destroy(); renderer = null; activeHost = null;
        root.dataset.ocRenderer = "off";
      } else { failure = null; refresh(); }
    },
    setState(next, hidden = false) {
      if (destroyed) return;
      state = next || "idle";
      paused = hidden;
      root.dataset.ocState = state;
      director?.setState(bodyState());
      if (state === "interrupted") {
        speaking = false; mouthLevel = 0; smoothMouth = 0;
        root.dataset.ocSpeaking = "false";
        root.dataset.ocMouth = "0";
        if (renderer && !paused && !document.hidden) paint();
      }
      refresh();
    },
    setSpeaking(value) {
      if (destroyed) return;
      const wasSpeaking = speaking;
      speaking = Boolean(value);
      root.dataset.ocSpeaking = String(speaking);
      if (!speaking) {
        mouthLevel = 0; smoothMouth = 0;
        // Physical playback completion/interrupt closes the mouth immediately.
        if (wasSpeaking && renderer && !paused && !document.hidden) paint();
      }
      resumeClock();
    },
    setMouthLevel(value) {
      if (destroyed || paused || document.hidden) return;
      mouthLevel = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
    },
    snapshot() {
      return { state, speaking, paused: paused || !enabled, enabled, motionMode, renderedFrames, error: failure, renderer: !enabled ? "off" : renderer ? "pixi" : "poster", sample: director?.sample() || null, media: renderer?.snapshot?.() || null };
    },
    redraw() { paint(); },
    reset() {
      state = "idle"; speaking = false; mouthLevel = 0; smoothMouth = 0; paused = false;
      if (director) director = createOcDirector(director.pack);
      renderer?.reset?.();
      lastTime = null;
      root.dataset.ocState = state;
      root.dataset.ocSpeaking = "false";
      refresh();
      if (renderer) paint();
    },
    destroy() {
      if (destroyed) return;
      destroyed = true; stopClock();
      layout.destroy();
      renderer?.destroy(); renderer = null;
      document.removeEventListener("visibilitychange", refresh);
      media.removeEventListener("change", refresh);
      window.removeEventListener("resize", refresh);
      root.dataset.ocSpeaking = "false";
      root.dataset.ocPaused = "true";
    },
  };
}
