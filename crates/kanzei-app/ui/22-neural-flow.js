// Minimal feedback is always available. Complex renderers are loaded on demand.
import { $, defer, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { activeSessionId, toastError } from "./03-shell.js";
let runtime = null, flight = null, generation = 0, enabled = false;
export let chatBackdrop = null;
export let neuralFlowEmit = (type, detail = {}) => {
  if (detail.session_id && activeSessionId && detail.session_id !== activeSessionId) { runtime?.emit(type, detail); return; }
  runtime?.emit(type, detail);
};
export let visualVoiceSignal = (sessionId, phase, level) => runtime?.voice(sessionId, phase, level);
export function setNeuralFlowEmit(value) { neuralFlowEmit = value; }
export async function setComplexVisuals(value, { persist = true } = {}) {
  enabled = Boolean(value);
  const revision = ++generation;
  document.documentElement.dataset.complexVisuals = String(enabled);
  if ($("set-visuals-enabled")) $("set-visuals-enabled").checked = enabled;
  for (const id of ["backdrop-settings"]) $(id)?.classList.toggle("hidden", !enabled);
  if (persist) {
    try { localStorage.setItem("kanzei.visuals.enabled", String(enabled)); } catch {}
    void uiPrefsSave({ ui_layout: { modules: { visuals: enabled } } });
  }
  if (!enabled) {
    runtime?.destroy(); runtime = null; chatBackdrop = null;
    document.documentElement.dataset.backdrop = "off";
    return;
  }
  try {
    if (!flight) flight = import("./22-visual-runtime.js").catch(error => { flight = null; throw error; });
    const module = await flight;
    if (!enabled || revision !== generation) return;
    runtime?.destroy(); runtime = module.createVisualRuntime(); chatBackdrop = runtime.backdrop;
  } catch (error) {
    if (revision === generation) {
      await setComplexVisuals(false);
      toastError(String(error));
    }
  }
}
defer(() => {
  document.addEventListener("kz:backdrop-settings", () => { if (!enabled) document.documentElement.dataset.backdrop = "off"; });
  $("set-visuals-enabled")?.addEventListener("change", event => { void setComplexVisuals(event.target.checked); });
  let cached = false;
  try { cached = localStorage.getItem("kanzei.visuals.enabled") === "true"; } catch {}
  void setComplexVisuals(false, { persist: false });
  const initial = generation;
  void uiPrefsLoad().then(prefs => {
    if (generation !== initial) return;
    return setComplexVisuals(prefs.ui_layout?.modules?.visuals ?? cached, { persist: false });
  });
  Object.assign(globalThis, { neuralFlowEmit });
});
