// 身体姿态、眨眼与语音嘴型各自采样；文本流不会自行触发说话嘴型。
const POSES = {
  idle: [[0, 5200], [1, 1200], [0, 6400], [2, 900], [0, 4100]],
  listening: [[0, 900], [1, 650], [0, 6200]],
  thinking: [[1, 380], [3, 2400], [1, 350], [0, 6200]],
  executing: [[1, 520], [3, 1900], [0, 6800]],
  interested: [[0, 320], [4, 1850], [0, 4800]],
  skeptical: [[0, 250], [2, 2300], [0, 5500]],
  replying: [[0, 600], [4, 1400], [0, 5400], [1, 800], [0, 3600]],
  complete: [[0, 220], [5, 1150], [0, 430]],
  interrupted: [[0, 500], [1, 700], [0, 5200]],
  blocked: [[2, 1800], [0, 7400]],
};
const BLINKS = [[0, 3300], [1, 130], [0, 4900], [1, 150], [0, 6400], [1, 110], [0, 160], [1, 100]];

function sampleClip(clip, elapsed, once = false) {
  const time = Number.isFinite(elapsed) ? Math.max(0, elapsed) : 0;
  const duration = clip.reduce((sum, [, ms]) => sum + ms, 0);
  if (once && time >= duration) return { value: 0, wait: Infinity };
  let remaining = time % duration;
  for (const [value, ms] of clip) {
    if (remaining < ms) return { value, wait: ms - remaining };
    remaining -= ms;
  }
  return { value: 0, wait: Infinity };
}

export function ocMouthFrame(level) {
  const value = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return value < .12 ? 0 : value < .48 ? 2 : 3;
}

export function sampleOcPerformance(state, elapsed, blinkElapsed = elapsed, mouthLevel = 0) {
  const clip = Object.hasOwn(POSES, state) ? POSES[state] : POSES.idle;
  const pose = sampleClip(clip, elapsed, state === "complete");
  const eyes = sampleClip(BLINKS, blinkElapsed);
  return { pose: pose.value, blink: eyes.value, mouth: ocMouthFrame(mouthLevel), wait: Math.min(pose.wait, eyes.wait) };
}

// 这里的定位以单格百分比表示；它们只决定现有图层的展示位置。
const FACE = [
  { eyes: [[27, 36, 45, 44], [50, 33, 69, 41]], mouth: [43, 46, 55, 52], cheek: [643, 657] },
  { eyes: [[27, 35, 43, 44], [50, 32, 68, 41]], mouth: [41, 44, 54, 52], cheek: [635, 657] },
  { eyes: [[51, 28, 66, 37], [71, 33, 83, 41]], mouth: [62, 42, 73, 49], cheek: [558, 608] },
  { eyes: [[24, 38, 41, 47], [45, 35, 64, 45]], mouth: [40, 50, 53, 56], cheek: [628, 710] },
  { eyes: [[31, 33, 46, 42], [56, 32, 72, 42]], mouth: [48, 46, 60, 53], cheek: [689, 657] },
  { eyes: [[27, 39, 44, 48], [50, 36, 69, 45]], mouth: [44, 47, 57, 55], cheek: [670, 689] },
];

function inset([left, top, right, bottom]) {
  return `inset(${top}% ${100 - right}% ${100 - bottom}% ${left}%)`;
}

function eyeClip(eyes) {
  const points = eyes.flatMap(([left, top, right, bottom]) => [
    `${left}% ${top}%`, `${right}% ${top}%`, `${right}% ${bottom}%`, `${left}% ${bottom}%`, `${left}% ${top}%`,
  ]);
  points.push(points[0]);
  return `polygon(${points.join(", ")})`;
}

export function ocPerformanceMarkup() {
  const source = "./assets/oc-pixel-cold-v1.png";
  const picture = (extra = "") => `<img class="oc-portrait ${extra}" src="${source}" alt="" decoding="async" draggable="false"/>`;
  return `<div class="oc-figure"><div class="oc-body">`
    + `<svg class="oc-network" viewBox="0 0 1024 1536" fill="none" aria-hidden="true">`
    + ["M80 576H192V416H384V576H584", "M944 432H880V576H584", "M944 1088H848V976H664V872"]
      .map(path => `<path class="oc-route" d="${path}"/><path class="oc-signal" pathLength="100" d="${path}"/>`).join("")
    + `</svg>`
    + `<div class="oc-sprite">${picture()}`
    + `<div class="oc-face-layer oc-eyes">${picture("oc-eye-portrait")}</div>`
    + `<div class="oc-face-layer oc-mouth">${picture("oc-mouth-portrait")}</div></div>`
    + `<svg class="oc-mark" viewBox="0 0 1024 1536" fill="none" aria-hidden="true">`
    + `<g class="oc-cheek"><path d="M0 -10V10M-9 0H9"/></g></svg>`
    + `</div></div>`;
}

export function initOcPerformance(root) {
  if (!root) return null;
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  let state = "idle";
  let paused = false;
  let destroyed = false;
  let poseElapsed = 0;
  let blinkElapsed = 0;
  let startedAt = null;
  let timer = null;
  let painted = "";
  let speaking = false;
  let mouthLevel = 0;

  function stopClock() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
    if (startedAt !== null) {
      const delta = performance.now() - startedAt;
      poseElapsed += delta;
      blinkElapsed += delta;
    }
    startedAt = null;
  }

  function paint(sample) {
    const key = `${sample.pose}/${sample.blink}/${sample.mouth}`;
    if (key === painted) return;
    painted = key;
    const face = FACE[sample.pose];
    root.dataset.ocFrame = String(sample.pose);
    root.dataset.ocBlink = String(sample.blink);
    root.dataset.ocMouth = String(sample.mouth);
    root.style.setProperty("--oc-sprite-col", String(sample.pose));
    root.style.setProperty("--oc-mouth-row", String(sample.mouth));
    root.style.setProperty("--oc-eye-clip", eyeClip(face.eyes));
    root.style.setProperty("--oc-mouth-clip", inset(face.mouth));
    root.style.setProperty("--oc-cheek-x", `${face.cheek[0]}px`);
    root.style.setProperty("--oc-cheek-y", `${face.cheek[1]}px`);
  }

  function sample() {
    if (media.matches) return { pose: 0, blink: 0, mouth: 0, wait: Infinity };
    const delta = startedAt === null ? 0 : performance.now() - startedAt;
    return sampleOcPerformance(state, poseElapsed + delta, blinkElapsed + delta, speaking ? mouthLevel : 0);
  }

  function tick() {
    timer = null;
    if (destroyed || startedAt === null) return;
    const value = sample();
    paint(value);
    if (Number.isFinite(value.wait)) timer = setTimeout(tick, Math.max(16, value.wait));
  }

  function refresh() {
    stopClock();
    if (destroyed) return;
    const hidden = paused || document.hidden;
    root.dataset.ocPaused = String(hidden);
    if (hidden) { mouthLevel = 0; paint(sample()); return; }
    if (media.matches) { paint(sample()); return; }
    startedAt = performance.now();
    tick();
  }

  function setSpeaking(value) {
    if (destroyed) return;
    speaking = Boolean(value);
    if (!speaking) mouthLevel = 0;
    root.dataset.ocSpeaking = String(speaking);
    paint(sample());
  }

  function setState(next, hidden = false) {
    if (destroyed) return;
    const normalized = Object.hasOwn(POSES, next) ? next : "idle";
    if (state === normalized && paused === hidden) return;
    stopClock();
    if (state !== normalized) poseElapsed = 0;
    state = normalized;
    paused = hidden;
    root.dataset.ocState = state;
    if (state === "interrupted") setSpeaking(false);
    refresh();
  }

  document.addEventListener("visibilitychange", refresh);
  media.addEventListener("change", refresh);
  root.dataset.ocState = state;
  root.dataset.ocSpeaking = "false";
  refresh();
  return {
    setState,
    setSpeaking,
    setMouthLevel(value) {
      if (destroyed || paused || document.hidden) return;
      mouthLevel = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
      paint(sample());
    },
    destroy() {
      if (destroyed) return;
      stopClock();
      speaking = false;
      mouthLevel = 0;
      paint({ pose: 0, blink: 0, mouth: 0 });
      root.dataset.ocSpeaking = "false";
      root.dataset.ocPaused = "true";
      destroyed = true;
      document.removeEventListener("visibilitychange", refresh);
      media.removeEventListener("change", refresh);
    },
  };
}
