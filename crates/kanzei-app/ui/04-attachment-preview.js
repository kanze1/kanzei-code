// Attachments already carry user-selected/authorized bytes. Never resolve a name/path,
// navigate to a URL, or insert attachment MIME/data as markup to preview them.
import { t } from "./02-i18n.js";
import { resourceIcon } from "./04-resource-types.js";

const MAX_BASE64 = Math.ceil(16 * 1024 * 1024 / 3) * 4;
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
  button.addEventListener("click", async () => {
    const { openRuntimeImage } = await import("./15-views-misc.js");
    openRuntimeImage(attachment.file_name, src);
  });
  return button;
}
