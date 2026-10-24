const $ = id => document.getElementById(id);
const frame = $("product");
const query = new URLSearchParams(location.search);
const sceneNames = ["chat", "empty", "work", "memory", "skills", "settings"];
const state = {
  scene: sceneNames.includes(query.get("scene")) ? query.get("scene") : "chat",
  theme: query.get("theme") === "dark" ? "dark" : "light",
  baseline: false, narrow: false,
};
let request = 0;
const captions = {
  chat: "橙色点亮当前对话与工作选择；活动图标和运行数并排。",
  empty: "欢迎区靠近输入；只留一句自动模式说明。",
  work: "页签悬停收在文字附近。橙色标记当前工作区，青色点亮内容与完成。",
  memory: "信息有层级，工具条与分隔线减轻。",
  skills: "已启用的青色开关，为整齐的列表增加一点亮色。",
  settings: "当前分组以橙色短线标记，保留原有底色与文字。",
};
function paint() {
  document.documentElement.dataset.theme = state.theme;
  document.body.dataset.narrow = String(state.narrow);
  $("theme").textContent = state.theme === "light" ? "深色" : "浅色";
  $("theme").title = state.theme === "light" ? "切换到深色" : "切换到浅色";
  $("theme").setAttribute("aria-pressed", String(state.theme === "dark"));
  $("baseline").textContent = state.baseline ? "返回新版" : "发行版对照";
  $("baseline").setAttribute("aria-pressed", String(state.baseline));
  $("narrow").setAttribute("aria-pressed", String(state.narrow));
  $("caption").textContent = state.baseline ? "发行版对照 · build-8fb9f71d · 相同示例数据。" : captions[state.scene];
  for (const button of document.querySelectorAll("[data-scene]")) button.setAttribute("aria-pressed", String(button.dataset.scene === state.scene));
  const sheet = frame.contentDocument?.getElementById("visual-proposal");
  if (sheet) sheet.disabled = state.baseline;
  for (const element of frame.contentDocument?.querySelectorAll("[data-prototype-only]") || []) element.hidden = state.baseline;
}
function loadScene(scene) {
  state.scene = scene;
  document.body.dataset.ready = "false";
  $("loading").textContent = "正在准备预览…";
  request += 1;
  const underlying = ["work", "skills"].includes(scene) ? "chat" : scene;
  const params = new URLSearchParams({ scene: underlying, theme: state.theme, visualScene: scene, visualRequest: request });
  frame.src = `/?${params}`;
  const next = new URL(location.href); next.searchParams.set("scene", scene); next.searchParams.set("theme", state.theme);
  history.replaceState(null, "", next);
  paint();
}
frame.addEventListener("load", async () => {
  const doc = frame.contentDocument;
  if (!doc || !frame.contentWindow.__kzPreview) return;
  const sheet = doc.createElement("link"); sheet.id = "visual-proposal"; sheet.rel = "stylesheet"; sheet.href = "/__visual-balance/proposal.css";
  const loaded = new Promise((resolve, reject) => {
    sheet.addEventListener("load", resolve, { once: true });
    sheet.addEventListener("error", () => reject(new Error("样式加载失败")), { once: true });
  });
  doc.head.append(sheet);
  await loaded;
  if (doc !== frame.contentDocument) return;
  sheet.disabled = state.baseline;
  const script = doc.createElement("script"); script.type = "module"; script.src = "/__visual-balance/preview.js"; doc.head.append(script);
});
window.addEventListener("message", event => {
  if (event.origin !== location.origin || event.source !== frame.contentWindow) return;
  if (event.data.type === "visual-ready" && event.data.request === request) {
    document.body.dataset.ready = "true"; paint();
  } else if (event.data.type === "visual-theme") { state.theme = event.data.theme; paint(); }
  else if (event.data.type === "visual-error" && event.data.request === request) $("loading").textContent = `预览加载失败：${event.data.message}`;
});
for (const button of document.querySelectorAll("[data-scene]")) button.addEventListener("click", () => loadScene(button.dataset.scene));
$("theme").addEventListener("click", () => {
  state.theme = state.theme === "light" ? "dark" : "light";
  frame.contentWindow.postMessage({ type: "visual-set-theme", theme: state.theme }, location.origin); paint();
});
$("baseline").addEventListener("click", () => { state.baseline = !state.baseline; paint(); });
$("narrow").addEventListener("click", () => { state.narrow = !state.narrow; paint(); });
loadScene(state.scene);
