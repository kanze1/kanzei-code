import { OcClipDirector } from "./22-oc-clip-director.js";

export function createOcDirector(pack) {
  if (pack.format !== "kanzei.character-pack.v3") throw new Error("Unsupported OC character pack");
  return new OcClipDirector(pack);
}

export function ocMouthFrame(level) {
  const amount = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return amount < .12 ? 0 : amount < .48 ? 2 : 3;
}
