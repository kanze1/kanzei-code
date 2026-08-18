// kanzei 移动端 service worker(R-271 批3):可添加主屏、离线时给明确提示。
// 不做离线数据——只做缓存壳页面,离线时显示「需要网络」提示而非白屏。
// R-292:离线提示文案走最小 i18n(按 navigator.language 选中/英)。

const CACHE_NAME = "kanzei-shell-v1";
// 壳资源:离线时仍能加载,提示用户需要网络(不做离线数据)。
const SHELL = ["/", "/index.html", "/style.css", "/app.js", "/manifest.json"];

// ---- 离线提示最小 i18n(R-292)----
const I18N_OFFLINE_EN = {
  "离线": "Offline",
  "当前离线": "Offline now",
  "需要网络连接才能使用 kanzei 移动端。请检查 Wi-Fi 或桥接服务。":
    "A network connection is required to use kanzei mobile. Check your Wi-Fi or the bridge service.",
};
function offlineText(key) {
  const lang = ((self.navigator && self.navigator.language) || "zh")
    .toLowerCase()
    .startsWith("zh")
    ? "zh"
    : "en";
  return lang === "en" ? (I18N_OFFLINE_EN[key] || key) : key;
}

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll(SHELL))
      .catch(() => {})
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
      )
  );
  self.clients.claim();
});

// 网络优先;失败(离线)回退缓存壳,并给页面一个「离线」标记(由 app.js 读取)。
self.addEventListener("fetch", (event) => {
  const { request } = event;
  // 只处理 GET 与同源请求;API(v1/*)与跨域不缓存。
  if (request.method !== "GET" || !request.url.startsWith(self.location.origin)) {
    return;
  }
  if (new URL(request.url).pathname.startsWith("/v1/")) {
    return;
  }
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(request, copy));
        }
        return response;
      })
      .catch(() =>
        caches.match(request).then((cached) => {
          if (cached) {
            // 缓存命中:在线状态未知,页面自己处理离线提示。
            return cached;
          }
          // 壳资源已缓存;其它路径离线给一个可读提示页。
          return new Response(
            "<!DOCTYPE html><html><head><meta charset='utf-8'><title>" +
              offlineText("离线") +
              "</title></head>" +
              "<body style='font-family:system-ui;padding:24px'>" +
              "<h2>" +
              offlineText("当前离线") +
              "</h2><p>" +
              offlineText("需要网络连接才能使用 kanzei 移动端。请检查 Wi-Fi 或桥接服务。") +
              "</p></body></html>",
            { headers: { "Content-Type": "text/html; charset=utf-8" } }
          );
        })
      )
  );
});
