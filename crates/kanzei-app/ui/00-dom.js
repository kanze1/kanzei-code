// DOM lookup and the explicit bootstrap installation boundary.
export const $ = id => document.getElementById(id);
let phase = "collecting";
export function defer(install) {
  if (phase === "ready") Promise.resolve().then(install);
  else document.addEventListener("kz:ui-ready", install, { once: true });
}
export function initializeUi() {
  if (phase !== "collecting") throw new Error("UI already initialized");
  phase = "ready";
  document.dispatchEvent(new Event("kz:ui-ready"));
}
