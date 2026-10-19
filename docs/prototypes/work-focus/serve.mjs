import http from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { startPreviewServer } from "../../../scripts/ui-preview/server.mjs";

const root = path.dirname(fileURLToPath(import.meta.url));
const prefix = "/__work-focus/";
const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8" };
const files = new Set(["index.html", "lab.css", "lab.js", "proposal.css"]);

export async function startWorkFocusPrototype({ port = 14128 } = {}) {
  const product = await startPreviewServer({ port: 0 });
  const server = http.createServer(async (request, response) => {
    const url = new URL(request.url, "http://127.0.0.1");
    if (url.pathname === "/__work-focus") {
      response.writeHead(302, { Location: prefix }); response.end(); return;
    }
    if (url.pathname.startsWith(prefix)) {
      const name = url.pathname.slice(prefix.length) || "index.html";
      if (!files.has(name)) { response.writeHead(404); response.end("Not found"); return; }
      const body = await readFile(path.join(root, name));
      response.writeHead(200, { "Content-Type": types[path.extname(name)], "Cache-Control": "no-store" });
      response.end(body); return;
    }
    const upstream = http.request(new URL(request.url, product.origin), { method: request.method, headers: request.headers }, result => {
      response.writeHead(result.statusCode, result.headers); result.pipe(response);
    });
    upstream.on("error", error => { response.writeHead(502); response.end(error.message); });
    request.pipe(upstream);
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject); server.listen(port, "127.0.0.1", resolve);
  });
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    close: async () => { await new Promise(resolve => server.close(resolve)); await product.close(); }
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const position = process.argv.indexOf("--port");
  const prototype = await startWorkFocusPrototype({ port: position < 0 ? 14128 : Number(process.argv[position + 1]) });
  console.log(`工作栏原型 ${prototype.origin}${prefix}`);
}
