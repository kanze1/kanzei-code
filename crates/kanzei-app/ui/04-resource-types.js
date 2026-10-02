// One visual vocabulary for input attachments, links and delivered files.
const GROUPS = {
  sheet: ["xlsx", "xls", "xlsm", "xlsb", "ods", "csv", "tsv"],
  pdf: ["pdf"], image: ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tif", "tiff", "avif", "heic"],
  document: ["doc", "docx", "odt", "rtf", "txt", "log"], markdown: ["md", "markdown", "rst"],
  slides: ["ppt", "pptx", "odp"], archive: ["zip", "7z", "rar", "gz", "tar", "bz2", "xz"],
  code: ["js", "jsx", "ts", "tsx", "py", "rs", "go", "java", "c", "cpp", "h", "cs", "html", "css", "json", "yaml", "yml", "toml", "xml", "sql", "sh", "ps1", "bat", "cmd"],
  audio: ["mp3", "wav", "ogg", "flac", "m4a", "aac"], video: ["mp4", "mov", "avi", "webm", "mkv"],
  app: ["apk", "aab", "exe", "msi", "dmg", "pkg", "deb", "appimage"], font: ["ttf", "otf", "woff", "woff2"],
};
const LABELS = { sheet: "表格", pdf: "PDF", image: "图片", document: "文档", markdown: "Markdown", slides: "演示文稿", archive: "压缩包", code: "代码", audio: "音频", video: "视频", app: "应用", font: "字体", git: "Git", web: "网页", file: "文件" };
const PATHS = {
  sheet: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M3 15h18M9 9v12M15 9v12"/>',
  pdf: '<path d="M14 3H5v18h14V8zM14 3v5h5M8 16c4-7 1-7 2-3s6 2 5 3-5-2-7 0Z"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8" cy="8" r="1.5"/><path d="m3 17 6-6 4 4 3-3 5 5"/>',
  document: '<path d="M14 3H5v18h14V8zM14 3v5h5M8 12h8M8 16h6"/>',
  markdown: '<rect x="2" y="5" width="20" height="14" rx="2"/><path d="M5 15V9l3 3 3-3v6M17 9v6m-2-2 2 2 2-2"/>',
  slides: '<rect x="3" y="3" width="18" height="13" rx="1"/><path d="M12 16v5m-4 0 4-3 4 3M7 12l4-4 3 2 3-4"/>',
  archive: '<path d="M14 3H5v18h14V8zM14 3v5h5M10 3v3h2v3h-2v3h2v3h-2v3"/>',
  code: '<path d="m8 6-6 6 6 6m8-12 6 6-6 6m-3-14-2 16"/>',
  audio: '<path d="M9 18V5l11-2v13M9 8l11-2"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="17" cy="16" rx="3" ry="3"/>',
  video: '<rect x="2" y="5" width="14" height="14" rx="2"/><path d="m16 10 6-4v12l-6-4"/>',
  app: '<rect x="3" y="3" width="18" height="18" rx="4"/><path d="M8 8h3v3H8zm5 0h3v3h-3zm-5 5h3v3H8zm5 0h3v3h-3z"/>',
  font: '<path d="m5 20 7-16 7 16M8 14h8M3 20h4m10 0h4"/>',
  git: '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="6" r="2"/><path d="M6 7v10m12-9v3a4 4 0 0 1-4 4H6"/>',
  web: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z"/>',
  file: '<path d="M14 3H5v18h14V8zM14 3v5h5"/>',
};
const MIMES = { pdf:"application/pdf", xlsx:"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", xls:"application/vnd.ms-excel", xlsm:"application/vnd.ms-excel.sheet.macroEnabled.12", xlsb:"application/vnd.ms-excel.sheet.binary.macroEnabled.12", ods:"application/vnd.oasis.opendocument.spreadsheet", csv:"text/csv", tsv:"text/tab-separated-values", txt:"text/plain", md:"text/markdown", markdown:"text/markdown", json:"application/json", log:"text/plain", png:"image/png", jpg:"image/jpeg", jpeg:"image/jpeg", gif:"image/gif", webp:"image/webp", svg:"image/svg+xml", bmp:"image/bmp", avif:"image/avif", tif:"image/tiff", tiff:"image/tiff", heic:"image/heic", ico:"image/x-icon" };
function extension(value) { return String(value || "").split(/[?#]/)[0].replace(/:\d+(?:-\d+)?$/, "").split(/[\\/]/).at(-1)?.split(".").at(-1)?.toLowerCase() || ""; }
export function attachmentMime(name, mime = "") { return MIMES[extension(name)] || (mime.startsWith("image/") || mime === "application/pdf" ? mime : null); }
export function resourceType(name, mime = "") {
  let path = String(name || ""), host = "";
  try { const url = new URL(path); if (["http:", "https:"].includes(url.protocol)) { path = decodeURIComponent(url.pathname); host = url.hostname.toLowerCase(); } } catch { /* A file path is not a URL. */ }
  if (/^(?:www\.)?(?:github\.com|gitlab\.com|bitbucket\.org|gitee\.com)$/.test(host)) return { kind:"git", label:host.replace(/^www\./, "") };
  const ext = extension(path);
  const kind = Object.keys(GROUPS).find(key => GROUPS[key].includes(ext)) || (mime.startsWith("image/") ? "image" : mime === "application/pdf" ? "pdf" : host ? "web" : "file");
  return { kind, label: LABELS[kind] };
}
export function resourceIconMarkup(name, mime = "") {
  const { kind } = resourceType(name, mime);
  return `<svg class="resource-icon" data-kind="${kind}" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${PATHS[kind]}</svg>`;
}
export function resourceIcon(name, mime = "") { const el = document.createElement("span"); el.className = "resource-mark"; el.innerHTML = resourceIconMarkup(name, mime); return el; }
export function resourceLinks(value) {
  const text = String(value || ""), links = [];
  const pattern = /(?:https?:\/\/|www\.|(?:github\.com|gitlab\.com|bitbucket\.org|gitee\.com)\/)[^\s<>"'`，。；）】」、]+/gi;
  for (const match of text.matchAll(pattern)) {
    let raw = match[0].replace(/[.,;:!?]+$/, "");
    while (raw.endsWith(")") && (raw.match(/\)/g)?.length || 0) > (raw.match(/\(/g)?.length || 0)) raw = raw.slice(0,-1);
    while (raw.endsWith("]")) raw = raw.slice(0,-1);
    try {
      const url = new URL(/^https?:\/\//i.test(raw) ? raw : `https://${raw}`);
      if (!["http:","https:"].includes(url.protocol) || !url.hostname) continue;
      links.push({ start:match.index, end:match.index+raw.length, text:raw, url:url.href, label:`${url.hostname}${url.pathname === "/" ? "" : url.pathname}` });
    } catch { /* Keep incomplete URLs as normal text. */ }
  }
  return links;
}
export function appendLinkedText(element, text) {
  let cursor = 0;
  for (const link of resourceLinks(text)) {
    element.append(document.createTextNode(text.slice(cursor, link.start)));
    const anchor = document.createElement("a"); anchor.href = link.url; anchor.target = "_blank"; anchor.rel = "noopener noreferrer"; anchor.className = "resource-link";
    anchor.append(resourceIcon(link.url), document.createTextNode(link.text)); element.append(anchor); cursor = link.end;
  }
  element.append(document.createTextNode(text.slice(cursor)));
}
