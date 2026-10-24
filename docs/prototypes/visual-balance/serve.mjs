import http from "node:http";
import { readFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { startPreviewServer, injectMock } from "../../../scripts/ui-preview/server.mjs";

const root = path.dirname(fileURLToPath(import.meta.url));
const repository = path.resolve(root, "../../..");
const git = promisify(execFile);
export const baselineTag = "build-8fb9f71d";
const prefix = "/__visual-balance/";
const productRoot = "crates/kanzei-app/ui/";
const types = new Map([
  [".html", "text/html; charset=utf-8"], [".css", "text/css; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"], [".mjs", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"], [".svg", "image/svg+xml"],
  [".png", "image/png"], [".ico", "image/x-icon"], [".woff2", "font/woff2"],
  [".ttf", "font/ttf"], [".wasm", "application/wasm"], [".map", "application/json"],
]);
const files = new Map([
  ["index.html", "text/html; charset=utf-8"],
  ["review.css", "text/css; charset=utf-8"],
  ["review.js", "text/javascript; charset=utf-8"],
  ["preview.js", "text/javascript; charset=utf-8"],
  ["proposal.css", "text/css; charset=utf-8"],
]);

export async function startVisualPrototype({ port = 14137 } = {}) {
  const [revision, tree] = await Promise.all([
    git("git", ["rev-parse", `${baselineTag}^{commit}`], { cwd: repository }),
    git("git", ["ls-tree", "-r", "--name-only", baselineTag, "--", productRoot], { cwd: repository }),
  ]);
  const baseline = { tag: baselineTag, commit: revision.stdout.trim() };
  const releaseFiles = new Set(tree.stdout.trim().split(/\r?\n/).map(file => file.slice(productRoot.length)));
  const sourceCache = new Map();
  const releaseSource = file => {
    if (!sourceCache.has(file)) sourceCache.set(file, git("git", ["show", `${baselineTag}:${productRoot}${file}`], {
      cwd: repository, encoding: "buffer", maxBuffer: 32 * 1024 * 1024,
    }).then(result => result.stdout));
    return sourceCache.get(file);
  };
  const product = await startPreviewServer({ port: 0 });
  async function handle(request, response) {
    const url = new URL(request.url, "http://127.0.0.1");
    if (!["GET", "HEAD"].includes(request.method)) {
      response.writeHead(405, { Allow: "GET, HEAD" }); response.end(); return;
    }
    if (url.pathname === "/__visual-balance") {
      response.writeHead(302, { Location: prefix }); response.end(); return;
    }
    if (url.pathname.startsWith(prefix)) {
      const name = url.pathname.slice(prefix.length) || "index.html";
      if (name === "baseline.json") {
        response.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store" });
        response.end(request.method === "HEAD" ? undefined : JSON.stringify(baseline)); return;
      }
      if (!files.has(name)) {
        response.writeHead(404); response.end("Not found"); return;
      }
      const body = await readFile(path.join(root, name));
      response.writeHead(200, { "Content-Type": files.get(name), "Cache-Control": "no-store" });
      response.end(request.method === "HEAD" ? undefined : body); return;
    }
    if (!url.pathname.startsWith("/__preview/")) {
      if (url.pathname === "/favicon.ico") { response.writeHead(204); response.end(); return; }
      const file = url.pathname === "/" ? "index.html" : decodeURIComponent(url.pathname.slice(1));
      if (!releaseFiles.has(file)) { response.writeHead(404); response.end("Not in release"); return; }
      const source = await releaseSource(file);
      const body = file === "index.html" ? injectMock(source.toString("utf8")) : source;
      response.writeHead(200, {
        "Content-Type": types.get(path.extname(file)) || "application/octet-stream",
        "Cache-Control": "no-store", "X-Kanzei-Baseline": baselineTag,
      });
      response.end(request.method === "HEAD" ? undefined : body); return;
    }
    const upstream = http.request(new URL(request.url, product.origin), { method: request.method, headers: request.headers }, result => {
      response.writeHead(result.statusCode, result.headers); result.pipe(response);
    });
    upstream.on("error", error => { response.writeHead(502); response.end(error.message); });
    request.pipe(upstream);
  }
  const server = http.createServer((request, response) => {
    handle(request, response).catch(error => { response.writeHead(500); response.end(error.message); });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject); server.listen(port, "127.0.0.1", resolve);
  });
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    baseline,
    close: async () => { await new Promise(resolve => server.close(resolve)); await product.close(); },
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const position = process.argv.indexOf("--port");
  const prototype = await startVisualPrototype({ port: position < 0 ? 14137 : Number(process.argv[position + 1]) });
  console.log(`Kanzei 视觉原型 ${prototype.origin}${prefix}`);
}
