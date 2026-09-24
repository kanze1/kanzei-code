const KEY = "kanzei.character.enabled";

export function isOcEnabled() {
  try { return localStorage.getItem(KEY) === "true"; } catch { return false; }
}

export function initOcPreference(button) {
  function reflect() {
    const enabled = isOcEnabled();
    document.documentElement.dataset.ocEnabled = String(enabled);
    button?.setAttribute("aria-pressed", String(enabled));
    document.dispatchEvent(new CustomEvent("kz:oc-preference", {detail:{enabled}}));
  }
  button?.addEventListener("click", () => {
    const enabled = button.getAttribute("aria-pressed") !== "true";
    try { localStorage.setItem(KEY, String(enabled)); } catch { /* Keep the current-window control usable. */ }
    document.documentElement.dataset.ocEnabled = String(enabled);
    button.setAttribute("aria-pressed", String(enabled));
    document.dispatchEvent(new CustomEvent("kz:oc-preference", {detail:{enabled}}));
  });
  window.addEventListener("storage", event => { if (event.key === KEY || event.key === null) reflect(); });
  reflect();
}
