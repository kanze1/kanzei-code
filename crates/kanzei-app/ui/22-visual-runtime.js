import { $ } from "./01-core.js";
import { activeSessionId, sessionStates } from "./03-shell.js";
import { createConstellationBackdrop } from "./22-constellation.js";
import { initBackdropSettings, readBackdropPrefs } from "./22-constellation-prefs.js";

let preferencesInitialized = false;
export function createVisualRuntime() {
  const waitForPrefs = !preferencesInitialized;
  if (!preferencesInitialized) {
    preferencesInitialized = true;
    initBackdropSettings();
  }
  document.documentElement.dataset.backdrop = readBackdropPrefs().enabled ? "on" : "off";
  const runtimeSource = {
    getSessionId: () => activeSessionId,
    getRuntime: (id) => sessionStates.get(id),
  };
  const chatBackdrop = createConstellationBackdrop($("neural-flow-chat"), { ...runtimeSource, prefs: readBackdropPrefs(), waitForPrefs });
  const voice = (sessionId, phase, level) => {
    chatBackdrop?.voice(sessionId, phase, level);
  };
  const emit = (eventType, detail = {}) => {
    chatBackdrop?.emit(eventType, detail);
  };
  function destroy() {
    chatBackdrop?.destroy();
    window.removeEventListener("beforeunload", destroy);
  }
  window.addEventListener("beforeunload", destroy);
  return { emit, voice, backdrop: chatBackdrop, destroy };
}
