// The default brand is real SVG. The constellation controller owns layout, timing
// and real run events; this renderer only paints its current frame.
import { AGENT_MODULE_PATH, AGENT_ROTATIONS, AGENT_SIGNAL_PATH, agentModuleTransform } from "./00-brand.js";
import { activityBusy, watermarkAlpha } from "./22-constellation-core.js";

const NS = "http://www.w3.org/2000/svg";
let nextClip = 0;
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const rgb = (value) => `rgb(${value.map(Math.round).join(",")})`;
function element(name, attrs = {}) {
  const node = document.createElementNS(NS, name);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
  return node;
}

export function createBrandBackdrop(canvas) {
  const svg = element("svg", { id: "neural-flow-brand", class: "neural-flow-chat kz-brand-backdrop", "aria-hidden": "true" });
  svg.style.display = "none";
  const clipId = `kz-brand-clip-${++nextClip}`;
  const defs = element("defs"), clip = element("clipPath", { id: clipId, clipPathUnits: "userSpaceOnUse" });
  const cutout = element("path", { "clip-rule": "evenodd" });
  clip.append(cutout); defs.append(clip); svg.append(defs);
  const clipped = element("g", { "clip-path": `url(#${clipId})` });
  const group = element("g");
  const modules = AGENT_ROTATIONS.map((angle) => {
    const path = element("path", { d: AGENT_MODULE_PATH, transform: agentModuleTransform(angle), "stroke-width": .65, "stroke-linejoin": "round" });
    group.append(path);
    return path;
  });
  const signal = element("path", { d: AGENT_SIGNAL_PATH, transform: agentModuleTransform(0) });
  group.append(signal);
  const dots = Array.from({ length: 4 }, () => element("circle", { r: .75, opacity: 0 }));
  dots.forEach((dot) => group.append(dot));
  clipped.append(group); svg.append(clipped);
  canvas.after(svg);

  return {
    hide() { svg.style.display = "none"; },
    destroy() { svg.remove(); },
    paint({ layout, size, frame, prefs, kit, model }) {
      const { palette } = kit;
      svg.style.display = "block";
      svg.setAttribute("viewBox", `0 0 ${size.w} ${size.h}`);
      svg.setAttribute("data-activity", frame.activity);
      const holes = !layout.capped ? (layout.avoid ?? []).map((r) => `M${r.x} ${r.y}h${r.w}v${r.h}h${-r.w}Z`).join("") : "";
      const clipBox = layout.clip ?? { x: 0, y: 0, w: size.w, h: size.h };
      cutout.setAttribute("d", `M${clipBox.x} ${clipBox.y}h${clipBox.w}v${clipBox.h}h${-clipBox.w}Z${holes}`);
      const scale = Math.max(layout.box.w, layout.box.h);
      group.setAttribute("transform", `translate(${layout.box.x + (layout.box.w - scale) / 2} ${layout.box.y + (layout.box.h - scale) / 2}) scale(${scale / 64})`);
      // One opacity on the whole SVG group, so overlapping paths cannot breach the text cap.
      clipped.setAttribute("opacity", String(layout.capped ? watermarkAlpha(kit.watermark, prefs.opacity)
        : clamp(layout.alpha * prefs.opacity, 0, 1)));
      const busy = activityBusy(frame.activity);
      const completed = frame.activity === "complete";
      const wave = frame.lit ? Math.max(0, ...frame.lit) : 0;
      const colors = [palette.pulse, palette.star, palette.line];
      modules.forEach((path, index) => {
        const pulse = busy && !frame.still ? (1 + Math.sin(frame.t / 420 - index * 2.1)) / 2 : busy ? .7 : 0;
        path.setAttribute("fill", rgb(palette.line));
        path.setAttribute("fill-opacity", String(.1 + pulse * .07));
        path.setAttribute("stroke", rgb(frame.activity === "blocked" ? palette.error : completed || wave > .1 ? palette.star : colors[index]));
        path.setAttribute("stroke-opacity", String(clamp((.48 + pulse * .45 + wave * .3 + (completed ? .25 : 0)) * frame.drawIn * frame.voiceGain, 0, 1)));
        path.setAttribute("pathLength", "1");
        path.setAttribute("stroke-dasharray", `${frame.drawIn} 1`);
      });
      signal.setAttribute("fill", rgb(frame.activity === "blocked" ? palette.error : palette.star));
      signal.setAttribute("opacity", String(frame.drawIn * (busy ? .95 : .65)));
      dots.forEach((dot, index) => {
        const comet = frame.comets[index];
        const progress = comet ? (frame.t - comet.start) / comet.perEdge : -1;
        const edge = comet?.route[Math.floor(progress)];
        if (!edge) { dot.setAttribute("opacity", "0"); return; }
        const a = model.points[edge[0]], b = model.points[edge[1]], p = progress % 1;
        dot.setAttribute("cx", String((a[0] + (b[0] - a[0]) * p) * 64));
        dot.setAttribute("cy", String((a[1] + (b[1] - a[1]) * p) * 64));
        dot.setAttribute("fill", rgb(index % 2 ? palette.pulse : palette.star));
        dot.setAttribute("opacity", "1");
      });
    },
  };
}
