const ENABLED_KEY = "kanzei.character.enabled";
const SETTINGS_KEY = "kanzei.character.preferences.v1";
let current = null;
const clamp = (value, low, high, fallback) => Number.isFinite(value) ? Math.max(low, Math.min(high, value)) : fallback;

export function normalizeOcSettings(value = {}) {
  const settings = value && typeof value === "object" ? value : {};
  return {
    enabled: settings.enabled === true,
    scale: clamp(settings.scale, .6, 1.8, 1),
    opacity: clamp(settings.opacity, .4, 1, 1),
    motion: ["full", "idle", "still"].includes(settings.motion) ? settings.motion : "full",
    locked: settings.locked === true,
    positions: Object.fromEntries(["welcome", "conversation", "voice"].map(name => [name, {
      x: clamp(settings.positions?.[name]?.x, -1, 1, 0),
      y: clamp(settings.positions?.[name]?.y, -1, 1, 0),
    }])),
  };
}

export function readOcSettings() {
  if (!current) {
    let saved = {}, enabled = false;
    try { saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) || "{}"); } catch { /* Use defaults. */ }
    try { enabled = localStorage.getItem(ENABLED_KEY) === "true"; } catch { /* Local storage can be disabled. */ }
    current = normalizeOcSettings({ ...saved, enabled });
  }
  return normalizeOcSettings(current);
}

export function updateOcSettings(patch, { persist = true } = {}) {
  const previous = readOcSettings();
  current = normalizeOcSettings({ ...previous, ...patch, positions: { ...previous.positions, ...patch.positions } });
  if (persist) {
    try {
      localStorage.setItem(ENABLED_KEY, String(current.enabled));
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(current));
    } catch { /* Preserve the working controls for this window. */ }
  }
  document.dispatchEvent(new CustomEvent("kz:oc-settings", { detail: readOcSettings() }));
  return readOcSettings();
}

export function resetOcPlacement() {
  return updateOcSettings({ scale: 1, positions: normalizeOcSettings().positions });
}

export function isOcEnabled() { return readOcSettings().enabled; }

export function initOcPreference(button) {
  let previousEnabled = null;
  const controls = document.getElementById("oc-settings");
  function reflect() {
    const settings = readOcSettings();
    const { enabled } = settings;
    document.documentElement.dataset.ocEnabled = String(enabled);
    button?.setAttribute("aria-pressed", String(enabled));
    for (const name of ["enabled", "locked", "scale", "opacity", "motion"]) {
      const input = controls?.querySelector(`[data-oc-setting="${name}"]`);
      if (!input) continue;
      if (input.type === "checkbox") input.checked = settings[name];
      else input.value = ["scale", "opacity"].includes(name) ? String(Math.round(settings[name] * 100)) : settings[name];
      const output = controls.querySelector(`[data-oc-value="${name}"]`);
      if (output) output.textContent = input.value + "%";
    }
    if (previousEnabled !== enabled) {
      previousEnabled = enabled;
      document.dispatchEvent(new CustomEvent("kz:oc-preference", { detail: { enabled } }));
    }
  }
  button?.addEventListener("click", () => updateOcSettings({ enabled: !isOcEnabled() }));
  controls?.addEventListener("input", event => {
    const input = event.target.closest("[data-oc-setting]");
    if (!input) return;
    const key = input.dataset.ocSetting;
    const value = input.type === "checkbox" ? input.checked : ["scale", "opacity"].includes(key) ? Number(input.value) / 100 : input.value;
    updateOcSettings({ [key]: value });
  });
  controls?.querySelector("#oc-reset-placement")?.addEventListener("click", resetOcPlacement);
  document.addEventListener("kz:oc-settings", reflect);
  window.addEventListener("storage", event => {
    if (event.key === SETTINGS_KEY || event.key === ENABLED_KEY || event.key === null) {
      current = null;
      document.dispatchEvent(new CustomEvent("kz:oc-settings", { detail: readOcSettings() }));
    }
  });
  reflect();
}
