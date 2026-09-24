// Whole-character clips contain the body motion. Composite the measured mouth
// anchor and remove the neutral backdrop without applying another idle rig.
const VERTEX = `
precision mediump float;
attribute vec2 aVertexPosition;
attribute vec2 aTextureCoord;
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
varying vec2 vUv;
void main(){
  vUv=aTextureCoord;
  gl_Position=vec4((projectionMatrix*translationMatrix*vec3(aVertexPosition,1.0)).xy,0.0,1.0);
}`;
const FRAGMENT = `
precision mediump float;
varying vec2 vUv;
uniform sampler2D uVideoA;
uniform sampler2D uVideoB;
uniform sampler2D uMouthArt;
uniform sampler2D uClosedMouthArt;
uniform vec4 uMouthA;
uniform vec4 uMouthB;
uniform vec2 uMouthReference;
uniform vec3 uBackground;
uniform float uBlend;
uniform float uMouth;
uniform float uKey;
uniform vec2 uTexel;
uniform vec2 uSize;
void edgeProbe(vec3 difference,vec3 neighbor,inout float nearest,inout float extent){
  vec3 other=neighbor-uBackground;
  float size=length(other);
  nearest=min(nearest,size);
  float direction=dot(difference,other)/max(length(difference)*size,.0001);
  if(direction>.97)extent=max(extent,size);
}
vec4 character(sampler2D art,vec4 track){
  vec4 color=texture2D(art,vUv);
  {
    vec2 anchor=track.xy;
    vec2 pixels=(vUv-anchor)*uSize;
    float cosine=cos(track.w),sine=sin(track.w);
    vec2 local=vec2(cosine*pixels.x+sine*pixels.y,-sine*pixels.x+cosine*pixels.y)/max(track.z,.8)/uSize;
    float region=1.0-smoothstep(.72,1.0,length(local/vec2(.042,.018)));
    vec2 openLocal=vec2(local.x,local.y/mix(.25,1.0,uMouth));
    vec2 mouthUv=uMouthReference+openLocal;
    float talking=step(.001,uMouth);
    vec3 mouth=mix(texture2D(uClosedMouthArt,uMouthReference+local).rgb,
                   texture2D(uMouthArt,mouthUv).rgb,talking);
    vec3 currentSkin=texture2D(art,anchor+vec2(0.0,-.019)).rgb;
    vec3 sourceSkin=mix(texture2D(uClosedMouthArt,uMouthReference+vec2(0.0,-.019)).rgb,
                        texture2D(uMouthArt,uMouthReference+vec2(0.0,-.019)).rgb,talking);
    mouth+=clamp(currentSkin-sourceSkin,vec3(-.12),vec3(.12));
    vec3 skin=mix(texture2D(art,anchor+vec2(-.040,0.0)).rgb,
                  texture2D(art,anchor+vec2(.040,0.0)).rgb,clamp(.5+local.x/.080,0.0,1.0));
    float opening=1.0-smoothstep(.82,1.0,length(mix(local,openLocal,talking)/vec2(.024,.007)));
    color.rgb=mix(color.rgb,mix(skin,clamp(mouth,0.0,1.0),opening),region);
  }
  float distanceToBackground=distance(color.rgb,uBackground);
  float alpha=smoothstep(.042,.125,distanceToBackground);
  vec3 difference=color.rgb-uBackground;
  float nearest=distanceToBackground,extent=distanceToBackground;
  edgeProbe(difference,texture2D(art,vUv+vec2(uTexel.x,0.0)).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv-vec2(uTexel.x,0.0)).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv+vec2(0.0,uTexel.y)).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv-vec2(0.0,uTexel.y)).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv+uTexel*.7).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv-uTexel*.7).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv+vec2(uTexel.x,-uTexel.y)*.7).rgb,nearest,extent);
  edgeProbe(difference,texture2D(art,vUv+vec2(-uTexel.x,uTexel.y)*.7).rgb,nearest,extent);
  float edge=1.0-smoothstep(.025,.05,nearest);
  alpha*=mix(1.0,clamp(distanceToBackground/max(extent,.001),0.0,1.0),edge);
  float skin=smoothstep(.04,.085,color.r-color.g)*smoothstep(.025,.065,color.g-color.b);
  float light=dot(color.rgb,vec3(.2126,.7152,.0722));
  float backdropLight=dot(uBackground,vec3(.2126,.7152,.0722));
  float brightFringe=smoothstep(backdropLight-.18,backdropLight-.08,light);
  alpha*=1.0-edge*brightFringe*(1.0-skin);
  alpha*=mix(1.0,smoothstep(.18,.62,distanceToBackground),edge*(1.0-skin));
  float insideHead=1.0-smoothstep(.96,1.0,length((vUv-vec2(.50,.18))/vec2(.16,.13)));
  alpha=max(alpha,insideHead);
  alpha=mix(1.0,alpha,uKey)*color.a;
  vec3 foreground=clamp((color.rgb-(1.0-alpha)*uBackground)/max(alpha,.001),0.0,1.0);
  return vec4(foreground*alpha,alpha);
}
void main(){
  gl_FragColor=mix(character(uVideoA,uMouthA),character(uVideoB,uMouthB),uBlend);
}`;

export function createOcClipRenderer(host, resources) {
  const { PIXI, pack, poster, mouth, clips } = resources;
  const [artWidth, artHeight] = pack.size;
  const app = new PIXI.Application({
    width: 1, height: 1, autoStart: false, sharedTicker: false, backgroundAlpha: 0,
    antialias: true, autoDensity: true, resolution: Math.min(2, window.devicePixelRatio || 1),
    powerPreference: "low-power", preserveDrawingBuffer: true,
  });
  app.stop();
  app.view.className = "oc-canvas";
  const stage = new PIXI.Container();
  app.stage.addChild(stage);
  const geometry = new PIXI.Geometry()
    .addAttribute("aVertexPosition", new Float32Array([0,0,artWidth,0,artWidth,artHeight,0,artHeight]), 2)
    .addAttribute("aTextureCoord", new Float32Array([0,0,1,0,1,1,0,1]), 2)
    .addIndex([0,1,2,0,2,3]);
  const uniforms = {
    uVideoA: poster, uVideoB: poster, uMouthArt: mouth, uClosedMouthArt: poster,
    uMouthA: [...pack.mouth.reference,1,0], uMouthB: [...pack.mouth.reference,1,0],
    uMouthReference: pack.mouth.reference.slice(), uBackground: pack.background.map(n => n / 255),
    uTexel: [3 / artWidth, 3 / artHeight], uSize: pack.size.slice(),
    uBlend: 1, uMouth: 0, uKey: 1,
  };
  const mesh = new PIXI.Mesh(geometry, PIXI.Shader.from(VERTEX, FRAGMENT, uniforms));
  stage.addChild(mesh);
  const entries = new Map();
  let currentHost = null, width = 0, height = 0, destroyed = false, paused = false;
  let lastSample = null, lastOptions = null, displayed = null, usage = 0, seekVersion = 0;
  let seekQueue = Promise.resolve();

  function dispose(entry) {
    entry.cancelled = true;
    entry.finish?.(new Error("OC video disposed"));
    entry.video.pause();
    entry.texture?.destroy(true);
    entry.video.removeAttribute("src");
    entry.video.load();
  }
  function entryFor(id, serial) {
    const key = id + ":" + (serial % 2);
    if (entries.has(key)) {
      const entry = entries.get(key);
      entry.used = ++usage;
      return entry;
    }
    const clip = clips[id];
    const video = document.createElement("video");
    video.muted = true; video.defaultMuted = true; video.playsInline = true;
    video.preload = "auto"; video.loop = false;
    const entry = { key, id, video, texture: null, error: null, cancelled: false, used: ++usage };
    entries.set(key, entry);
    entry.ready = new Promise(resolve => {
      let settled = false;
      const finish = error => {
        if (settled) return;
        settled = true; clearTimeout(timer);
        video.removeEventListener("canplay", loaded);
        video.removeEventListener("error", failed);
        if (error) entry.error = error;
        resolve(!error);
      };
      const loaded = () => {
        if (entry.cancelled || destroyed) return finish(new Error("OC video disposed"));
        if (video.videoWidth !== artWidth || video.videoHeight !== artHeight) {
          return finish(new Error("OC video dimensions: " + id));
        }
        entry.texture = PIXI.Texture.from(video, { resourceOptions: { autoPlay: false, updateFPS: pack.fps } });
        entry.texture.baseTexture.resource.autoUpdate = false;
        finish(null);
      };
      const failed = () => finish(new Error("OC video unavailable: " + id));
      const timer = setTimeout(() => finish(new Error("OC video timeout: " + id)), 20000);
      entry.finish = finish;
      video.addEventListener("canplay", loaded);
      video.addEventListener("error", failed);
      video.src = clip.url; video.load();
    });
    return entry;
  }
  function resize() {
    if (!currentHost || destroyed) return;
    const bounds = currentHost.getBoundingClientRect();
    const w = Math.max(1, Math.round(bounds.width)), h = Math.max(1, Math.round(bounds.height));
    if (w === width && h === height) return;
    width = w; height = h; app.renderer.resize(w, h);
    const scale = Math.min(w / artWidth, h / artHeight);
    stage.scale.set(scale); stage.position.set((w - artWidth * scale) / 2, h - artHeight * scale);
  }
  function moveTo(next) {
    if (destroyed || next === currentHost) return;
    currentHost?.removeAttribute("data-oc-ready");
    currentHost = next; next.appendChild(app.view); width = height = 0; resize();
  }
  function position(entry, seconds, rate, playing) {
    if (!entry.texture) return false;
    if (entry.error) throw entry.error;
    const target = Math.max(0, Math.min(entry.video.duration - 1 / pack.fps, seconds));
    if (Math.abs(entry.video.currentTime - target) > .13 && !entry.video.seeking) entry.video.currentTime = target;
    entry.video.playbackRate = rate;
    if (playing && !paused && entry.video.paused) {
      void entry.video.play().catch(error => {
        if (error.name !== "AbortError" && !entry.cancelled && !destroyed) entry.error = error;
      });
    } else if (!playing || paused) entry.video.pause();
    if (entry.video.seeking || entry.video.readyState < 2) return false;
    entry.frameTime = entry.video.currentTime;
    entry.texture.baseTexture.resource.update();
    return true;
  }
  function anchor(id, seconds) {
    const tracking = clips[id].tracking;
    const index = Math.max(0, Math.min(tracking.mouth.length - 1, Math.floor(seconds * tracking.fps + .001)));
    return tracking.mouth[index].slice(0, 4);
  }
  function render(sample, options = {}) {
    if (destroyed) return;
    lastSample = sample; lastOptions = options; resize();
    const { speaking = false, level = 0, reduced = false } = options;
    const keep = new Set();
    if (reduced) {
      uniforms.uVideoA = uniforms.uVideoB = poster;
      uniforms.uMouthA = uniforms.uMouthB = [...pack.mouth.reference,1,0];
      uniforms.uBlend = 1;
    } else {
      const current = entryFor(sample.clip, sample.serial);
      keep.add(current.key);
      if (current.error) throw current.error;
      const before = sample.previous && entryFor(sample.previous.clip, sample.previous.serial);
      if (before) keep.add(before.key);
      const ready = position(current, sample.sourceTime, sample.playbackRate, true);
      if (ready) {
        let previousReady = false;
        if (before) previousReady = position(before, sample.previous.sourceTime, 1, true);
        uniforms.uVideoB = current.texture;
        uniforms.uMouthB = anchor(sample.clip, current.frameTime);
        uniforms.uVideoA = previousReady ? before.texture : current.texture;
        uniforms.uMouthA = previousReady ? anchor(sample.previous.clip, before.frameTime) : uniforms.uMouthB;
        uniforms.uBlend = previousReady ? sample.blend : 1;
        displayed = current;
      } else if (displayed?.texture) {
        keep.add(displayed.key);
        uniforms.uVideoA = uniforms.uVideoB = displayed.texture;
        uniforms.uMouthA = uniforms.uMouthB = anchor(displayed.id, displayed.frameTime || 0);
        uniforms.uBlend = 1;
      }
      const sequence = pack.states[sample.requested]?.clips || pack.states.idle.clips;
      const nextId = sample.state !== sample.requested ? clips[sample.clip].exit || sequence[0] :
        clips[sample.clip].next || sequence[(sequence.indexOf(sample.clip) + 1) % sequence.length];
      if (nextId) keep.add(entryFor(nextId, sample.serial + 1).key);
    }
    for (const entry of entries.values()) if (!keep.has(entry.key) || reduced) entry.video.pause();
    while (entries.size > 4) {
      const disposable = [...entries.values()].filter(entry => !keep.has(entry.key)).sort((a,b) => a.used - b.used)[0];
      if (!disposable) break;
      entries.delete(disposable.key); dispose(disposable);
    }
    uniforms.uMouth = speaking && level >= .12 ? Math.min(1, Math.max(0, level)) : 0;
    app.renderer.render(app.stage);
    currentHost?.setAttribute("data-oc-ready", "true");
    app.view.dataset.ocClip = sample.state + "/" + sample.phase;
    app.view.dataset.ocAction = sample.clip;
    app.view.dataset.ocFrame = String(sample.to[1]);
    app.view.dataset.ocMouth = String(uniforms.uMouth);
  }
  function seek(sample, options = {}) {
    const version = ++seekVersion;
    const operation = async () => {
      if (version !== seekVersion || destroyed) return;
      const poses = options.reduced ? [] : [sample, sample.previous].filter(Boolean);
      for (const pose of poses) {
        const active = entryFor(pose.clip, pose.serial);
        await active.ready;
        if (version !== seekVersion || destroyed) return;
        if (active.error) throw active.error;
        const target = Math.max(0, Math.min(active.video.duration - 1 / pack.fps, pose.sourceTime));
        active.video.pause();
        if (Math.abs(active.video.currentTime - target) > .0005) {
          await new Promise((resolve, reject) => {
            const finish = error => {
              clearTimeout(timer); active.video.removeEventListener("seeked", loaded);
              if (error) reject(error); else resolve();
            };
            const loaded = () => finish();
            const timer = setTimeout(() => finish(new Error("OC seek timeout")), 8000);
            active.video.addEventListener("seeked", loaded, { once: true });
            active.video.currentTime = target;
          });
        }
      }
      if (version !== seekVersion || destroyed) return;
      const wasPaused = paused; paused = true;
      try { render(sample, options); } finally { paused = wasPaused; }
    };
    seekQueue = seekQueue.catch(() => {}).then(operation);
    return seekQueue;
  }
  moveTo(host);
  return {
    canvas: app.view, moveTo, render, seek,
    pause() { paused = true; for (const entry of entries.values()) entry.video.pause(); },
    resume() { seekVersion += 1; paused = false; },
    reset() {
      seekVersion += 1; displayed = null;
      for (const entry of entries.values()) entry.video.pause();
      uniforms.uVideoA = uniforms.uVideoB = poster;
      uniforms.uMouthA = uniforms.uMouthB = [...pack.mouth.reference,1,0];
      uniforms.uBlend = 1; uniforms.uMouth = 0;
    },
    redraw() { if (lastSample) render(lastSample, lastOptions); },
    snapshot() { return { decodedSources: entries.size, playingSources: [...entries.values()].filter(e => !e.video.paused).length }; },
    destroy() {
      if (destroyed) return;
      destroyed = true; currentHost?.removeAttribute("data-oc-ready");
      for (const entry of entries.values()) dispose(entry);
      entries.clear(); app.destroy(true, { children: true, texture: false, baseTexture: false });
    },
  };
}
