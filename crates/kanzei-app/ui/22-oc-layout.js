import { readOcSettings, updateOcSettings } from "./22-oc-preference.js";

const clamp = (n, low, high) => Math.max(low, Math.min(high, n));
const contextFor = figure => figure.closest(".voice-art") ? "voice" : figure.closest("#oc-companion") ? "conversation" : "welcome";

// Offsets are relative to the chat viewport; each layout retains its own position.
export function fitOcPlacement(base, bounds, settings, position) {
  const scale = Math.max(.1, Math.min(settings.scale, (bounds.width - 24) / base.width, (bounds.height - 24) / base.height));
  const width = base.width * scale, height = base.height * scale;
  const left = base.left + (base.width - width) / 2, top = base.top + base.height - height;
  const x = clamp(position.x * bounds.width, bounds.left + 12 - left, bounds.right - 12 - left - width);
  const y = clamp(position.y * bounds.height, bounds.top + 12 - top, bounds.bottom - 12 - top - height);
  return { x, y, scale, width, height, left: left + x, top: top + y };
}

export function initOcLayout(root, changed) {
  let active = null, drag = null, request = null, pending = null;
  const figures = new Map();
  const label = (id, fallback) => document.getElementById(id)?.textContent.trim() || fallback;

  function geometry(figure) {
    const rect = figure.getBoundingClientRect(), prior = figures.get(figure)?.applied || { x: 0, y: 0, scale: 1 };
    const width = rect.width / prior.scale, height = rect.height / prior.scale;
    return {
      base: { width, height, left: rect.left - prior.x - (width - width * prior.scale) / 2,
        top: rect.top - prior.y - height * (1 - prior.scale) },
      bounds: root.getBoundingClientRect(),
    };
  }
  function apply(figure) {
    if (!figure?.isConnected || !figure.getClientRects().length) return;
    const settings = readOcSettings();
    let entry = figures.get(figure);
    if (!entry) {
      const handle = document.createElement("button");
      handle.type = "button"; handle.className = "oc-resize";
      handle.innerHTML = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 12 12 4M8 12l4-4" /></svg>';
      figure.append(handle); entry = { handle, applied: null }; figures.set(figure, entry);
      figure.closest(".empty-art, .voice-art, #oc-companion")?.removeAttribute("aria-hidden");
      figure.setAttribute("role", "group");
    }
    const { base, bounds } = geometry(figure);
    if (!base.width || !base.height || bounds.width <= 24 || bounds.height <= 24) return;
    const placed = fitOcPlacement(base, bounds, settings, settings.positions[contextFor(figure)]);
    entry.applied = placed;
    figure.style.transform = `translate(${placed.x}px, ${placed.y}px) scale(${placed.scale})`;
    figure.style.opacity = String(settings.opacity);
    figure.dataset.ocLocked = String(settings.locked);
    figure.tabIndex = settings.locked ? -1 : 0;
    figure.title = label("oc-adjust-help", "Drag to move; scroll or use the corner to resize.");
    figure.setAttribute("aria-label", figure.title);
    entry.handle.setAttribute("aria-label", label("oc-resize-label", "Resize character"));
    entry.handle.tabIndex = settings.locked ? -1 : 0;
    entry.handle.disabled = settings.locked;
  }
  function refresh(host) {
    const figure = host?.closest(".oc-figure") || null;
    if (drag && figure !== active) finish(true);
    active = figure;
    if (active) apply(active);
    for (const [node] of figures) if (!node.isConnected) figures.delete(node);
  }
  function updatePosition(name, x, y, persist = false) {
    updateOcSettings({ positions: { [name]: { x, y } } }, { persist });
  }
  function move() {
    request = null;
    if (!drag || !pending) return;
    const { base, bounds } = drag.geometry;
    const dx = pending.x - drag.start.x, dy = pending.y - drag.start.y;
    if (drag.resize) {
      const height = clamp(drag.rect.height + (dx * 2 / 3 + dy) / (1 + 4 / 9), base.height * .6, base.height * 1.8);
      const scale = Math.min(height / base.height, (bounds.right - 12 - drag.rect.left) / base.width,
        (bounds.bottom - 12 - drag.rect.top) / base.height);
      const x = (drag.rect.left + base.width * scale / 2 - base.left - base.width / 2) / bounds.width;
      const y = (drag.rect.top + base.height * scale - base.top - base.height) / bounds.height;
      updateOcSettings({ scale, positions: { [drag.name]: { x, y } } }, { persist: false });
    } else updatePosition(drag.name, (drag.offset.x + dx) / bounds.width, (drag.offset.y + dy) / bounds.height);
    pending = null;
  }
  function finish(cancel = false) {
    if (!drag) return;
    if (request !== null) cancelAnimationFrame(request);
    request = null;
    if (!cancel) move();
    const previous = drag; drag = null; pending = null;
    previous.figure.classList.remove("oc-adjusting");
    if (previous.figure.hasPointerCapture?.(previous.pointerId)) previous.figure.releasePointerCapture(previous.pointerId);
    if (cancel) updateOcSettings({ scale: previous.settings.scale, positions: { [previous.name]: previous.settings.positions[previous.name] } });
    else {
      const { bounds } = geometry(previous.figure), offset = figures.get(previous.figure).applied;
      updatePosition(previous.name, offset.x / bounds.width, offset.y / bounds.height, true);
    }
  }
  function down(event) {
    const figure = event.target.closest?.(".oc-figure"), settings = readOcSettings();
    if (event.button !== 0 || !figure || figure !== active || settings.locked || !settings.enabled || drag) return;
    event.preventDefault(); event.stopPropagation();
    apply(figure);
    drag = { figure, pointerId: event.pointerId, resize: Boolean(event.target.closest(".oc-resize")),
      name: contextFor(figure), settings, geometry: geometry(figure), rect: figure.getBoundingClientRect(),
      offset: figures.get(figure).applied, start: { x: event.clientX, y: event.clientY } };
    figure.classList.add("oc-adjusting"); figure.focus({ preventScroll: true }); figure.setPointerCapture(event.pointerId);
  }
  function pointerMove(event) {
    if (!drag || event.pointerId !== drag.pointerId) return;
    event.preventDefault(); pending = { x: event.clientX, y: event.clientY };
    if (request === null) request = requestAnimationFrame(move);
  }
  function up(event) { if (drag?.pointerId === event.pointerId) finish(event.type !== "pointerup"); }
  function wheel(event) {
    if (!active?.contains(event.target) || readOcSettings().locked || event.ctrlKey || event.metaKey) return;
    event.preventDefault(); event.stopPropagation();
    updateOcSettings({ scale: Math.round((readOcSettings().scale + (event.deltaY < 0 ? .05 : -.05)) * 100) / 100 });
  }
  function keyboard(event) {
    if (!active?.contains(event.target) || readOcSettings().locked) return;
    const settings = readOcSettings(), name = contextFor(active), { bounds } = geometry(active);
    const step = event.shiftKey ? 24 : 8, offset = figures.get(active).applied;
    const arrows = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    if (event.key === "Escape" && drag) finish(true);
    else if (event.key === "Home") updateOcSettings({ scale: 1, positions: { [name]: { x: 0, y: 0 } } });
    else if (["+", "=", "-"].includes(event.key)) updateOcSettings({ scale: settings.scale + (event.key === "-" ? -.05 : .05) });
    else if (arrows[event.key]) updatePosition(name, (offset.x + arrows[event.key][0]) / bounds.width, (offset.y + arrows[event.key][1]) / bounds.height, true);
    else return;
    event.preventDefault(); event.stopPropagation();
  }
  function settingsChanged() {
    if (drag && (readOcSettings().locked || !readOcSettings().enabled)) finish(true);
    if (active) apply(active);
    changed();
  }
  function blur() { finish(true); }
  root.addEventListener("pointerdown", down); root.addEventListener("pointermove", pointerMove);
  root.addEventListener("pointerup", up); root.addEventListener("pointercancel", up); root.addEventListener("lostpointercapture", up);
  root.addEventListener("wheel", wheel, { passive: false }); root.addEventListener("keydown", keyboard);
  window.addEventListener("blur", blur); document.addEventListener("kz:oc-settings", settingsChanged);
  return {
    refresh,
    destroy() {
      finish(true);
      for (const [figure, entry] of figures) { entry.handle.remove(); figure.style.removeProperty("transform"); figure.style.removeProperty("opacity"); }
      root.removeEventListener("pointerdown", down); root.removeEventListener("pointermove", pointerMove);
      root.removeEventListener("pointerup", up); root.removeEventListener("pointercancel", up); root.removeEventListener("lostpointercapture", up);
      root.removeEventListener("wheel", wheel); root.removeEventListener("keydown", keyboard);
      window.removeEventListener("blur", blur); document.removeEventListener("kz:oc-settings", settingsChanged);
    },
  };
}
