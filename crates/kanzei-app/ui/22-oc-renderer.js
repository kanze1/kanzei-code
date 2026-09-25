import { createOcClipRenderer } from "./22-oc-clip-renderer.js";
const resourceCache = new Map();

export function loadOcResources(manifestSrc) {
  const url = new URL(manifestSrc, import.meta.url).href;
  if (!resourceCache.has(url)) {
    const loading = (async () => {
      const [PIXI, response] = await Promise.all([import("./vendor/pixi/pixi-7.4.3.min.mjs"), fetch(url)]);
      if (!response.ok) throw new Error("OC manifest: " + response.status);
      const pack = await response.json();
      if (pack.format !== "kanzei.character-pack.v3" || pack.fps !== 24 || !pack.states?.idle?.clips?.length) {
        throw new Error("Invalid OC clip pack");
      }
      for (const [name, state] of Object.entries(pack.states)) {
        if (!state.clips?.length || state.clips.some(id => !pack.clips[id])) throw new Error("Invalid OC state: " + name);
      }
      const [poster, mouth, closedMouth, tattoo, entries] = await Promise.all([
        PIXI.Assets.load(new URL(pack.basePoster || pack.poster, url).href),
        PIXI.Assets.load(new URL(pack.mouth.texture, url).href),
        pack.mouth.closedTexture ? PIXI.Assets.load(new URL(pack.mouth.closedTexture, url).href) : null,
        pack.tattoo ? PIXI.Assets.load(new URL(pack.tattoo.texture, url).href) : null,
        Promise.all(Object.entries(pack.clips).map(async ([id, clip]) => {
          if (!Number.isFinite(clip.end) || !Number.isFinite(clip.start || 0) || !(clip.end > (clip.start || 0))) {
            throw new Error("Invalid OC clip duration: " + id);
          }
          for (const edge of [clip.next, clip.exit].filter(Boolean)) if (!pack.clips[edge]) throw new Error("Invalid OC clip edge: " + id);
          const response = await fetch(new URL(clip.tracking, url));
          if (!response.ok) throw new Error("OC tracking: " + id);
          const tracking = await response.json();
          if (!tracking.mouth?.length || tracking.fps !== pack.fps ||
              tracking.mouth.length < Math.ceil(clip.end * pack.fps - .01) ||
              tracking.mouth.some(row => row.length < 4 || !row.slice(0,4).every(Number.isFinite))) {
            throw new Error("Invalid OC mouth tracking: " + id);
          }
          if (pack.tattoo && (!tracking.tattoo || tracking.tattoo.length !== tracking.mouth.length ||
              tracking.tattoo.some(row => row.length < 4 || !row.slice(0,4).every(Number.isFinite)))) {
            throw new Error("Invalid OC tattoo tracking: " + id);
          }
          return [id, { ...clip, url: new URL(clip.file, url).href, tracking }];
        })),
      ]);
      return { PIXI, pack, poster, mouth, closedMouth, tattoo, clips: Object.fromEntries(entries) };
    })().catch(error => { resourceCache.delete(url); throw error; });
    resourceCache.set(url, loading);
  }
  return resourceCache.get(url);
}

export function createOcRenderer(host, resources, options) {
  return createOcClipRenderer(host, resources, options);
}
