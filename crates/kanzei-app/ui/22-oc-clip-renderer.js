// Whole-character clips contain the body motion. Composite the measured mouth
// anchor and remove the neutral backdrop without applying another idle rig.
import { OC_DETAIL_FRAGMENT } from "./22-oc-detail-shader.js";
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

// Packed RGB + alpha is authored offline; only mouth pixels need dynamic work.
const PACKED_FRAGMENT = `
precision mediump float;
varying vec2 vUv;
uniform sampler2D uVideoA;
uniform sampler2D uVideoB;
uniform sampler2D uMouthArt;
uniform vec4 uMouthA;
uniform vec4 uMouthB;
uniform vec2 uMouthReference;
uniform vec2 uSize;
uniform vec2 uPacking;
uniform float uBlend;
uniform float uMouth;
vec4 character(sampler2D art,vec4 track,float isPacked){
  vec2 uv=vec2(vUv.x,vUv.y*mix(1.0,.5,isPacked));
  vec4 color=texture2D(art,uv);
  float alpha=color.a;
  if(isPacked>.5)alpha=texture2D(art,vec2(vUv.x,.5+vUv.y*.5)).r;
  if(alpha<.004)return vec4(0.0);
  if(uMouth>.001){
    vec2 pixels=(vUv-track.xy)*uSize;
    float c=cos(track.w),s=sin(track.w);
    vec2 local=vec2(c*pixels.x+s*pixels.y,-s*pixels.x+c*pixels.y)/max(track.z,.8)/uSize;
    float region=1.0-smoothstep(.72,1.0,length(local/vec2(.042,.018)));
    if(region>0.0){
      vec2 openLocal=vec2(local.x,local.y/mix(.25,1.0,uMouth));
      vec3 mouth=texture2D(uMouthArt,uMouthReference+openLocal).rgb;
      vec2 skinUv=vec2(track.x,(track.y-.019)*mix(1.0,.5,isPacked));
      vec3 skin=texture2D(art,skinUv).rgb;
      vec3 source=texture2D(uMouthArt,uMouthReference+vec2(0.0,-.019)).rgb;
      mouth+=clamp(skin-source,vec3(-.12),vec3(.12));
      float opening=1.0-smoothstep(.82,1.0,length(openLocal/vec2(.024,.007)));
      color.rgb=mix(color.rgb,mix(skin,clamp(mouth,0.0,1.0),opening),region);
    }
  }
  return vec4(color.rgb*alpha,alpha);
}
void main(){
  if(uBlend>=.999)gl_FragColor=character(uVideoB,uMouthB,uPacking.y);
  else gl_FragColor=mix(character(uVideoA,uMouthA,uPacking.x),character(uVideoB,uMouthB,uPacking.y),uBlend);
}`;

export function createOcClipRenderer(host, resources, options = {}) {
  const { PIXI, pack, poster, mouth, closedMouth, tattoo, clips } = resources;
  const [artWidth, artHeight] = pack.size;
  const app = new PIXI.Application({
    width: 1, height: 1, autoStart: false, sharedTicker: false, backgroundAlpha: 0,
    antialias: false, autoDensity: true, resolution: Math.min(1.5, window.devicePixelRatio || 1),
    powerPreference: "default", preserveDrawingBuffer: Boolean(options.export),
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
    uVideoA: poster, uVideoB: poster, uMouthArt: mouth, uClosedMouthArt: closedMouth || poster,
    uTattooArt: tattoo || poster,
    uTattooA: [...(pack.tattoo?.anchor || [.588,.340]),1,0],
    uTattooB: [...(pack.tattoo?.anchor || [.588,.340]),1,0],
    uTattooReference: pack.tattoo?.reference || [811,1017],
    uTattooTextureSize: pack.tattoo?.textureSize || [1254,1254],
    uTattooScale: pack.tattoo?.scale || .3,
    uMouthA: [...pack.mouth.reference,1,0], uMouthB: [...pack.mouth.reference,1,0],
    uMouthReference: pack.mouth.reference.slice(), uBackground: pack.background.map(n => n / 255),
    uTexel: [3 / artWidth, 3 / artHeight], uSize: pack.size.slice(),
    uBlend: 1, uMouth: 0, uKey: 1, uPacking: [0, 0],
  };
  const packed = pack.videoLayout === "rgb-alpha-vertical";
  const shader = pack.mouth.closed === "clean-plate" ? OC_DETAIL_FRAGMENT : packed ? PACKED_FRAGMENT : FRAGMENT;
  const mesh = new PIXI.Mesh(geometry, PIXI.Shader.from(VERTEX, shader, uniforms));
  stage.addChild(mesh);
  const entries = new Map();
  let currentHost = null, width = 0, height = 0, destroyed = false, paused = false;
  let lastSample = null, lastOptions = null, displayed = null, usage = 0, seekVersion = 0;
  let seekQueue = Promise.resolve();
  let frameCallback = null, paintSignature = "", buffering = false, textureUploads = 0, paints = 0;

  function dispose(entry) {
    entry.cancelled = true;
    entry.finish?.(new Error("OC video disposed"));
    entry.video.pause();
    if (entry.frameRequest != null) entry.video.cancelVideoFrameCallback?.(entry.frameRequest);
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
    const entry = { key, id, video, texture: null, error: null, cancelled: false, used: ++usage,
      frameVersion: 0, uploadedVersion: -1, mediaTime: null, startedSerial: null };
    function decoded(_now, metadata) {
      if (entry.cancelled || destroyed) return;
      entry.mediaTime = metadata.mediaTime; entry.frameVersion++;
      entry.frameRequest = video.requestVideoFrameCallback(decoded);
      if (!paused && entry === displayed) frameCallback?.();
    }
    if (video.requestVideoFrameCallback) entry.frameRequest = video.requestVideoFrameCallback(decoded);
    video.addEventListener("seeked", () => {
      if (!video.requestVideoFrameCallback) { entry.mediaTime = video.currentTime; entry.frameVersion++; }
    });
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
        if (video.videoWidth !== artWidth || video.videoHeight !== artHeight * (packed ? 2 : 1)) {
          return finish(new Error("OC video dimensions: " + id));
        }
        // Decoded-frame versions below already gate uploads. A second Pixi FPS
        // throttle can skip an explicit seek while the detail anchors advance.
        entry.texture = PIXI.Texture.from(video, { resourceOptions: { autoPlay: false, updateFPS: 0 } });
        entry.texture.baseTexture.resource.autoUpdate = false;
        if (clip.start) video.currentTime = clip.start;
        entry.frameVersion++;
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
    width = w; height = h; paintSignature = "";
    const detailWidth = closedMouth ? Math.max(artWidth, closedMouth.width) : artWidth;
    const resolution = Math.min(1.5, window.devicePixelRatio || 1, detailWidth / Math.max(1, Math.min(w, h * artWidth / artHeight)));
    app.renderer.resolution = resolution;
    app.renderer.resize(w, h);
    const scale = Math.min(w / artWidth, h / artHeight);
    stage.scale.set(scale); stage.position.set((w - artWidth * scale) / 2, h - artHeight * scale);
  }
  function moveTo(next) {
    if (destroyed || next === currentHost) return;
    currentHost?.removeAttribute("data-oc-ready");
    currentHost = next; next.appendChild(app.view); width = height = 0; resize();
  }
  function position(entry, seconds, rate, playing, serial) {
    if (!entry.texture) return false;
    if (entry.error) throw entry.error;
    const target = Math.max(0, Math.min(entry.video.duration - 1 / pack.fps, seconds));
    // Native video playback owns time inside a clip. Repeated wall-clock seeks
    // forced the decoder back to a GOP boundary during stalls and transitions.
    if (entry.startedSerial !== serial && !paused) {
      entry.startedSerial = serial;
      if (Math.abs(entry.video.currentTime - target) > 1 / pack.fps && !entry.video.seeking) entry.video.currentTime = target;
    }
    if (entry.video.playbackRate !== rate) entry.video.playbackRate = rate;
    if (playing && !paused && entry.video.paused && !entry.video.ended) {
      void entry.video.play().catch(error => {
        if (error.name !== "AbortError" && !entry.cancelled && !destroyed) entry.error = error;
      });
    } else if (!playing || paused) entry.video.pause();
    if (entry.video.seeking || entry.video.readyState < 2) return false;
    entry.frameTime = entry.mediaTime ?? entry.video.currentTime;
    const version = entry.video.requestVideoFrameCallback ? entry.frameVersion : Math.floor(entry.video.currentTime * pack.fps);
    if (entry.uploadedVersion !== version) {
      entry.texture.baseTexture.resource.update(); textureUploads++;
      entry.uploadedVersion = version;
    }
    return true;
  }
  function anchor(id, seconds, kind = "mouth") {
    const tracking = clips[id].tracking;
    const points = tracking[kind];
    if (!points) return [...(pack.tattoo?.anchor || [.588,.340]),1,0];
    const index = Math.max(0, Math.min(points.length - 1, Math.floor(seconds * tracking.fps + .001)));
    return points[index].slice(0, 4);
  }
  function render(sample, options = {}) {
    if (destroyed) return false;
    lastSample = sample; lastOptions = options; resize();
    const { speaking = false, level = 0, reduced = false } = options;
    const keep = new Set();
    const playing = new Set();
    let signature = "poster";
    if (reduced) {
      uniforms.uVideoA = uniforms.uVideoB = poster;
      uniforms.uMouthA = uniforms.uMouthB = [...pack.mouth.reference,1,0];
      uniforms.uTattooA = uniforms.uTattooB = [...(pack.tattoo?.anchor || [.588,.340]),1,0];
      uniforms.uBlend = 1;
      uniforms.uPacking = [0, 0]; buffering = false;
    } else {
      const current = entryFor(sample.clip, sample.serial);
      keep.add(current.key);
      playing.add(current.key);
      if (current.error) throw current.error;
      const before = sample.previous && entryFor(sample.previous.clip, sample.previous.serial);
      if (before) keep.add(before.key);
      if (before) playing.add(before.key);
      const ready = position(current, sample.sourceTime, sample.playbackRate, true, sample.serial);
      buffering = !ready;
      if (ready) {
        let previousReady = false;
        if (before) previousReady = position(before, sample.previous.sourceTime, 1, true, sample.previous.serial);
        uniforms.uVideoB = current.texture;
        uniforms.uMouthB = anchor(sample.clip, current.frameTime);
        uniforms.uTattooB = anchor(sample.clip, current.frameTime, "tattoo");
        uniforms.uVideoA = previousReady ? before.texture : current.texture;
        uniforms.uMouthA = previousReady ? anchor(sample.previous.clip, before.frameTime) : uniforms.uMouthB;
        uniforms.uTattooA = previousReady ? anchor(sample.previous.clip, before.frameTime, "tattoo") : uniforms.uTattooB;
        uniforms.uBlend = previousReady ? sample.blend : 1;
        uniforms.uPacking = [packed ? 1 : 0, packed ? 1 : 0];
        signature = current.key + ":" + current.uploadedVersion + (previousReady ? ":" + before.key + ":" + before.uploadedVersion + ":" + Math.round(sample.blend * 100) : "");
        displayed = current;
      } else if (displayed?.texture) {
        keep.add(displayed.key);
        uniforms.uVideoA = uniforms.uVideoB = displayed.texture;
        uniforms.uMouthA = uniforms.uMouthB = anchor(displayed.id, displayed.frameTime || 0);
        uniforms.uTattooA = uniforms.uTattooB = anchor(displayed.id, displayed.frameTime || 0, "tattoo");
        uniforms.uBlend = 1;
        uniforms.uPacking = [packed ? 1 : 0, packed ? 1 : 0];
        signature = displayed.key + ":" + displayed.uploadedVersion;
      }
      const sequence = pack.states[sample.requested]?.clips || pack.states.idle.clips;
      const nextId = sample.state !== sample.requested ? clips[sample.clip].exit || sequence[0] :
        clips[sample.clip].next || sequence[(sequence.indexOf(sample.clip) + 1) % sequence.length];
      if (nextId) keep.add(entryFor(nextId, sample.serial + 1).key);
    }
    for (const entry of entries.values()) if (!playing.has(entry.key) || reduced) entry.video.pause();
    while (entries.size > 4) {
      const disposable = [...entries.values()].filter(entry => !keep.has(entry.key)).sort((a,b) => a.used - b.used)[0];
      if (!disposable) break;
      entries.delete(disposable.key); dispose(disposable);
    }
    uniforms.uMouth = speaking && level >= .12 ? Math.round(Math.min(1, Math.max(0, level)) * 30) / 30 : 0;
    signature += ":" + uniforms.uMouth;
    if (signature === paintSignature) return false;
    paintSignature = signature;
    app.renderer.render(app.stage);
    if (!paints && app.renderer.gl.getError() !== app.renderer.gl.NO_ERROR) throw new Error("OC shader initialization failed");
    paints++;
    currentHost?.setAttribute("data-oc-ready", "true");
    app.view.dataset.ocClip = sample.state + "/" + sample.phase;
    app.view.dataset.ocAction = sample.clip;
    app.view.dataset.ocFrame = String(sample.to[1]);
    app.view.dataset.ocMouth = String(uniforms.uMouth);
    return true;
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
        const frameTime = Math.floor(Math.max(0, pose.sourceTime) * pack.fps + .001) / pack.fps;
        // Seek inside the frame interval: a rounded 24 fps boundary can still
        // fall in the preceding frame after the decoder converts timestamps.
        const target = Math.min(active.video.duration - .0001, frameTime + .5 / pack.fps);
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
        // seeked makes the selected frame available for a texture upload.
        // Paused WebView2 videos need not submit it to the compositor, so a
        // requestVideoFrameCallback may never follow this explicit seek.
        active.mediaTime = Math.floor(target * pack.fps + .001) / pack.fps;
        active.frameVersion++;
      }
      if (version !== seekVersion || destroyed) return;
      const wasPaused = paused; paused = true;
      paintSignature = "";
      try { render(sample, options); } finally { paused = wasPaused; }
    };
    seekQueue = seekQueue.catch(() => {}).then(operation);
    return seekQueue;
  }
  moveTo(host);
  return {
    canvas: app.view, moveTo, render, seek,
    onFrame(callback) { frameCallback = callback; },
    isBuffering: () => buffering,
    pause() { paused = true; for (const entry of entries.values()) entry.video.pause(); },
    resume() { seekVersion += 1; paused = false; },
    reset() {
      seekVersion += 1; displayed = null; paintSignature = ""; buffering = false;
      for (const entry of entries.values()) entry.video.pause();
      uniforms.uVideoA = uniforms.uVideoB = poster;
      uniforms.uMouthA = uniforms.uMouthB = [...pack.mouth.reference,1,0];
      uniforms.uTattooA = uniforms.uTattooB = [...(pack.tattoo?.anchor || [.588,.340]),1,0];
      uniforms.uBlend = 1; uniforms.uMouth = 0;
      uniforms.uPacking = [0, 0];
    },
    redraw() { paintSignature = ""; if (lastSample) render(lastSample, lastOptions); },
    snapshot() { return { decodedSources: entries.size, playingSources: [...entries.values()].filter(e => !e.video.paused).length, textureUploads, paints, buffering, packedAlpha: packed, drawingBuffer: [app.view.width, app.view.height] }; },
    destroy() {
      if (destroyed) return;
      destroyed = true; currentHost?.removeAttribute("data-oc-ready");
      for (const entry of entries.values()) dispose(entry);
      entries.clear(); app.destroy(true, { children: true, texture: false, baseTexture: false });
    },
  };
}
