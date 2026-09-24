import { createOcDirector, ocMouthFrame } from "./22-oc-director.js";
import { loadOcResources, createOcRenderer } from "./22-oc-renderer.js";
import { OC_REFERENCE_SRC, OC_CHARACTER_PACK_SRC } from "./22-oc-config.js";

export { ocMouthFrame };

export function ocPerformanceMarkup() {
  return `<div class="oc-figure"><div class="oc-stage"><img class="oc-poster" src="${OC_REFERENCE_SRC}" alt="" aria-hidden="true" /></div></div>`;
}

export function initOcPerformance(root) {
  if (!root) return null;
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  let state = "idle"; let speaking = false; let mouthLevel = 0;
  let paused = false; let destroyed = false; let frame = null;
  let director = null; let renderer = null; let loading = null;
  let activeHost = null; let lastTime = null; let lastPaint = 0;
  let smoothMouth = 0; let renderedFrames = 0; let failure = null;

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
    if (!renderer || !director || !activeHost || destroyed) return;
    const sample = director.sample();
    renderer.render(sample, { speaking, level: smoothMouth, reduced: media.matches });
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
    if (destroyed || paused || document.hidden || !activeHost?.isConnected) return;
    if (!activeHost.getClientRects().length) { refresh(); return; }
    const delta = lastTime === null ? 0 : Math.min(100, Math.max(0, time - lastTime));
    lastTime = time;
    director.advance(delta);
    smoothMouth = speaking ? smoothMouth + (mouthLevel - smoothMouth) * (1 - Math.exp(-delta / 28)) : 0;
    if (time - lastPaint >= 1000 / director.pack.fps - 1) {
      lastPaint = time;
      try { paint(); } catch (error) { fail(error); return; }
    }
    frame = requestAnimationFrame(tick);
  }

  function resumeClock() {
    if (!renderer || paused || document.hidden || !activeHost || destroyed) { stopClock(); return; }
    if (media.matches && !speaking) { stopClock(); smoothMouth = 0; paint(); return; }
    renderer?.resume?.();
    if (frame === null) frame = requestAnimationFrame(tick);
  }

  async function prepare() {
    if (!OC_CHARACTER_PACK_SRC) return null;
    if (loading || renderer || destroyed || failure || typeof HTMLCanvasElement === "undefined") return loading;
    if (!visibleHost()) return null;
    loading = loadOcResources(OC_CHARACTER_PACK_SRC).then(resources => {
      if (destroyed) return;
      director = createOcDirector(resources.pack);
      director.setState(state);
      activeHost = visibleHost();
      if (!activeHost) return;
      renderer = createOcRenderer(activeHost, resources);
      root.dataset.ocRenderer = "pixi";
      delete root.dataset.ocError;
      paint(); resumeClock();
    }).catch(fail).finally(() => { loading = null; });
    return loading;
  }

  function refresh() {
    if (destroyed) return;
    const nextHost = paused || document.hidden ? null : visibleHost();
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
    setState(next, hidden = false) {
      if (destroyed) return;
      state = next || "idle";
      paused = hidden;
      root.dataset.ocState = state;
      director?.setState(state);
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
      return { state, speaking, paused, renderedFrames, error: failure, renderer: renderer ? "pixi" : "poster", sample: director?.sample() || null, media: renderer?.snapshot?.() || null };
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
      renderer?.destroy(); renderer = null;
      document.removeEventListener("visibilitychange", refresh);
      media.removeEventListener("change", refresh);
      window.removeEventListener("resize", refresh);
      root.dataset.ocSpeaking = "false";
      root.dataset.ocPaused = "true";
    },
  };
}
