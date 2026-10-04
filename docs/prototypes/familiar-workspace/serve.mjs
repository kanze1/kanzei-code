import http from 'node:http';
import { readFile, access } from 'node:fs/promises';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
import { spawn } from 'node:child_process';

const projectRoot=fileURLToPath(new URL('../../../',import.meta.url));
async function launchVSCode() {
  const candidates=[process.env.LOCALAPPDATA&&path.join(process.env.LOCALAPPDATA,'Programs','Microsoft VS Code','Code.exe'),process.env.ProgramFiles&&path.join(process.env.ProgramFiles,'Microsoft VS Code','Code.exe')].filter(Boolean);
  let executable;
  for(const candidate of candidates) {try {await access(candidate);executable=candidate;break;}catch {}}
  if(!executable)throw new Error('未找到已安装的 VS Code。');
  await new Promise((resolve,reject)=>{
    const child=spawn(executable,[projectRoot],{detached:true,stdio:'ignore',windowsHide:true});
    child.once('error',reject);child.once('spawn',()=>{child.unref();resolve();});
  });
}

const files = new Map([
  ['/', ['index.html', 'text/html; charset=utf-8']],
  ['/index.html', ['index.html', 'text/html; charset=utf-8']],
  ['/style.css', ['style.css', 'text/css; charset=utf-8']],
  ['/app.js', ['app.js', 'text/javascript; charset=utf-8']],
  ['/brand.svg', ['brand.svg', 'image/svg+xml']],
  ['/composer-before.png', ['composer-before.png', 'image/png']],
  ['/work-list-before.png', ['work-list-before.png', 'image/png']],
]);
export async function startServer(port = 0, {launchEditor=launchVSCode} = {}) {
  const server = http.createServer(async (req, res) => {
    const name = new URL(req.url, 'http://127.0.0.1').pathname;
    if(name==='/api/open-editor') {
      const json=(code,value)=>{res.writeHead(code,{'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store'});res.end(JSON.stringify(value));};
      if(req.method!=='POST'){json(405,{error:'请使用项目的 VS Code 按钮。'});return;}
      const expected=`127.0.0.1:${server.address().port}`;
      if(req.headers.host!==expected||req.headers.origin!==`http://${expected}`){json(403,{error:'只能从本地原型页面打开编辑器。'});return;}
      if(!req.headers['content-type']?.startsWith('application/json')){json(415,{error:'请求格式不正确。'});return;}
      let body='';
      try {
        for await(const chunk of req){body+=chunk;if(Buffer.byteLength(body)>2048){json(413,{error:'请求过长。'});return;}}
        const data=JSON.parse(body);
        if(data.project!=='kanzei'||Object.keys(data).some(k=>k!=='project')){json(400,{error:'这个示例项目没有绑定目录。'});return;}
        await launchEditor(projectRoot);json(200,{ok:true,project:'kanzei'});
      } catch(error){json(error instanceof SyntaxError?400:500,{error:error instanceof SyntaxError?'请求格式不正确。':error.message});}
      return;
    }
    if (name === '/favicon.ico') { res.writeHead(204); res.end(); return; }
    const entry = files.get(name);
    if (!entry || !['GET', 'HEAD'].includes(req.method)) { res.writeHead(404); res.end(); return; }
    try {
      const content = await readFile(new URL(entry[0], import.meta.url));
      res.writeHead(200, { 'Content-Type': entry[1], 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
      res.end(req.method === 'HEAD' ? undefined : content);
    } catch { res.writeHead(500); res.end('Preview resource unavailable'); }
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
  return { server, origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise(resolve => server.close(resolve)) };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const flag = process.argv.indexOf('--port');
  const port = flag >= 0 ? Number(process.argv[flag + 1]) : 14105;
  const result = await startServer(port);
  console.log(`Kanzei prototype: ${result.origin}`);
}
