import http from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
const base = new URL("./", import.meta.url);
const repo = new URL("../../../", base);
const prefix = "/docs/prototypes/decision_console/";
const routes = new Map(["index.html", "console.css", "console.js"].map(name => [prefix + name, new URL(name, base)]));
routes.set(prefix, new URL("index.html", base));
routes.set("/", new URL("index.html", base));
for (const path of ["crates/kanzei-app/ui/vendor/force-graph/force-graph-1.51.4.min.js", "crates/kanzei-app/ui/assets/kanzei.svg"]) routes.set("/" + path, new URL(path, repo));
const types = { html: "text/html; charset=utf-8", js: "text/javascript; charset=utf-8", css: "text/css; charset=utf-8", svg: "image/svg+xml" };
const server = http.createServer(async (req, res) => {
  const pathname = new URL(req.url, "http://127.0.0.1").pathname;
  if (pathname === "/") { res.writeHead(302, { Location: prefix }); res.end(); return; }
  if (pathname === "/favicon.ico") { res.writeHead(204); res.end(); return; }
  const file = routes.get(pathname);
  if (!file || !["GET", "HEAD"].includes(req.method)) { res.writeHead(404); res.end("Not found"); return; }
  try {
    const body = await readFile(file);
    res.writeHead(200, { "Content-Type": types[fileURLToPath(file).split(".").pop()] || "application/octet-stream", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" });
    res.end(req.method === "HEAD" ? undefined : body);
  } catch { res.writeHead(500); res.end("Preview resource unavailable"); }
});
server.listen(0, "127.0.0.1", () => console.log(`Prototype: http://127.0.0.1:${server.address().port}${prefix}`));
