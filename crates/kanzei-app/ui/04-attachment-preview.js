// Attachments already carry user-selected/authorized bytes. Never resolve a name/path,
// navigate to a URL, or insert attachment MIME/data as markup to preview them.
import { el } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { currentProject, toast } from "./03-shell.js";
import { resourceIcon } from "./04-resource-types.js";
import { openRuntimeImage } from "./15-views-misc.js";

const MAX_BASE64 = Math.ceil(16 * 1024 * 1024 / 3) * 4;
const TOOL_IMAGE_REL = /^\.kanzei\/artifacts\/tool-images\/[0-9a-f]{64}\.png$/;
const TOOL_IMAGE_CACHE_MAX = 64;

export function attachmentImageSource(attachment) {
  const mime = String(attachment?.media_type || "").toLowerCase();
  const data = attachment?.data;
  if (!/^(?:image\/(?:png|jpeg|gif|webp|bmp))$/.test(mime)
      || typeof data !== "string" || !data.length || data.length > MAX_BASE64
      || data.length % 4 !== 0 || !/^[A-Za-z0-9+/]+={0,2}$/.test(data)) return null;
  // Check actual raster magic as well as MIME. SVG/HTML (including a forged PNG
  // MIME) never reaches an image element or the existing enlarged-image viewer.
  let head;
  try { head = atob(data.slice(0, 32)); } catch { return null; }
  const matches = {
    "image/png": head.startsWith("\x89PNG\r\n\x1a\n"),
    "image/jpeg": head.startsWith("\xff\xd8\xff"),
    "image/gif": /^GIF8[79]a/.test(head),
    "image/webp": head.startsWith("RIFF") && head.slice(8, 12) === "WEBP",
    "image/bmp": head.startsWith("BM"),
  };
  return matches[mime] ? `data:${mime};base64,${data}` : null;
}

export function attachmentPreview(attachment) {
  const src = attachmentImageSource(attachment);
  const fallback = () => resourceIcon(attachment.file_name, attachment.media_type);
  if (!src) return fallback();
  const button = document.createElement("button");
  button.type = "button";
  button.className = "attachment-image-preview";
  button.title = `${t("预览")} ${attachment.file_name}`;
  button.setAttribute("aria-label", button.title);
  const image = document.createElement("img");
  image.className = "attachment-thumbnail";
  image.alt = attachment.file_name;
  image.decoding = "async";
  image.src = src;
  image.addEventListener("error", () => button.replaceWith(fallback()), { once: true });
  button.append(image);
  button.addEventListener("click", () => {
    openRuntimeImage(attachment.file_name, src);
  });
  return button;
}

/// 工具正文里的截图标记行(后端 persist_tool_images 追加):`[tool-image] .kanzei/artifacts/tool-images/<sha>.png`。
export function extractToolImages(content) {
  const text = typeof content === "string" ? content : "";
  if (!text.includes("[tool-image]")) return { images: [], content: text };
  const images = [];
  const kept = [];
  for (const line of text.split(/\r?\n/)) {
    const m = line.match(/^\[tool-image\]\s+(\S.*?)\s*$/);
    const rel = m ? m[1].replace(/\\/g, "/") : "";
    if (m && TOOL_IMAGE_REL.test(rel)) {
      if (!images.includes(rel)) images.push(rel);
    } else {
      kept.push(line);
    }
  }
  if (!images.length) return { images, content: text };
  return { images, content: kept.join("\n").replace(/\n+$/, "") };
}

const toolImageCache = new Map();
function cachedImage(key, load) {
  if (toolImageCache.has(key)) return toolImageCache.get(key);
  const promise = load().then((src) => {
    if (!src) toolImageCache.delete(key);
    return src;
  }, () => {
    toolImageCache.delete(key);
    return null;
  });
  toolImageCache.set(key, promise);
  if (toolImageCache.size > TOOL_IMAGE_CACHE_MAX) toolImageCache.delete(toolImageCache.keys().next().value);
  return promise;
}

export function loadToolImage(rel, projectDir = currentProject) {
  return cachedImage(`tool\n${projectDir}\n${rel}`, async () => {
    const result = await invoke("tool_image", { projectDir, rel });
    return typeof result?.png === "string" && result.png ? `data:image/png;base64,${result.png}` : null;
  });
}

let lazyObserver = null;
function whenVisible(node, fn) {
  if (typeof IntersectionObserver !== "function") {
    fn();
    return;
  }
  lazyObserver ??= new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      lazyObserver.unobserve(entry.target);
      entry.target._kzLoad?.();
    }
  }, { rootMargin: "200px" });
  node._kzLoad = fn;
  lazyObserver.observe(node);
}

function thumbButton(label, load, title) {
  const button = el("button", "pv-thumb");
  button.type = "button";
  button.setAttribute("aria-label", label);
  button.title = label;
  const img = el("img", "pv-thumb-img");
  img.alt = "";
  button.append(img);
  whenVisible(button, () => {
    void load().then((src) => {
      if (src) {
        img.setAttribute("src", src);
        button.classList.add("is-ready");
      } else {
        button.classList.add("is-missing");
        button.append(el("span", "pv-thumb-missing", t("图片不可用")));
      }
    });
  });
  button.addEventListener("click", async (event) => {
    event.stopPropagation?.();
    const src = await load();
    if (src) openRuntimeImage(title, src);
    else toast(t("图片不可用(可能已被清理)"), { kind: "warn" });
  });
  return button;
}

export function mountToolShots(block, images) {
  const wrap = block?.wrap;
  if (!wrap) return null;
  for (const old of [...(wrap.children ?? [])]) {
    if (old.classList?.contains("tool-shots")) old.remove();
  }
  wrap.classList.remove("has-shot");
  if (!images?.length) return null;
  const strip = el("div", "tool-shots");
  images.slice(0, 4).forEach((rel, index) => {
    strip.append(thumbButton(`${t("查看截图")} ${index + 1}`, () => loadToolImage(rel), `${t("截图")}  ${block.name ?? ""}`));
  });
  if (block.detail?.parentNode === wrap) wrap.insertBefore(strip, block.detail);
  else wrap.append(strip);
  wrap.classList.add("has-shot");
  return strip;
}
