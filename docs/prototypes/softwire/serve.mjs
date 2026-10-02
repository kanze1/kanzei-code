import http from 'node:http';
import { readFile } from 'node:fs/promises';

const resources = new Map([
  ['/', ['index.html', 'text/html']],
  ['/index.html', ['index.html', 'text/html']],
  ['/softwire.css', ['softwire.css', 'text/css']],
  ['/softwire.js', ['softwire.js', 'text/javascript']],
]);
const server = http.createServer(async (req, res) => {
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  if (path === '/favicon.ico') { res.writeHead(204); res.end(); return; }
  const item = resources.get(path);
  if (!item || !['GET', 'HEAD'].includes(req.method)) { res.writeHead(404); res.end(); return; }
  try {
    const body = await readFile(new URL(item[0], import.meta.url));
    res.writeHead(200, { 'Content-Type': `${item[1]}; charset=utf-8`, 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
    res.end(req.method === 'HEAD' ? undefined : body);
  } catch { res.writeHead(500); res.end('Preview resource unavailable'); }
});
const port = Number(process.argv[2] ?? 14078);
if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('Port must be an integer from 0 to 65535');
server.listen(port, '127.0.0.1', () => console.log(`Softwire experience: http://127.0.0.1:${server.address().port}/`));
