// UI2-0926 #10 对话背景(星座背景)的偏好与设置页「对话背景」分组。设计见 docs/design/ui_chat_backdrop.md。
//
// 持久化走 D-404 的后端通道:ui_prefs_set 的 backdrop 字段 → ~/.kanzei/app.json(本机 WebView2 的 localStorage
// 重启即丢,只作缓存与旧值兼容)。启动先用本地缓存 / 默认值立刻渲染,后端值回来后覆盖;用户在后端值回来之前
// 已经动过设置,则以用户为准。「我的图片」只存由图片导出的点集(约 64 颗星、2KB),原图从不落盘。
import { $, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { t } from "./02-i18n.js";
import { toast } from "./03-shell.js";
import { BACKDROP_PRESETS, normalizeBackdropPrefs, resolveBackdropModel, serializeBackdropPrefs } from "./22-constellation-core.js";
import { KANZEI_LOGO_STROKES, STAR_PRESETS } from "./22-constellation-data.js";
import { imageFileToConstellation, renderConstellationThumb } from "./22-constellation.js";

const KEY = "kz-backdrop";
const DATA = { KANZEI_LOGO_STROKES, STAR_PRESETS };
let current = null;
let touched = false;

function hasLocal() {
  try { return localStorage.getItem(KEY) !== null; } catch { return false; }
}
function remember(prefs) {
  try { localStorage.setItem(KEY, JSON.stringify(serializeBackdropPrefs(prefs))); } catch { /* 缓存写不进去不影响本窗口 */ }
}
function publish() {
  document.documentElement.dataset.backdrop = current.enabled ? "on" : "off";
  document.dispatchEvent(new CustomEvent("kz:backdrop-settings", { detail: current }));
}

export function readBackdropPrefs() {
  if (!current) {
    let saved = null;
    try { saved = JSON.parse(localStorage.getItem(KEY) || "null"); } catch { saved = null; }
    current = normalizeBackdropPrefs(saved);
  }
  return current;
}

export function updateBackdropPrefs(patch) {
  touched = true;
  current = normalizeBackdropPrefs({ ...readBackdropPrefs(), ...patch });
  remember(current);
  void uiPrefsSave({ backdrop: serializeBackdropPrefs(current) });
  publish();
  return current;
}

// 后端权威:app.json 里有就覆盖;没有而本地有旧值,把旧值迁上去一次。
export async function loadBackdropPrefs() {
  const hadLocal = hasLocal();
  const saved = await uiPrefsLoad();
  if (touched) return current;
  if (saved?.backdrop && typeof saved.backdrop === "object") {
    current = normalizeBackdropPrefs(saved.backdrop);
    remember(current);
    publish();
  } else if (hadLocal) {
    void uiPrefsSave({ backdrop: serializeBackdropPrefs(readBackdropPrefs()) });
  }
  return current;
}

function presetLabel(key) {
  if (key === "big-dipper") return t("北斗七星");
  if (key === "orion") return t("猎户座");
  if (key === "cassiopeia") return t("仙后座");
  if (key === "custom") return t("我的图片");
  return "kanzei";
}

export function initBackdropSettings() {
  const group = $("backdrop-settings");
  const enabled = $("set-bg-enabled");
  const presets = $("backdrop-presets");
  const upload = $("backdrop-upload");
  const remove = $("backdrop-remove");
  const file = $("backdrop-file");
  const density = $("set-bg-density");
  const opacity = $("set-bg-opacity");
  const densityValue = $("set-bg-density-value");
  const opacityValue = $("set-bg-opacity-value");
  let renderedKeys = "";
  let thumbKey = "";
  const buttons = () => [...(presets?.querySelectorAll("[data-backdrop-preset]") ?? [])];

  // 选图案 = 想看到它:背景关着时顺手打开。
  function choose(key, focus = false) {
    const prefs = readBackdropPrefs();
    if (key === "custom" && !prefs.custom) return;
    updateBackdropPrefs({ preset: key, enabled: true });
    if (focus) buttons().find((button) => button.dataset.backdropPreset === key)?.focus?.();
  }

  // 单选组的键盘约定:←/↑ 上一个,→/↓ 下一个,Home/End 首尾;移动即选中(roving tabindex)。
  function onKey(event) {
    const keys = buttons().map((button) => button.dataset.backdropPreset);
    const at = keys.indexOf(event.currentTarget?.dataset?.backdropPreset ?? readBackdropPrefs().preset);
    const move = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    let next = -1;
    if (move) next = (at + move + keys.length) % keys.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = keys.length - 1;
    if (next < 0) return;
    event.preventDefault?.();
    choose(keys[next], true);
  }

  function renderPresets(prefs) {
    if (!presets) return;
    const keys = BACKDROP_PRESETS.filter((key) => key !== "custom" || prefs.custom);
    if (keys.join("|") !== renderedKeys) {
      renderedKeys = keys.join("|");
      thumbKey = "";
      presets.replaceChildren(...keys.map((key) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "backdrop-preset";
        button.setAttribute("role", "radio");
        button.dataset.backdropPreset = key;
        const thumb = document.createElement("canvas");
        thumb.className = "backdrop-thumb";
        thumb.setAttribute("aria-hidden", "true");
        const label = document.createElement("span");
        label.textContent = presetLabel(key);
        button.append(thumb, label);
        button.addEventListener("click", () => choose(key));
        button.addEventListener("keydown", onKey);
        return button;
      }));
    }
    for (const button of buttons()) {
      const checked = button.dataset.backdropPreset === prefs.preset;
      button.setAttribute("aria-checked", String(checked));
      button.setAttribute("tabindex", checked ? "0" : "-1");
      const label = button.querySelector("span");
      if (label) label.textContent = presetLabel(button.dataset.backdropPreset);
    }
  }

  // 缩略图用同一个渲染器画静帧;只在分组展开时画,主题或自定义点集变了才重画。
  function renderThumbs(prefs) {
    if (!group?.open) return;
    const key = `${document.documentElement.getAttribute("data-theme") || "dark"}|${renderedKeys}|${prefs.custom?.points.length ?? 0}|${prefs.custom?.points[0]?.join(",") ?? ""}`;
    if (key === thumbKey) return;
    thumbKey = key;
    for (const button of buttons()) {
      const preset = button.dataset.backdropPreset;
      renderConstellationThumb(button.querySelector("canvas"), resolveBackdropModel({ ...prefs, preset, density: 1 }, DATA));
    }
  }

  function reflect() {
    const prefs = readBackdropPrefs();
    if (enabled) enabled.checked = prefs.enabled;
    if (density) density.value = String(Math.round(prefs.density * 100));
    if (opacity) opacity.value = String(Math.round(prefs.opacity * 100));
    if (densityValue) densityValue.textContent = `${Math.round(prefs.density * 100)}%`;
    if (opacityValue) opacityValue.textContent = `${Math.round(prefs.opacity * 100)}%`;
    if (remove) remove.hidden = !prefs.custom;
    renderPresets(prefs);
    renderThumbs(prefs);
  }

  async function onFile() {
    const picked = file?.files?.[0];
    if (file) file.value = "";
    if (!picked) return;
    try {
      const model = await imageFileToConstellation(picked);
      if (!model) {
        toast(t("图片里没有足够清晰的轮廓，换一张试试"));
        return;
      }
      updateBackdropPrefs({ custom: model, preset: "custom", enabled: true });
      toast(t("已生成星座，原图未保存"));
    } catch (error) {
      toast(error?.code === "too-large" ? t("图片超过 20MB，换一张小一点的") : t("图片读取失败"));
    }
  }

  enabled?.addEventListener("change", () => updateBackdropPrefs({ enabled: enabled.checked }));
  density?.addEventListener("input", () => updateBackdropPrefs({ density: Number(density.value) / 100 }));
  opacity?.addEventListener("input", () => updateBackdropPrefs({ opacity: Number(opacity.value) / 100 }));
  upload?.addEventListener("click", () => file?.click?.());
  file?.addEventListener("change", () => void onFile());
  remove?.addEventListener("click", () => {
    const prefs = readBackdropPrefs();
    updateBackdropPrefs({ custom: null, preset: prefs.preset === "custom" ? "kanzei" : prefs.preset });
  });
  group?.addEventListener("toggle", () => renderThumbs(readBackdropPrefs()));
  document.addEventListener("kz:backdrop-settings", reflect);
  new MutationObserver(() => renderThumbs(readBackdropPrefs()))
    .observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  readBackdropPrefs();
  publish();
  void loadBackdropPrefs();
}
