// One lazy loader for optional interfaces. Import failure remains retryable.
import { $, defer } from "./01-core.js";
import { toastError, currentProject } from "./03-shell.js";
const flights = new Map();
const loaded = new Set();
export function loadOptional(name, load) {
  if (!flights.has(name)) flights.set(name, load().then(async module => {
    // Imported modules install deferred controls after their dependency graph evaluates.
    await new Promise(resolve => setTimeout(resolve, 0));
    loaded.add(name);
    return module;
  }).catch(error => { flights.delete(name); throw error; }));
  return flights.get(name);
}
const chat = () => loadOptional("memory-chat", () => import("./13-memory-chat.js"));
const graph = () => loadOptional("memory-graph", () => import("./24-memory-graph.js"));
const voice = () => loadOptional("voice", () => import("./23-voice.js"));
const mobile = () => loadOptional("mobile", () => import("./16-mobile.js"));

defer(() => {
  let selection = null;
  document.addEventListener("kz:memory-selected", event => { selection = event.detail; });
  document.addEventListener("kz:memory-selection-cleared", () => { selection = null; });
  document.addEventListener("kz:memory-chat-open", () => {
    if (loaded.has("memory-chat")) return;
    const project = currentProject;
    void chat().then(() => {
      if (currentProject !== project) return;
      if (selection?.project === project) document.dispatchEvent(new CustomEvent("kz:memory-selected", { detail: selection }));
      document.dispatchEvent(new CustomEvent("kz:memory-chat-open"));
    }).catch(error => toastError(String(error)));
  });
  function firstClick(id, name, load) {
    const button = $(id);
    button?.addEventListener("click", event => {
      if (loaded.has(name)) return;
      event.preventDefault(); event.stopImmediatePropagation();
      const project = currentProject;
      if (button.disabled) return;
      button.disabled = true;
      void load().then(() => {
        button.disabled = false;
        if (currentProject === project) button.click();
      }).catch(error => { button.disabled = false; toastError(String(error)); });
    }, true);
  }
  firstClick("memory-view-graph", "memory-graph", graph);
  firstClick("voice-toggle", "voice", voice);
  firstClick("voice-check", "voice", voice);
  firstClick("mobile-service-start", "mobile", mobile);
  $("mobile-settings")?.addEventListener("toggle", event => {
    if (event.target.open) void mobile().catch(error => toastError(String(error)));
  });
});
