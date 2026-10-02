import http from 'node:http';
import { readFile } from 'node:fs/promises';
const files = new Map([
  ['/', ['index.html', 'text/html']],
  ['/index.html', ['index.html', 'text/html']],
  ['/workbench.css', ['workbench.css', 'text/css']],
  ['/workbench.js', ['workbench.js', 'text/javascript']],
]);
const server = http.createServer(async (req, res) => {
  const pathname = new URL(req.url, 'http://127.0.0.1').pathname;
  if (pathname === '/favicon.ico') { res.writeHead(204); res.end(); return; }
  const entry = files.get(pathname);
  if (!entry || !['GET', 'HEAD'].includes(req.method)) { res.writeHead(404); res.end(); return; }
  try {
    const bytes = await readFile(new URL(entry[0], import.meta.url));
    res.writeHead(200, { 'Content-Type': `${entry[1]}; charset=utf-8`, 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
    res.end(req.method === 'HEAD' ? undefined : bytes);
  } catch { res.writeHead(500); res.end('Preview resource unavailable'); }
});
server.listen(0, '127.0.0.1', () => console.log(`Prototype: http://127.0.0.1:${server.address().port}/`));
